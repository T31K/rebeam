//! `rebeam acp` — the Agent Client Protocol bridge.
//!
//! Any ACP-speaking agent (our `claude-code-acp`, `codex-acp`, Gemini CLI's
//! `--experimental-acp`) is invoked the same way: we play the *client* role,
//! spawn the agent as a subprocess over stdio, run one prompt, and translate
//! its session events into Rebeam's model.
//!
//! This is what makes the bridge agent-agnostic. Instead of a bespoke parser
//! per provider, one ACP client handles them all:
//!
//! - `session/update` → streaming text + tool-call telemetry (Rebeam `Status`).
//! - `session/request_permission` → a human approval (auto-answered for now;
//!   `claude -p` does not surface these, so real approval needs the PreToolUse
//!   hook path — tracked separately).
//!
//! Two modes: `Terminal` (the interactive `rebeam acp` probe) and `Gateway`
//! (driven by `rebeam gateway`: posts tool telemetry to the relay as `Status`
//! and prints the final reply on stdout for the gateway to `Send`).

use std::io::Write;
use std::str::FromStr;
use std::sync::Arc;

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};
use anyhow::{anyhow, bail, Result};
use owo_colors::OwoColorize;
use rebeam_core::{Command as Cmd, StatusState};
use tokio::sync::Mutex;

/// Resolve a provider name to the command that launches its ACP adapter, with
/// no manual adapter install. `rebeam acp --command "<launcher>"` overrides.
pub fn adapter_command(provider: &str) -> Result<String> {
    match provider.trim().to_ascii_lowercase().as_str() {
        // Our own CLI-driven adapter: uses the Claude Code subscription login,
        // no Node, no API key. This is the default because it's what users have.
        "claude" => claude_code_adapter(),
        // The official Agent-SDK adapter — API-key billing only (Anthropic
        // blocks subscription auth for the SDK). Opt in with `--provider claude-api`.
        "claude-api" => npx("@agentclientprotocol/claude-agent-acp"),
        "codex" => npx("@zed-industries/codex-acp"),
        // The Gemini CLI is an ACP agent itself — no adapter needed.
        "gemini" => native("gemini --experimental-acp", "gemini"),
        other => bail!(
            "unknown provider {other:?}. Known: claude, claude-api, codex, gemini. \
             For anything else pass --command \"<acp launcher>\"."
        ),
    }
}

/// The `claude-code-acp` binary rebeam ships alongside itself — prefer the one
/// next to the running `rebeam`, else fall back to PATH.
fn claude_code_adapter() -> Result<String> {
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("claude-code-acp")))
        .filter(|path| path.is_file());
    let command = match sibling {
        Some(path) => path.display().to_string(),
        None if on_path("claude-code-acp") => "claude-code-acp".to_string(),
        None => bail!("the claude-code-acp adapter is missing (not next to rebeam or on PATH)"),
    };
    if !on_path("claude") {
        eprintln!(
            "{} the `claude` CLI is not on PATH — the adapter needs it; run `claude login`.",
            "warning".yellow()
        );
    }
    Ok(command)
}

/// An npm-published adapter, launched on demand. `npx -y` runs it without a
/// global install and caches it after the first fetch.
fn npx(package: &str) -> Result<String> {
    if !on_path("npx") {
        bail!(
            "`npx` (Node.js) is needed to launch the {package} adapter.\n  \
             Install Node 18+, or pass --command \"<acp launcher>\" to point at your own."
        );
    }
    Ok(format!("npx -y {package}"))
}

/// An agent CLI that speaks ACP natively — we just add its flag.
fn native(command: &str, bin: &str) -> Result<String> {
    if !on_path(bin) {
        bail!("`{bin}` is not installed or not on PATH.");
    }
    Ok(command.to_string())
}

fn on_path(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(bin);
                candidate.is_file()
                    || candidate.with_extension("exe").is_file()
                    || candidate.with_extension("cmd").is_file()
            })
        })
        .unwrap_or(false)
}

/// How permission requests are answered while the approval hook is unbuilt.
#[derive(Clone, Copy)]
pub enum Approve {
    /// Grant every request (pick an allow-* option).
    Auto,
    /// Refuse every request (pick a reject-* option, else cancel).
    Deny,
}

/// How a session is driven and where its events go.
pub enum Mode {
    /// Stream to the terminal (the `rebeam acp` probe).
    Terminal,
    /// Drive from the gateway: post `Status` per tool call to the relay, and
    /// print the final reply to stdout for the gateway to `Send`.
    Gateway {
        relay: String,
        chat: String,
        agent: String,
        token: String,
    },
}

/// Terminal, or a relay-posting sink with an accumulating reply buffer.
#[derive(Clone)]
enum Sink {
    Terminal,
    Gateway(Arc<GatewaySink>),
}

struct GatewaySink {
    http: reqwest::Client,
    relay: String,
    chat: String,
    agent: String,
    text: Mutex<String>,
}

/// Spawn an ACP agent, run one prompt, and route its events per `mode`.
///
/// `cwd` is the agent's working directory — the project it operates in, so
/// `claude` reads that project's `CLAUDE.md`/docs. Defaults to the process cwd.
pub async fn run(
    command: &str,
    prompt: &str,
    approve: Approve,
    mode: Mode,
    cwd: Option<std::path::PathBuf>,
) -> Result<()> {
    let agent = AcpAgent::from_str(command)
        .map_err(|e| anyhow!("cannot launch agent {command:?}: {e}"))?;
    let prompt = prompt.to_string();
    let session_cwd = cwd.unwrap_or_else(default_cwd);

    let sink = match mode {
        Mode::Terminal => {
            eprintln!("{} {}", "acp".green().bold(), command.dimmed());
            Sink::Terminal
        }
        Mode::Gateway {
            relay,
            chat,
            agent: author,
            token,
        } => {
            let mut headers = reqwest::header::HeaderMap::new();
            let value = format!("Bearer {token}")
                .parse()
                .map_err(|_| anyhow!("invalid machine token"))?;
            headers.insert(reqwest::header::AUTHORIZATION, value);
            let http = reqwest::Client::builder()
                .default_headers(headers)
                .build()
                .map_err(|e| anyhow!("http client: {e}"))?;
            Sink::Gateway(Arc::new(GatewaySink {
                http,
                relay,
                chat,
                agent: author,
                text: Mutex::new(String::new()),
            }))
        }
    };

    let note_sink = sink.clone();
    let end_sink = sink.clone();

    agent_client_protocol::Client
        .builder()
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                handle_update(&note_sink, notification.update).await;
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                let outcome = decide(&request, approve);
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection: ConnectionTo<Agent>| async move {
            connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            let session = connection
                .send_request(NewSessionRequest::new(session_cwd))
                .block_task()
                .await?;
            connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("acp session failed: {e}"))?;

    // Gateway mode returns the whole reply at once, on stdout, for the gateway
    // to post as a message. Terminal mode already streamed it live.
    if let Sink::Gateway(gateway) = &end_sink {
        print!("{}", gateway.text.lock().await);
        let _ = std::io::stdout().flush();
    }

    Ok(())
}

/// Route one streamed event to the terminal or the relay.
async fn handle_update(sink: &Sink, update: SessionUpdate) {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            let text = content_to_string(&chunk.content);
            match sink {
                Sink::Terminal => {
                    print!("{text}");
                    let _ = std::io::stdout().flush();
                }
                Sink::Gateway(gateway) => gateway.text.lock().await.push_str(&text),
            }
        }
        SessionUpdate::AgentThoughtChunk(chunk) => {
            if let Sink::Terminal = sink {
                eprint!("{}", content_to_string(&chunk.content).dimmed());
            }
        }
        SessionUpdate::ToolCall(call) => match sink {
            Sink::Terminal => eprintln!(
                "{} {} {}",
                "⚙".yellow(),
                call.title.bold(),
                format!("{:?}·{:?}", call.kind, call.status)
                    .to_lowercase()
                    .dimmed(),
            ),
            // Live tool chip in the app: broadcast an (ephemeral) Status event.
            Sink::Gateway(gateway) => {
                let (tool, target) = split_title(&call.title);
                let command = Cmd::Status {
                    chat: gateway.chat.clone(),
                    author: gateway.agent.clone(),
                    state: StatusState::Tool,
                    label: None,
                    tool: Some(tool),
                    target,
                };
                let _ = gateway
                    .http
                    .post(format!("{}/commands", gateway.relay))
                    .json(&command)
                    .send()
                    .await;
            }
        },
        _ => {}
    }
}

/// `"Bash: ls -1"` → `("Bash", Some("ls -1"))`; `"Read"` → `("Read", None)`.
fn split_title(title: &str) -> (String, Option<String>) {
    match title.split_once(": ") {
        Some((tool, target)) => (tool.to_string(), Some(target.to_string())),
        None => (title.to_string(), None),
    }
}

/// Answer a permission request with a static policy.
///
/// `claude -p` does not surface permission requests, so this is effectively a
/// no-op for the Claude path today; real approval is the PreToolUse-hook work.
fn decide(request: &RequestPermissionRequest, approve: Approve) -> RequestPermissionOutcome {
    let want_allow = matches!(approve, Approve::Auto);
    let pick = request
        .options
        .iter()
        .find(|option| is_allow(option.kind) == want_allow)
        .or_else(|| request.options.first());
    match pick {
        Some(option) => RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(
            option.option_id.clone(),
        )),
        None => RequestPermissionOutcome::Cancelled,
    }
}

fn is_allow(kind: PermissionOptionKind) -> bool {
    matches!(
        kind,
        PermissionOptionKind::AllowOnce | PermissionOptionKind::AllowAlways
    )
}

fn content_to_string(block: &ContentBlock) -> String {
    match block {
        ContentBlock::Text(text) => text.text.clone(),
        ContentBlock::Image(image) => format!("[image: {}]", image.mime_type),
        ContentBlock::Audio(audio) => format!("[audio: {}]", audio.mime_type),
        ContentBlock::ResourceLink(link) => link.uri.clone(),
        _ => String::new(),
    }
}

fn default_cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"))
}

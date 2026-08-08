//! `rebeam acp` — the Agent Client Protocol bridge.
//!
//! Any ACP-speaking agent (Zed's `claude-agent-acp`, `codex-acp`, Gemini CLI's
//! `--experimental-acp`) is invoked the same way: we play the *client* role,
//! spawn the agent as a subprocess over stdio, run one prompt, and translate
//! its session events into Rebeam's model.
//!
//! This is what makes the bridge agent-agnostic. Instead of a bespoke parser
//! per provider, one ACP client handles them all:
//!
//! - `session/update` → streaming text + tool-call telemetry (Rebeam `Status`).
//! - `session/request_permission` → a human approval. Today it applies a static
//!   policy; the marked call site is exactly where `rebeam ask` will route the
//!   request to the owner's phone and map the answer back to a `PermissionOption`.

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PermissionOptionKind, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, TextContent,
};
use agent_client_protocol::{AcpAgent, Agent, ConnectionTo};
use anyhow::{anyhow, bail, Result};
use owo_colors::OwoColorize;
use std::io::Write;
use std::str::FromStr;

/// Resolve a provider name to the command that launches its ACP adapter, with
/// no manual adapter install. The official adapters ship on npm, so we
/// `npx`-launch them (npx fetches + caches on first run); Gemini speaks ACP
/// natively. `rebeam acp --command "<launcher>"` overrides this for anything
/// else.
pub fn adapter_command(provider: &str) -> Result<String> {
    match provider.trim().to_ascii_lowercase().as_str() {
        // Official, wraps the Claude Agent SDK. Needs a signed-in Claude
        // (ANTHROPIC_API_KEY or `claude` login) to actually answer.
        "claude" => npx("@agentclientprotocol/claude-agent-acp"),
        "codex" => npx("@zed-industries/codex-acp"),
        // The Gemini CLI is an ACP agent itself — no adapter needed.
        "gemini" => native("gemini --experimental-acp", "gemini"),
        other => bail!(
            "unknown provider {other:?}. Known: claude, codex, gemini. \
             For anything else pass --command \"<acp launcher>\"."
        ),
    }
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

/// How permission requests are answered while the `rebeam ask` wiring is a stub.
#[derive(Clone, Copy)]
pub enum Approve {
    /// Grant every request (pick an allow-* option).
    Auto,
    /// Refuse every request (pick a reject-* option, else cancel).
    Deny,
}

/// Spawn an ACP agent, run one prompt, and stream its events to the terminal.
pub async fn run(command: &str, prompt: &str, approve: Approve) -> Result<()> {
    let agent = AcpAgent::from_str(command)
        .map_err(|e| anyhow!("cannot launch agent {command:?}: {e}"))?;
    let prompt = prompt.to_string();

    eprintln!("{} {}", "acp".green().bold(), command.dimmed());

    agent_client_protocol::Client
        .builder()
        // Everything the agent tells us mid-turn: text, thoughts, tool calls.
        .on_receive_notification(
            async move |notification: SessionNotification, _cx| {
                render_update(notification.update);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        // The agent pauses its own turn to ask permission for a tool call.
        .on_receive_request(
            async move |request: RequestPermissionRequest, responder, _connection| {
                let outcome = decide(&request, approve);
                responder.respond(RequestPermissionResponse::new(outcome))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, move |connection: ConnectionTo<Agent>| async move {
            let init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;
            eprintln!("{} {:?}", "✓ initialized".green(), init.agent_info);

            let session = connection
                .send_request(NewSessionRequest::new(cwd()))
                .block_task()
                .await?;
            eprintln!("{}", "✓ session".green());

            let done = connection
                .send_request(PromptRequest::new(
                    session.session_id,
                    vec![ContentBlock::Text(TextContent::new(prompt))],
                ))
                .block_task()
                .await?;

            eprintln!("\n{} {:?}", "done".green().bold(), done.stop_reason);
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("acp session failed: {e}"))?;

    Ok(())
}

/// Render one streamed session event. Text streams to stdout so the reply reads
/// naturally; telemetry goes to stderr so it never contaminates the message.
fn render_update(update: SessionUpdate) {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => {
            print!("{}", content_to_string(&chunk.content));
            let _ = std::io::stdout().flush();
        }
        SessionUpdate::AgentThoughtChunk(chunk) => {
            eprint!("{}", content_to_string(&chunk.content).dimmed());
        }
        // The ToolChips source: name + kind + status, one line per tool.
        SessionUpdate::ToolCall(call) => {
            eprintln!(
                "{} {} {}",
                "⚙".yellow(),
                call.title.bold(),
                format!("{:?}·{:?}", call.kind, call.status).to_lowercase().dimmed(),
            );
        }
        _ => {}
    }
}

/// Answer a permission request.
///
/// **This is the `rebeam ask` integration point.** Today it applies a static
/// policy; the real bridge will post an `Ask` to the chat here, block on the
/// owner's choice, and map their answer onto one of `request.options`.
fn decide(request: &RequestPermissionRequest, approve: Approve) -> RequestPermissionOutcome {
    let want_allow = matches!(approve, Approve::Auto);
    eprintln!(
        "{} {} — {}",
        "ask".magenta().bold(),
        request.tool_call.tool_call_id.0.dimmed(),
        if want_allow { "auto-allow".green().to_string() } else { "deny".red().to_string() },
    );

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

fn cwd() -> std::path::PathBuf {
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"))
}

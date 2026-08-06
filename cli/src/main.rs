//! `rebeam` — the CLI is the protocol.
//!
//! Every verb here maps to exactly one [`rebeam_core::Command`]. Any agent that
//! can run a shell command is integrated; no SDK, no config file, no MCP.
//!
//! ```text
//! rebeam send   -c war-room -m "deploy finished"
//! rebeam ask    -c war-room -m "Deploy to prod?" --options yes,no
//! rebeam status -c war-room thinking -m "planning the fix"
//! rebeam listen -c war-room --json
//! ```

mod up;
mod update;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use owo_colors::OwoColorize;
use rebeam_core::{
    Chat, Command as Cmd, Event, HistoryGrant, Invite, Member, Membership, Message, StatusState,
};

/// Exit code for an unanswered `ask`, matching GNU `timeout`.
const EXIT_TIMEOUT: u8 = 124;

#[derive(Parser)]
#[command(name = "rebeam", version, about = "Talk to your agents from anywhere.")]
struct Cli {
    /// Relay URL (env: REBEAM_RELAY)
    #[arg(
        long,
        global = true,
        env = "REBEAM_RELAY",
        default_value = "http://127.0.0.1:8787"
    )]
    relay: String,

    /// Act as this member (env: REBEAM_AS)
    #[arg(
        long = "as",
        global = true,
        env = "REBEAM_AS",
        default_value = "claude-main"
    )]
    author: String,

    #[command(subcommand)]
    command: Verb,
}

#[derive(Subcommand)]
enum Verb {
    /// Install the newest Rebeam CLI release
    Update {
        /// Install a specific release tag, for example `v0.2.0`
        version: Option<String>,
        /// GitHub repository that publishes Rebeam releases
        #[arg(long, env = "REBEAM_RELEASE_REPO", default_value = "T31K/rebeam")]
        repo: String,
        /// Only report whether an update is available
        #[arg(long)]
        check: bool,
    },

    /// Post a message to a chat
    Send {
        #[arg(short, long)]
        chat: String,
        /// Message text; omit to read stdin
        #[arg(short = 'm', long)]
        message: Option<String>,
    },

    /// Ask a human to choose, and block until they do
    Ask {
        #[arg(short, long)]
        chat: String,
        #[arg(short = 'm', long)]
        message: String,
        /// Comma-separated choices
        #[arg(long, value_delimiter = ',', default_value = "yes,no")]
        options: Vec<String>,
        /// Seconds to wait before giving up
        #[arg(long, default_value_t = 3600)]
        timeout: u64,
    },

    /// Report what this agent is doing right now (never persisted)
    Status {
        #[arg(value_enum)]
        state: State,
        #[arg(short, long)]
        chat: String,
        #[arg(short = 'm', long)]
        message: Option<String>,
        /// Tool name, when state is `tool` — Write, Bash, Read
        #[arg(long)]
        tool: Option<String>,
        /// What the tool acted on — a path, a command, a URL
        #[arg(long)]
        target: Option<String>,
    },

    /// Stream events from the relay, one JSON object per line
    Listen {
        /// Only this chat
        #[arg(short, long)]
        chat: Option<String>,
        /// Raw JSON instead of the human-readable rendering
        #[arg(long)]
        json: bool,
    },

    /// Run the bridge: wake local agents when messages arrive for them
    Gateway {
        /// Run the local agent gateway
        #[arg(short, long)]
        config: Option<PathBuf>,
    },

    /// Install, inspect, or remove the background gateway service
    Service {
        #[arg(value_enum)]
        action: ServiceAction,
        /// Override the global Rebeam config location
        #[arg(short, long)]
        config: Option<PathBuf>,
    },

    /// Mint an invite to a chat. Hand the code to whoever — or whatever — joins
    Invite {
        #[arg(short, long)]
        chat: String,
        /// How far back the new member may read
        #[arg(long, value_enum, default_value_t = History::All)]
        history: History,
    },

    /// Connect an agent to a chat from this machine
    Connect {
        /// The code from the app, `RB-7K2M-QX4P`
        code: String,
        /// Agent runtime selected in the Rebeam app
        #[arg(long, value_enum)]
        provider: AgentProvider,
        /// What this agent is called in the member list
        #[arg(short, long)]
        name: String,
        /// How to run it. `$REBEAM_TEXT` is the incoming message
        #[arg(short, long)]
        exec: Option<String>,
        /// Override the global Rebeam config location
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Connect only; don't start the gateway
        #[arg(long)]
        no_gateway: bool,
    },

    /// Pair this machine with a Rebeam workspace
    Pair { code: String },

    /// List chats
    Chats,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum History {
    /// The whole backlog, like being added to a Slack channel
    All,
    /// Only what happens from here on
    None,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum State {
    Thinking,
    Tool,
    Done,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum AgentProvider {
    Claude,
    Codex,
    Hermes,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum ServiceAction {
    Install,
    Status,
    Uninstall,
}

impl AgentProvider {
    fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Hermes => "hermes",
        }
    }
}

impl From<State> for StatusState {
    fn from(s: State) -> Self {
        match s {
            State::Thinking => StatusState::Thinking,
            State::Tool => StatusState::Tool,
            State::Done => StatusState::Done,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{} {err:#}", "error".red().bold());
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();

    match &cli.command {
        Verb::Update {
            version,
            repo,
            check,
        } => {
            let service_was_running = bridge_service_running();
            let updated = update::run(version.as_deref(), repo, *check).await?;
            if updated && service_was_running {
                let config = default_config_path();
                if config.exists() {
                    install_bridge_service(&cli.relay, &config)
                        .context("the CLI updated, but the gateway service did not restart")?;
                    println!("{} gateway restarted", "✓".green());
                }
            }
        }

        Verb::Send { chat, message } => {
            let text = match message {
                Some(m) => m.clone(),
                None => read_stdin()?,
            };
            let event = send(
                &client,
                &cli.relay,
                Cmd::Send {
                    chat: chat.clone(),
                    author: cli.author.clone(),
                    text,
                    idem: None,
                },
            )
            .await?;
            if let Event::Message { message } = event {
                println!("{} {}", "sent".green(), message.id.dimmed());
            }
        }

        Verb::Ask {
            chat,
            message,
            options,
            timeout,
        } => {
            let event = send(
                &client,
                &cli.relay,
                Cmd::Ask {
                    chat: chat.clone(),
                    author: cli.author.clone(),
                    text: message.clone(),
                    options: options.clone(),
                },
            )
            .await?;
            let Event::Message { message } = event else {
                bail!("relay did not open an ask");
            };
            eprintln!(
                "{} {}  {}",
                "waiting".yellow(),
                message.text,
                format!("[{}]", options.join(" / ")).dimmed()
            );

            match await_answer(&client, &cli.relay, &message.id, *timeout).await? {
                Some(choice) => println!("{choice}"),
                None => {
                    eprintln!("{} no answer in {timeout}s", "timeout".red());
                    return Ok(ExitCode::from(EXIT_TIMEOUT));
                }
            }
        }

        Verb::Status {
            state,
            chat,
            message,
            tool,
            target,
        } => {
            send(
                &client,
                &cli.relay,
                Cmd::Status {
                    chat: chat.clone(),
                    author: cli.author.clone(),
                    state: (*state).into(),
                    label: message.clone(),
                    tool: tool.clone(),
                    target: target.clone(),
                },
            )
            .await?;
        }

        Verb::Listen { chat, json } => {
            listen(&client, &cli.relay, chat.as_deref(), *json).await?;
        }

        Verb::Gateway { config } => {
            up::run(
                &cli.relay,
                &config.clone().unwrap_or_else(default_config_path),
            )
            .await?;
        }

        Verb::Service { action, config } => {
            let config = config.clone().unwrap_or_else(default_config_path);
            match action {
                ServiceAction::Install => {
                    let log = install_bridge_service(&cli.relay, &config)?;
                    println!("{} {}", "✓".green(), "gateway service installed".bold());
                    println!("  {}", format!("log: {}", log.display()).dimmed());
                }
                ServiceAction::Status => {
                    let running = bridge_service_running();
                    println!(
                        "{} gateway {}",
                        if running {
                            "●".green().to_string()
                        } else {
                            "○".dimmed().to_string()
                        },
                        if running {
                            "running".green().to_string()
                        } else {
                            "stopped".dimmed().to_string()
                        },
                    );
                }
                ServiceAction::Uninstall => {
                    uninstall_bridge_service()?;
                    println!("{} {}", "✓".green(), "gateway service removed".bold());
                }
            }
        }

        Verb::Invite { chat, history } => {
            let body = serde_json::json!({
                "chat": chat,
                "history": match history {
                    History::All => serde_json::json!({ "t": "all" }),
                    History::None => serde_json::json!({ "t": "none" }),
                },
            });
            let res = client
                .post(format!("{}/invites", cli.relay))
                .json(&body)
                .send()
                .await
                .with_context(|| format!("cannot reach the relay at {}", cli.relay))?;
            if !res.status().is_success() {
                bail!("{}", error_body(res).await);
            }
            let invite: Invite = res.json().await?;

            // The code is the value; everything else is for the human reading
            // over its shoulder. Keeps `rebeam invite -c x | pbcopy` honest.
            println!("{}", invite.code);
            eprintln!(
                "\n  {}\n  {}\n",
                "run this where the agent lives:".dimmed(),
                format!("rebeam connect {} --name <agent-name>", invite.code).cyan()
            );
            let mins = (invite.expires_at - now_millis()) / 60_000;
            eprintln!(
                "  {}",
                format!(
                    "expires in {mins}m · single use · sees {}",
                    match invite.history {
                        HistoryGrant::All => "the whole backlog",
                        _ => "only new messages",
                    }
                )
                .dimmed()
            );
        }

        Verb::Connect {
            code,
            provider,
            name,
            exec,
            config,
            no_gateway,
        } => {
            let provider = provider.as_str();
            let exec = match exec {
                Some(e) => e.clone(),
                None => {
                    if !which(provider) {
                        bail!("{provider} is not installed or is not on PATH");
                    }
                    managed_exec(provider).unwrap().to_string()
                }
            };

            let res = client
                .post(format!("{}/connect", cli.relay))
                .json(&serde_json::json!({
                    "code": code,
                    "name": name,
                    "host": hostname(),
                }))
                .send()
                .await
                .with_context(|| format!("cannot reach the relay at {}", cli.relay))?;
            if !res.status().is_success() {
                bail!("{}", error_body(res).await);
            }
            let joined: Joined = res.json().await?;

            // The relay owns which chats and how far back. This file records
            // only how to run the thing.
            let config = config.clone().unwrap_or_else(default_config_path);
            write_agent(&config, &joined.member.name, provider, &exec)?;

            println!(
                "{} {} joined {}",
                "✓".green(),
                joined.member.name.bold(),
                joined.chat.name.bold()
            );
            let backlog = if joined.membership.history_floor_seq == 0 {
                "the whole backlog".to_string()
            } else {
                format!("messages after #{}", joined.membership.history_floor_seq)
            };
            println!("  {}", format!("can read {backlog}").dimmed());
            println!(
                "  {}",
                format!("exec recorded in {}", config.display()).dimmed()
            );

            if *no_gateway {
                println!("\n  {}", "rebeam gateway".cyan());
            } else {
                // Joining and then not listening is never what anyone wanted,
                // and a command that ends by telling you the next step should
                // just take it. Detached, so joining stays a one-shot act
                // rather than a command that blocks forever.
                match install_bridge_service(&cli.relay, &config) {
                    Ok(log) => {
                        println!(
                            "\n  {} {}",
                            "●".green(),
                            "gateway running in background".bold()
                        );
                        println!("  {}", format!("log: {}", log.display()).dimmed());
                        println!("  {}", "starts automatically after reboot".dimmed());
                    }
                    Err(err) => {
                        eprintln!(
                            "\n  {} {err:#}",
                            "could not install gateway service".yellow()
                        );
                        match start_bridge(&cli.relay, &config) {
                            Ok(log) => {
                                println!(
                                    "  {} {}",
                                    "●".green(),
                                    "gateway running for this login".bold()
                                );
                                println!("  {}", format!("log: {log}").dimmed());
                            }
                            Err(start_error) => {
                                eprintln!(
                                    "  {} {start_error:#}",
                                    "could not start gateway".yellow()
                                );
                                eprintln!("  {}", "start it yourself: rebeam gateway".cyan());
                            }
                        }
                    }
                }
            }
        }

        Verb::Pair { code } => {
            let res = client
                .post(format!("{}/pair", cli.relay))
                .json(&serde_json::json!({ "code": code }))
                .send()
                .await?;
            if !res.status().is_success() {
                bail!("{}", error_body(res).await);
            }
            let token = res
                .json::<serde_json::Value>()
                .await?
                .get("token")
                .and_then(|v| v.as_str())
                .context("relay did not return a machine credential")?
                .to_string();
            let root = default_config_path().parent().unwrap().to_path_buf();
            std::fs::create_dir_all(&root)?;
            std::fs::write(root.join("machine-token"), token)?;
            println!("{} machine paired", "✓".green());
        }

        Verb::Chats => {
            let chats: Vec<Chat> = client
                .get(format!("{}/chats", cli.relay))
                .send()
                .await?
                .json()
                .await?;
            for chat in chats {
                println!(
                    "{:<18} {:<6} {}",
                    chat.name.bold(),
                    format!("{:?}", chat.kind).to_lowercase().dimmed(),
                    chat.topic.unwrap_or_default().dimmed()
                );
            }
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// POST one command; the relay answers with the event it produced.
async fn send(client: &reqwest::Client, relay: &str, cmd: Cmd) -> Result<Event> {
    let res = client
        .post(format!("{relay}/commands"))
        .json(&cmd)
        .send()
        .await
        .with_context(|| format!("cannot reach the relay at {relay}"))?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        bail!("relay returned {status}: {body}");
    }
    Ok(res.json().await?)
}

/// Poll until a human answers or the clock runs out.
async fn await_answer(
    client: &reqwest::Client,
    relay: &str,
    id: &str,
    timeout_secs: u64,
) -> Result<Option<String>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let message: Message = client
            .get(format!("{relay}/messages/{id}"))
            .send()
            .await?
            .json()
            .await?;
        if let Some(choice) = message.resolved_option {
            return Ok(Some(choice));
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
}

/// Hold a live connection and print events as they happen.
async fn listen(
    client: &reqwest::Client,
    relay: &str,
    chat: Option<&str>,
    json: bool,
) -> Result<()> {
    // Resolve the chat name to an id once, so filtering is a cheap comparison.
    let filter = match chat {
        Some(needle) => {
            let chats: Vec<Chat> = client
                .get(format!("{relay}/chats"))
                .send()
                .await?
                .json()
                .await?;
            let found = chats
                .into_iter()
                .find(|c| {
                    c.id.eq_ignore_ascii_case(needle)
                        || c.name.eq_ignore_ascii_case(needle)
                        || c.name.to_lowercase().replace(' ', "")
                            == needle.to_lowercase().replace(' ', "")
                })
                .with_context(|| format!("no chat matching {needle:?}"))?;
            Some(found.id)
        }
        None => None,
    };

    let ws_url = relay.replacen("http", "ws", 1) + "/stream";
    let (socket, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .with_context(|| format!("cannot open {ws_url}"))?;
    eprintln!("{} {ws_url}", "listening".green());

    let (_, mut stream) = socket.split();
    while let Some(frame) = stream.next().await {
        let frame = frame?;
        let Ok(text) = frame.into_text() else {
            continue;
        };
        let Ok(event) = serde_json::from_str::<Event>(&text) else {
            continue;
        };
        if let Some(want) = &filter {
            if event.chat() != want {
                continue;
            }
        }
        if json {
            println!("{text}");
        } else {
            print_event(&event);
        }
    }
    Ok(())
}

fn print_event(event: &Event) {
    match event {
        Event::Message { message } => println!(
            "{} {} {}",
            message.author_id.blue().bold(),
            message.text,
            message.id.dimmed()
        ),
        Event::MessageUpdated { message } => println!(
            "{} {} → {}",
            message.author_id.blue().bold(),
            message.text,
            message.resolved_option.as_deref().unwrap_or("?").green()
        ),
        Event::ChatUpdated { chat } => println!(
            "{} {}",
            "updated".green(),
            format!("{} ({})", chat.name, chat.id).dimmed()
        ),
        Event::Status {
            author,
            state,
            label,
            tool,
            target,
            ..
        } => println!(
            "{} {} {} {}",
            author.blue().bold(),
            tool.as_deref()
                .unwrap_or(&format!("{state:?}").to_lowercase())
                .yellow(),
            target.as_deref().unwrap_or("").cyan(),
            label.as_deref().unwrap_or("").dimmed()
        ),
        Event::SessionReset { chat, member } => println!(
            "{} {} {}",
            "reset".yellow(),
            member.bold(),
            format!("in {chat}").dimmed()
        ),
    }
}

/// What `POST /connect` answers with.
#[derive(serde::Deserialize)]
struct Joined {
    member: Member,
    membership: Membership,
    chat: JoinedChat,
}

#[derive(serde::Deserialize)]
struct JoinedChat {
    name: String,
}

/// The relay's refusal, which is usually the useful half of a failed connection.
async fn error_body(res: reqwest::Response) -> String {
    let status = res.status();
    let body = res.text().await.unwrap_or_default();
    serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("error")?.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("relay returned {status}: {body}"))
}

pub(crate) fn provider_for_exec(exec: &str) -> String {
    ["claude", "codex", "hermes"]
        .into_iter()
        .find(|provider| exec.split_whitespace().any(|part| part.ends_with(provider)))
        .unwrap_or("custom")
        .to_string()
}

pub(crate) fn managed_exec(provider: &str) -> Option<&'static str> {
    match provider {
        "claude" => Some(
            r#"if [ "$REBEAM_SESSION_RESUME" = 1 ]; then claude -p --output-format json --resume "$REBEAM_SESSION" "$REBEAM_CONTEXT"; else claude -p --output-format json --session-id "$REBEAM_SESSION" "$REBEAM_CONTEXT"; fi"#,
        ),
        "codex" => Some(
            r#"if [ "$REBEAM_SESSION_RESUME" = 1 ]; then codex exec resume --json "$REBEAM_SESSION" "$REBEAM_CONTEXT"; else codex exec --json "$REBEAM_CONTEXT"; fi"#,
        ),
        "hermes" => Some(
            r#"if [ "$REBEAM_SESSION_RESUME" = 1 ]; then hermes chat -Q --source rebeam --resume "$REBEAM_SESSION" -q "$REBEAM_CONTEXT"; else hermes chat -Q --source rebeam -q "$REBEAM_CONTEXT"; fi"#,
        ),
        _ => None,
    }
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

fn hostname() -> String {
    std::process::Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

/// Append an `[[agents]]` block, replacing any block with the same name so a
/// re-connecting updates rather than duplicates.
fn default_config_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".rebeam").join("config.toml")
}

fn write_agent(path: &std::path::Path, name: &str, provider: &str, exec: &str) -> Result<()> {
    let root = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let chats = root.join("chats");
    std::fs::create_dir_all(&chats).with_context(|| format!("creating {}", chats.display()))?;
    if !path.exists() {
        std::fs::write(
            path,
            "# Rebeam machine configuration\n[chats]\ndirectory = \"chats\"\n",
        )
        .with_context(|| format!("writing {}", path.display()))?;
    }
    let safe_name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let profile = chats.join(format!("{safe_name}.toml"));
    std::fs::write(
        &profile,
        format!("name = {name:?}\nprovider = {provider:?}\nexec = {exec:?}\n"),
    )
    .with_context(|| format!("writing {}", profile.display()))?;
    Ok(())
}

const SERVICE_LABEL: &str = "app.rebeam.gateway";

/// Install a per-user service. No root access: launchd owns it on macOS and
/// the user's systemd instance owns it on Linux.
fn install_bridge_service(relay: &str, config: &Path) -> Result<PathBuf> {
    let exe = std::env::current_exe().context("locating the rebeam binary")?;
    let config = std::fs::canonicalize(config).unwrap_or_else(|_| config.to_path_buf());
    let root = config.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(root)?;
    let log = root.join("gateway.log");

    match std::env::consts::OS {
        "macos" => install_launch_agent(&exe, relay, &config, &log)?,
        "linux" => install_systemd_user_service(&exe, relay, &config, &log)?,
        other => anyhow::bail!("automatic gateway startup is not supported on {other}"),
    }
    Ok(log)
}

#[cfg(target_os = "macos")]
fn launch_domain() -> Result<String> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .context("finding the current user id")?;
    if !output.status.success() {
        anyhow::bail!("could not determine the current user id");
    }
    Ok(format!(
        "gui/{}",
        String::from_utf8_lossy(&output.stdout).trim()
    ))
}

#[cfg(target_os = "macos")]
fn install_launch_agent(exe: &Path, relay: &str, config: &Path, log: &Path) -> Result<()> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    let agents = PathBuf::from(&home).join("Library/LaunchAgents");
    std::fs::create_dir_all(&agents)?;
    let plist = agents.join(format!("{SERVICE_LABEL}.plist"));
    let domain = launch_domain()?;

    let _ = std::process::Command::new("launchctl")
        .args(["bootout", &format!("{domain}/{SERVICE_LABEL}")])
        .output();
    stop_gateway_processes();

    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{exe}</string>
    <string>--relay</string><string>{relay}</string>
    <string>gateway</string>
    <string>--config</string><string>{config}</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
  <key>ThrottleInterval</key><integer>3</integer>
  <key>ProcessType</key><string>Background</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>HOME</key><string>{home}</string>
    <key>PATH</key><string>{path}</string>
  </dict>
  <key>StandardOutPath</key><string>{log}</string>
  <key>StandardErrorPath</key><string>{log}</string>
</dict>
</plist>
"#,
        label = SERVICE_LABEL,
        exe = xml_escape(&exe.display().to_string()),
        relay = xml_escape(relay),
        config = xml_escape(&config.display().to_string()),
        log = xml_escape(&log.display().to_string()),
        home = xml_escape(&home),
        path = xml_escape(&path),
    );
    std::fs::write(&plist, document).with_context(|| format!("writing {}", plist.display()))?;

    command_ok(
        std::process::Command::new("launchctl")
            .args(["bootstrap", &domain])
            .arg(&plist),
        "loading the Rebeam launch agent",
    )?;
    let _ = std::process::Command::new("launchctl")
        .args(["enable", &format!("{domain}/{SERVICE_LABEL}")])
        .status();
    command_ok(
        std::process::Command::new("launchctl").args([
            "kickstart",
            "-k",
            &format!("{domain}/{SERVICE_LABEL}"),
        ]),
        "starting the Rebeam launch agent",
    )
}

#[cfg(not(target_os = "macos"))]
fn install_launch_agent(_: &Path, _: &str, _: &Path, _: &Path) -> Result<()> {
    anyhow::bail!("launchd is unavailable")
}

#[cfg(target_os = "linux")]
fn install_systemd_user_service(exe: &Path, relay: &str, config: &Path, log: &Path) -> Result<()> {
    let home = std::env::var("HOME").context("HOME is not set")?;
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    let units = PathBuf::from(&home).join(".config/systemd/user");
    std::fs::create_dir_all(&units)?;
    let unit = units.join("rebeam-gateway.service");
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "stop", "rebeam-gateway.service"])
        .status();
    stop_gateway_processes();
    let document = format!(
        "[Unit]\nDescription=Rebeam agent gateway\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nEnvironment={}\nEnvironment={}\nExecStart={} --relay {} gateway --config {}\nRestart=always\nRestartSec=3\nStandardOutput=append:{}\nStandardError=append:{}\n\n[Install]\nWantedBy=default.target\n",
        systemd_value(&format!("HOME={home}")),
        systemd_value(&format!("PATH={path}")),
        systemd_quote(exe),
        systemd_quote(Path::new(relay)),
        systemd_quote(config),
        systemd_quote(log),
        systemd_quote(log),
    );
    std::fs::write(&unit, document).with_context(|| format!("writing {}", unit.display()))?;
    if let Ok(user) = std::env::var("USER") {
        // Best effort: when permitted, linger keeps the user service alive on
        // a headless VPS after logout and starts it during boot.
        let _ = std::process::Command::new("loginctl")
            .args(["enable-linger", &user])
            .status();
    }
    command_ok(
        std::process::Command::new("systemctl").args(["--user", "daemon-reload"]),
        "reloading user services",
    )?;
    command_ok(
        std::process::Command::new("systemctl").args([
            "--user",
            "enable",
            "--now",
            "rebeam-gateway.service",
        ]),
        "enabling the Rebeam user service",
    )
}

#[cfg(not(target_os = "linux"))]
fn install_systemd_user_service(_: &Path, _: &str, _: &Path, _: &Path) -> Result<()> {
    anyhow::bail!("systemd is unavailable")
}

fn uninstall_bridge_service() -> Result<()> {
    match std::env::consts::OS {
        "macos" => {
            #[cfg(target_os = "macos")]
            {
                let domain = launch_domain()?;
                let _ = std::process::Command::new("launchctl")
                    .args(["bootout", &format!("{domain}/{SERVICE_LABEL}")])
                    .status();
                let home = std::env::var("HOME").context("HOME is not set")?;
                let plist = PathBuf::from(home)
                    .join("Library/LaunchAgents")
                    .join(format!("{SERVICE_LABEL}.plist"));
                match std::fs::remove_file(&plist) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
            }
        }
        "linux" => {
            #[cfg(target_os = "linux")]
            {
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "disable", "--now", "rebeam-gateway.service"])
                    .status();
                let home = std::env::var("HOME").context("HOME is not set")?;
                let unit = PathBuf::from(home).join(".config/systemd/user/rebeam-gateway.service");
                match std::fs::remove_file(&unit) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error.into()),
                }
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "daemon-reload"])
                    .status();
            }
        }
        _ => {}
    }
    stop_gateway_processes();
    Ok(())
}

fn bridge_service_running() -> bool {
    let managed = match std::env::consts::OS {
        "macos" => {
            #[cfg(target_os = "macos")]
            {
                launch_domain()
                    .ok()
                    .and_then(|domain| {
                        std::process::Command::new("launchctl")
                            .args(["print", &format!("{domain}/{SERVICE_LABEL}")])
                            .output()
                            .ok()
                    })
                    .is_some_and(|output| output.status.success())
            }
            #[cfg(not(target_os = "macos"))]
            false
        }
        "linux" => {
            #[cfg(target_os = "linux")]
            {
                std::process::Command::new("systemctl")
                    .args(["--user", "is-active", "--quiet", "rebeam-gateway.service"])
                    .status()
                    .is_ok_and(|status| status.success())
            }
            #[cfg(not(target_os = "linux"))]
            false
        }
        _ => false,
    };
    managed || bridge_running()
}

fn stop_gateway_processes() {
    let _ = std::process::Command::new("pkill")
        .args(["-f", "rebeam.*gateway"])
        .status();
}

fn command_ok(command: &mut std::process::Command, action: &str) -> Result<()> {
    let output = command.output().with_context(|| action.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("{action}: {}", stderr.trim())
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(target_os = "linux")]
fn systemd_quote(value: &Path) -> String {
    format!(
        "\"{}\"",
        value
            .display()
            .to_string()
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

#[cfg(target_os = "linux")]
fn systemd_value(value: &str) -> String {
    format!(
        "\"{}\"",
        value
            .replace('%', "%%")
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

/// Start `rebeam gateway` detached, and return where its output went.
///
/// Not a service yet — this still dies with the machine, and installing a
/// launchd/systemd unit is the real fix. But it removes the step that everyone,
/// human and agent alike, forgets.
fn start_bridge(relay: &str, config: &std::path::Path) -> Result<String> {
    use std::process::{Command, Stdio};

    if bridge_running() {
        return Ok("already running".into());
    }

    let log_path = std::env::temp_dir().join("rebeam-gateway.log");
    let log = std::fs::File::create(&log_path)
        .with_context(|| format!("creating {}", log_path.display()))?;
    let errlog = log.try_clone()?;

    let exe = std::env::current_exe().context("locating the rebeam binary")?;
    Command::new(exe)
        .arg("--relay")
        .arg(relay)
        .arg("gateway")
        .arg("--config")
        // Absolute, because the detached process does not keep our cwd in any
        // way we should rely on.
        .arg(std::fs::canonicalize(config).unwrap_or_else(|_| config.to_path_buf()))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errlog))
        .spawn()
        .context("spawning the bridge")?;

    Ok(log_path.display().to_string())
}

fn bridge_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", "rebeam.*\\bgateway\\b"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn read_stdin() -> Result<String> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("reading stdin")?;
    Ok(buf.trim_end().to_string())
}

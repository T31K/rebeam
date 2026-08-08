//! `claude-code-acp` — rebeam's own ACP adapter for the Claude Code CLI.
//!
//! rebeam ships this so `--provider claude` needs **no Node** and uses the
//! user's **Claude Code subscription login**: an ACP client (the rebeam gateway)
//! spawns this over stdio, and it drives `claude -p --output-format stream-json`,
//! translating Claude's JSONL into ACP session updates and permission requests.
//!
//! Subscription auth: we strip `ANTHROPIC_API_KEY` / `ANTHROPIC_AUTH_TOKEN` from
//! the child's env so `claude` authenticates via `claude login` (the Agent SDK
//! path refuses subscription auth — that's why we wrap the CLI, not the SDK).
//!
//! Design ported from harukitosa/claude-code-acp (MIT). Streaming + tool
//! telemetry + resume are solid; the permission round-trip depends on Claude
//! emitting a `permission_request` event in `-p` mode and is best-effort until
//! validated against a live Claude (see the `permission_request` arm).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use agent_client_protocol::schema::v1::{
    AgentCapabilities, ContentBlock, ContentChunk, InitializeRequest, InitializeResponse,
    NewSessionRequest, NewSessionResponse, PermissionOption, PermissionOptionKind, PromptRequest,
    PromptResponse, SessionId, SessionNotification, SessionUpdate, StopReason, TextContent,
    ToolCall, ToolCallId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Agent, Result, Stdio as AcpStdio};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::Mutex;

/// ACP session id → what we need to drive and resume the matching Claude run.
#[derive(Default, Clone)]
struct SessionState {
    cwd: Option<PathBuf>,
    claude_session_id: Option<String>,
}

type Sessions = Arc<Mutex<HashMap<String, SessionState>>>;

#[tokio::main]
async fn main() -> Result<()> {
    let sessions: Sessions = Arc::new(Mutex::new(HashMap::new()));
    let for_new = sessions.clone();
    let for_prompt = sessions.clone();

    Agent
        .builder()
        .name("claude-code-acp")
        .on_receive_request(
            async |req: InitializeRequest, responder, _connection| {
                responder.respond(
                    InitializeResponse::new(req.protocol_version)
                        .agent_capabilities(AgentCapabilities::new()),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: NewSessionRequest, responder, _connection| {
                let id = format!("sess-{}", uuid::Uuid::new_v4().simple());
                for_new.lock().await.insert(
                    id.clone(),
                    SessionState {
                        cwd: Some(req.cwd.clone()),
                        claude_session_id: None,
                    },
                );
                responder.respond(NewSessionResponse::new(SessionId::new(id)))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: PromptRequest, responder, connection| {
                let key = req.session_id.0.to_string();
                let (cwd, claude_session_id) = {
                    let map = for_prompt.lock().await;
                    let state = map.get(&key).cloned().unwrap_or_default();
                    (state.cwd, state.claude_session_id)
                };

                // Flatten the prompt's text blocks into the argument Claude reads.
                let text = req
                    .prompt
                    .iter()
                    .filter_map(|block| match block {
                        ContentBlock::Text(t) => Some(t.text.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");

                let mut args = vec![
                    "-p".to_string(),
                    text,
                    "--output-format".into(),
                    "stream-json".into(),
                    "--verbose".into(),
                ];
                if let Some(sid) = &claude_session_id {
                    args.push("--resume".into());
                    args.push(sid.clone());
                }

                let mut command = Command::new("claude");
                command
                    .args(&args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    // Force subscription auth — no API-key fallback.
                    .env_remove("ANTHROPIC_API_KEY")
                    .env_remove("ANTHROPIC_AUTH_TOKEN");
                if let Some(cwd) = &cwd {
                    command.current_dir(cwd);
                }

                let mut child = match command.spawn() {
                    Ok(child) => child,
                    Err(err) => {
                        connection.send_notification(SessionNotification::new(
                            req.session_id.clone(),
                            SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                                TextContent::new(format!("failed to launch `claude`: {err}")),
                            ))),
                        ))?;
                        return responder.respond(PromptResponse::new(StopReason::EndTurn));
                    }
                };

                let stdout = child.stdout.take().expect("stdout piped");
                let mut lines = BufReader::new(stdout).lines();
                let mut tool_counter: u32 = 0;
                let mut new_claude_session: Option<String> = None;

                while let Ok(Some(line)) = lines.next_line().await {
                    let Ok(value) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if let Some(sid) = value.get("session_id").and_then(Value::as_str) {
                        new_claude_session = Some(sid.to_string());
                    }
                    match value.get("type").and_then(Value::as_str) {
                        Some("assistant") => {
                            if let Some(content) = value
                                .get("message")
                                .and_then(|m| m.get("content"))
                                .and_then(Value::as_array)
                            {
                                for block in content {
                                    match block.get("type").and_then(Value::as_str) {
                                        Some("text") => {
                                            if let Some(t) =
                                                block.get("text").and_then(Value::as_str)
                                            {
                                                connection.send_notification(
                                                    SessionNotification::new(
                                                        req.session_id.clone(),
                                                        SessionUpdate::AgentMessageChunk(
                                                            ContentChunk::new(ContentBlock::Text(
                                                                TextContent::new(t),
                                                            )),
                                                        ),
                                                    ),
                                                )?;
                                            }
                                        }
                                        Some("tool_use") => {
                                            tool_counter += 1;
                                            let name = block
                                                .get("name")
                                                .and_then(Value::as_str)
                                                .unwrap_or("tool");
                                            // A short preview from whatever the
                                            // tool acted on, for the chip title.
                                            let preview = block
                                                .get("input")
                                                .and_then(|input| {
                                                    ["command", "file_path", "path", "pattern", "url"]
                                                        .iter()
                                                        .find_map(|k| {
                                                            input.get(k).and_then(Value::as_str)
                                                        })
                                                })
                                                .unwrap_or("");
                                            let title = if preview.is_empty() {
                                                name.to_string()
                                            } else {
                                                format!("{name}: {preview}")
                                            };
                                            connection.send_notification(
                                                SessionNotification::new(
                                                    req.session_id.clone(),
                                                    SessionUpdate::ToolCall(ToolCall::new(
                                                        ToolCallId::new(format!(
                                                            "call_{tool_counter}"
                                                        )),
                                                        title,
                                                    )),
                                                ),
                                            )?;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                        // Best-effort: if Claude surfaces a permission request, route
                        // it to the ACP client (→ rebeam's Ask card). Enforcement of
                        // the answer back into Claude needs the --permission-prompt-tool
                        // MCP path; tracked as the next step.
                        Some("permission_request") => {
                            tool_counter += 1;
                            let name = value
                                .get("tool_name")
                                .or_else(|| value.get("toolName"))
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_string();
                            let _outcome = connection
                                .send_request(agent_client_protocol::schema::v1::RequestPermissionRequest::new(
                                    req.session_id.clone(),
                                    ToolCallUpdate::new(
                                        ToolCallId::new(format!("call_{tool_counter}")),
                                        ToolCallUpdateFields::new()
                                            .title(name)
                                            .kind(ToolKind::Execute)
                                            .status(ToolCallStatus::Pending),
                                    ),
                                    vec![
                                        PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
                                        PermissionOption::new("allow_always", "Allow always", PermissionOptionKind::AllowAlways),
                                        PermissionOption::new("reject_once", "Reject once", PermissionOptionKind::RejectOnce),
                                        PermissionOption::new("reject_always", "Reject always", PermissionOptionKind::RejectAlways),
                                    ],
                                ))
                                .block_task()
                                .await?;
                        }
                        Some("result") => break,
                        _ => {}
                    }
                }

                if let Some(sid) = new_claude_session {
                    for_prompt
                        .lock()
                        .await
                        .entry(key)
                        .or_default()
                        .claude_session_id = Some(sid);
                }

                responder.respond(PromptResponse::new(StopReason::EndTurn))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_to(AcpStdio::new())
        .await
}

//! The rebeam relay.
//!
//! Commands come in over HTTP, events go out over one WebSocket. Durable
//! events are appended to SQLite first; ephemeral ones (agent telemetry) are
//! broadcast and forgotten.

mod store;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{header, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use rebeam_core::{Command, Event, HistoryGrant, Message, MessageKind, Trigger};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::broadcast;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::store::{Store, User};

const DEFAULT_PORT: u16 = 8787;

/// Where the log lives. A relative default would make the database depend on
/// the directory you happened to launch from — two shells, two histories, and
/// no hint that anything is wrong. `REBEAM_DB` overrides for tests.
fn db_path() -> String {
    if let Ok(path) = std::env::var("REBEAM_DB") {
        return path;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let dir = format!("{home}/.rebeam");
    let _ = std::fs::create_dir_all(&dir);
    format!("{dir}/rebeam.db")
}

#[derive(Clone)]
struct App {
    store: Arc<Store>,
    events: broadcast::Sender<Event>,
}

#[tokio::main]
async fn main() {
    let db = db_path();
    let store = Store::open(&db).expect("open database");
    let (events, _) = broadcast::channel(1024);
    let app_state = App {
        store: Arc::new(store),
        events,
    };

    let app = router(app_state);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(DEFAULT_PORT);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port))
        .await
        .expect("bind");

    println!("rebeam relay  ·  http://127.0.0.1:{port}  ·  {db}");
    axum::serve(listener, app).await.unwrap();
}

fn router(app_state: App) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/auth/me", get(me))
        .route("/auth/logout", post(logout))
        .route("/bootstrap", post(bootstrap))
        .route("/pairings", post(create_pairing))
        .route("/pair", post(pair))
        .route("/machines", get(machines))
        .route("/machines/heartbeat", post(heartbeat))
        .route("/chats", get(chats).post(create_chat))
        .route("/chats/{id}", patch(update_chat))
        .route("/members", get(members))
        .route("/chats/{id}/messages", get(messages))
        .route("/commands", post(command))
        .route("/messages/{id}", get(message))
        .route("/invites", post(create_invite))
        .route("/connect", post(connect))
        .route("/members/{id}/memberships", get(memberships))
        .route("/chats/{id}/unread", get(unread))
        .route("/chats/{id}/read", post(read))
        .route(
            "/chats/{id}/members/{member}",
            post(set_trigger).delete(kick),
        )
        .route(
            "/chats/{id}/members/{member}/reset-session",
            post(reset_session),
        )
        .route("/stream", get(stream))
        .layer(cors_layer())
        .with_state(app_state)
}

fn cors_layer() -> CorsLayer {
    let mut origins = vec![
        HeaderValue::from_static("http://localhost:1420"),
        HeaderValue::from_static("http://127.0.0.1:1420"),
        HeaderValue::from_static("tauri://localhost"),
        HeaderValue::from_static("http://tauri.localhost"),
        HeaderValue::from_static("https://tauri.localhost"),
    ];
    if let Ok(extra) = std::env::var("REBEAM_ALLOWED_ORIGINS") {
        origins.extend(
            extra
                .split(',')
                .filter_map(|origin| origin.trim().parse::<HeaderValue>().ok()),
        );
    }

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
}

async fn health() -> impl IntoResponse {
    Json(json!({ "ok": true, "service": "rebeam-relay" }))
}

async fn bootstrap(State(app): State<App>) -> Result<impl IntoResponse, Fail> {
    let session = app.store.bootstrap_workspace()?;
    Ok(Json(
        json!({ "token": session.token, "user": session.user }),
    ))
}

async fn create_pairing(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let workspace = user_authenticated(&app, &headers)?;
    Ok(Json(app.store.create_pairing(&workspace.id)?))
}

#[derive(Deserialize)]
struct Pair {
    code: String,
}

async fn pair(State(app): State<App>, Json(body): Json<Pair>) -> Result<impl IntoResponse, Fail> {
    match app.store.redeem_pairing(&body.code)? {
        Ok(token) => Ok(Json(json!({ "token": token }))),
        Err(error) => Err(Fail::bad_request(error)),
    }
}

async fn machines(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let workspace = user_authenticated(&app, &headers)?;
    let (count, online) = app.store.machine_status(&workspace.id)?;
    Ok(Json(json!({ "count": count, "online": online })))
}

async fn heartbeat(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    if !app.store.touch_machine(bearer_token(&headers)?)? {
        return Err(Fail::unauthorized("machine credential is invalid".into()));
    }
    Ok(Json(json!({ "ok": true })))
}

async fn me(State(app): State<App>, headers: axum::http::HeaderMap) -> Result<Json<User>, Fail> {
    let token = bearer_token(&headers)?;
    app.store
        .user_for_token(token)?
        .map(Json)
        .ok_or_else(|| Fail::unauthorized("your session has expired".into()))
}

async fn logout(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let token = bearer_token(&headers)?;
    user_authenticated(&app, &headers)?;
    app.store.revoke_session(token)?;
    Ok(Json(json!({ "ok": true })))
}

fn bearer_token(headers: &axum::http::HeaderMap) -> Result<&str, Fail> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| !token.is_empty())
        .ok_or_else(|| Fail::unauthorized("sign in to continue".into()))
}

fn authenticated(app: &App, headers: &axum::http::HeaderMap) -> Result<User, Fail> {
    let token = bearer_token(headers)?;
    app.store
        .user_for_token(token)?
        .or(app.store.machine_for_token(token)?)
        .ok_or_else(|| Fail::unauthorized("your session has expired".into()))
}

fn user_authenticated(app: &App, headers: &axum::http::HeaderMap) -> Result<User, Fail> {
    let token = bearer_token(headers)?;
    app.store
        .user_for_token(token)?
        .ok_or_else(|| Fail::unauthorized("a device credential is required".into()))
}

fn machine_authenticated(app: &App, headers: &axum::http::HeaderMap) -> Result<User, Fail> {
    let token = bearer_token(headers)?;
    app.store
        .machine_for_token(token)?
        .ok_or_else(|| Fail::unauthorized("a paired machine credential is required".into()))
}

async fn chats(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = authenticated(&app, &headers)?;
    Ok(Json(app.store.chats_for_member(&user.id)?))
}

async fn members(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = authenticated(&app, &headers)?;
    Ok(Json(app.store.members_for_member(&user.id)?))
}

async fn messages(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &id, &user.id)?;
    Ok(Json(app.store.messages(&chat)?))
}

async fn message(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<Json<Message>, Fail> {
    let viewer = authenticated(&app, &headers)?;
    let message = app
        .store
        .message(&id)?
        .ok_or_else(|| Fail::not_found(format!("no message {id:?}")))?;
    if !app
        .store
        .is_member_of_chat(&message.channel_id, &viewer.id)?
    {
        return Err(Fail::not_found(format!("no message {id:?}")));
    }
    Ok(Json(message))
}

// ---------------------------------------------------------------------------
// Membership
//
// An agent joins a chat exactly like a person does. The invite does not know
// what will redeem it, so both paths land in the same table and emit the same
// system message.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewChat {
    name: String,
    #[serde(default)]
    topic: Option<String>,
}

async fn create_chat(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Json(body): Json<NewChat>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    if body.name.trim().is_empty() {
        return Err(Fail::bad_request("a chat needs a name".into()));
    }
    let chat = app.store.create_chat(
        body.name.trim(),
        body.topic.as_deref(),
        &user.id,
        now_millis(),
    )?;
    Ok(Json(chat))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatPatch {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    topic: Option<String>,
    #[serde(default)]
    avatar_seed: Option<String>,
}

async fn update_chat(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
    Json(body): Json<ChatPatch>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat_id = resolve_chat_for(&app, &id, &user.id)?;

    let name = body.name.as_deref().map(str::trim);
    if name.is_some_and(str::is_empty) {
        return Err(Fail::bad_request("a chat needs a name".into()));
    }
    if name.is_some_and(|value| value.len() > 80) {
        return Err(Fail::bad_request(
            "chat names are limited to 80 characters".into(),
        ));
    }
    let topic = body.topic.as_deref().map(str::trim);
    if topic.is_some_and(|value| value.len() > 240) {
        return Err(Fail::bad_request(
            "chat topics are limited to 240 characters".into(),
        ));
    }
    let avatar_seed = body.avatar_seed.as_deref().map(str::trim);
    if avatar_seed.is_some_and(|value| value.len() > 128) {
        return Err(Fail::bad_request(
            "avatar seeds are limited to 128 characters".into(),
        ));
    }
    if name.is_none() && topic.is_none() && avatar_seed.is_none() {
        return Err(Fail::bad_request("nothing to update".into()));
    }

    let chat = app
        .store
        .update_chat(&chat_id, name, topic, avatar_seed)?
        .ok_or_else(|| Fail::not_found("no chat matching that id".into()))?;
    let _ = app.events.send(Event::ChatUpdated { chat: chat.clone() });
    Ok(Json(chat))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewInvite {
    chat: String,
    /// Defaults to the whole backlog, the way adding someone to a Slack
    /// channel does.
    #[serde(default)]
    history: HistoryGrant,
}

async fn create_invite(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Json(body): Json<NewInvite>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &body.chat, &user.id)?;
    match app
        .store
        .create_invite(&chat, &user.id, body.history, now_millis())?
    {
        Ok(invite) => Ok(Json(invite)),
        Err(why) => Err(Fail::bad_request(why)),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Join {
    code: String,
    /// What the agent will be called in the member list.
    name: String,
    /// The machine redeeming, so a code used by the wrong box is visible.
    #[serde(default)]
    host: Option<String>,
}

async fn connect(
    State(app): State<App>,
    Json(body): Json<Join>,
) -> Result<impl IntoResponse, Fail> {
    let owner = app
        .store
        .invite_inviter(&body.code)?
        .ok_or_else(|| Fail::bad_request("no such invite".into()))?;

    let member = app
        .store
        .upsert_agent(&body.name, &owner, body.host.as_deref())?;

    let membership = match app.store.redeem(&body.code, &member.id, now_millis())? {
        Ok(m) => m,
        Err(why) => return Err(Fail::bad_request(why)),
    };

    let chat = app
        .store
        .chats()?
        .into_iter()
        .find(|c| c.id == membership.chat)
        .ok_or_else(|| Fail::not_found("chat vanished mid-connect".into()))?;

    // Joining is a message. The log stays the truth, and the host that
    // redeemed the code is on the record.
    let line = match &member.host {
        Some(host) => format!("added **{}** · {host}", member.name),
        None => format!("added **{}**", member.name),
    };
    let system = app.store.append(new_message(
        &app,
        &membership.chat,
        &owner,
        MessageKind::System,
        line,
        None,
    )?)?;
    let _ = app.events.send(Event::Message { message: system });

    Ok(Json(json!({
        "member": member,
        "membership": membership,
        "chat": { "id": chat.id, "name": chat.name, "kind": chat.kind },
    })))
}

/// Every chat this member belongs to, with its standing in each. The bridge's
/// whole picture of where an agent may speak.
async fn memberships(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let viewer = authenticated(&app, &headers)?;
    let member = resolve_member(&app, &id)?;
    let visible = app
        .store
        .members_for_member(&viewer.id)?
        .into_iter()
        .any(|candidate| candidate.id == member);
    if !visible {
        return Err(Fail::not_found("no such member".into()));
    }
    let memberships = app
        .store
        .memberships_of(&member)?
        .into_iter()
        .filter(|membership| {
            app.store
                .is_member_of_chat(&membership.chat, &viewer.id)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    Ok(Json(memberships))
}

/// Everything this member may see and has not seen. The window the bridge
/// hands over on the next turn — clamped to the floor, capped, oldest first.
#[derive(Deserialize)]
struct As {
    #[serde(rename = "as")]
    member: String,
    #[serde(default = "default_window")]
    limit: usize,
}

fn default_window() -> usize {
    50
}

async fn unread(
    State(app): State<App>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<As>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let workspace = machine_authenticated(&app, &headers)?;
    let member = resolve_member(&app, &q.member)?;
    let chat = resolve_chat_for(&app, &id, &member)?;
    if !app.store.agent_belongs_to(&member, &workspace.id)?
        || !app.store.is_member_of_chat(&chat, &member)?
    {
        return Err(Fail::not_found("no matching agent membership".into()));
    }
    Ok(Json(app.store.unread(&chat, &member, q.limit)?))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Read {
    #[serde(rename = "as")]
    member: String,
    /// Omit to mark the whole chat read.
    #[serde(default)]
    seq: Option<i64>,
}

async fn read(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: axum::http::HeaderMap,
    Json(body): Json<Read>,
) -> Result<impl IntoResponse, Fail> {
    let workspace = machine_authenticated(&app, &headers)?;
    let member = resolve_member(&app, &body.member)?;
    let chat = resolve_chat_for(&app, &id, &member)?;
    if !app.store.agent_belongs_to(&member, &workspace.id)?
        || !app.store.is_member_of_chat(&chat, &member)?
    {
        return Err(Fail::not_found("no matching agent membership".into()));
    }
    let seq = match body.seq {
        Some(seq) => seq,
        None => app.store.head_seq(&chat)?,
    };
    app.store.advance_cursor(&chat, &member, seq)?;
    Ok(Json(
        json!({ "chat": chat, "member": member, "cursorSeq": seq }),
    ))
}

#[derive(Deserialize)]
struct SetTrigger {
    trigger: Trigger,
}

async fn set_trigger(
    State(app): State<App>,
    Path((id, member)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
    Json(body): Json<SetTrigger>,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &id, &user.id)?;
    let member = resolve_member(&app, &member)?;
    if !app.store.is_member_of_chat(&chat, &member)? {
        return Err(Fail::not_found("that agent is not in this chat".into()));
    }
    let target = app
        .store
        .members_for_member(&user.id)?
        .into_iter()
        .find(|candidate| candidate.id == member)
        .ok_or_else(|| Fail::not_found("no such agent".into()))?;
    if target.kind != rebeam_core::MemberKind::Agent {
        return Err(Fail::bad_request("only agents have wake triggers".into()));
    }
    app.store.set_trigger(&chat, &member, body.trigger)?;
    Ok(Json(json!({ "ok": true })))
}

/// Removing a member is the only revocation there is, and it is the same
/// gesture for humans and agents.
async fn kick(
    State(app): State<App>,
    Path((id, member)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &id, &user.id)?;
    let member_id = resolve_member(&app, &member)?;
    if !app.store.kick(&chat, &member_id)? {
        return Err(Fail::not_found(format!("{member:?} is not in that chat")));
    }

    let _ = app.events.send(Event::SessionReset {
        chat: chat.clone(),
        member: member_id.clone(),
    });

    let name = app
        .store
        .members()?
        .into_iter()
        .find(|m| m.id == member_id)
        .map(|m| m.name)
        .unwrap_or(member_id);
    let system = app.store.append(new_message(
        &app,
        &chat,
        &user.id,
        MessageKind::System,
        format!("removed **{name}**"),
        None,
    )?)?;
    let _ = app.events.send(Event::Message {
        message: system.clone(),
    });
    Ok(Json(system))
}

async fn reset_session(
    State(app): State<App>,
    Path((id, member)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, Fail> {
    let user = user_authenticated(&app, &headers)?;
    let chat = resolve_chat_for(&app, &id, &user.id)?;
    let member = resolve_member(&app, &member)?;
    if !app.store.is_member_of_chat(&chat, &member)? {
        return Err(Fail::not_found("that agent is not in this chat".into()));
    }
    let target = app
        .store
        .members()?
        .into_iter()
        .find(|candidate| candidate.id == member)
        .ok_or_else(|| Fail::not_found("no such agent".into()))?;
    if target.kind != rebeam_core::MemberKind::Agent {
        return Err(Fail::bad_request("only agent sessions can be reset".into()));
    }

    app.store.reset_cursor(&chat, &member)?;
    let _ = app.events.send(Event::SessionReset {
        chat: chat.clone(),
        member: member.clone(),
    });
    Ok(Json(json!({ "ok": true, "chat": chat, "member": member })))
}

/// The single write path. Every client — app, CLI, future bridge — posts here.
async fn command(
    State(app): State<App>,
    headers: axum::http::HeaderMap,
    Json(mut cmd): Json<Command>,
) -> Result<impl IntoResponse, Fail> {
    let token = bearer_token(&headers)?;
    let machine = app.store.machine_for_token(token)?;
    let user = authenticated(&app, &headers)?;
    if let Some(workspace) = machine {
        match &mut cmd {
            Command::Send { chat, author, .. } | Command::Status { chat, author, .. } => {
                let author_id = resolve_member(&app, author)?;
                if !app.store.agent_belongs_to(&author_id, &workspace.id)? {
                    return Err(Fail::unauthorized(
                        "agent is not paired to this workspace".into(),
                    ));
                }
                let chat_id = resolve_chat_for(&app, chat, &author_id)?;
                *chat = chat_id;
                *author = author_id;
            }
            _ => {
                return Err(Fail::unauthorized(
                    "machine credentials may only act as their paired agent".into(),
                ))
            }
        }
        let event = apply(&app, cmd)?;
        let _ = app.events.send(event.clone());
        return Ok(Json(event));
    }
    match &mut cmd {
        Command::Send { chat, author, .. } | Command::Ask { chat, author, .. } => {
            let chat_id = resolve_chat_for(&app, chat, &user.id)?;
            *chat = chat_id;
            *author = user.id;
        }
        Command::Resolve { message, by, .. } => {
            let target = app
                .store
                .message(message)?
                .ok_or_else(|| Fail::not_found(format!("no message {message:?}")))?;
            if !app.store.is_member_of_chat(&target.channel_id, &user.id)? {
                return Err(Fail::not_found(format!("no message {message:?}")));
            }
            *by = user.id;
        }
        Command::Status { .. } => {
            return Err(Fail::bad_request(
                "agent status is not available for user sessions".into(),
            ))
        }
    }
    let event = apply(&app, cmd)?;
    // A receiver-less broadcast is not an error: nobody is listening yet.
    let _ = app.events.send(event.clone());
    Ok(Json(event))
}

fn apply(app: &App, cmd: Command) -> Result<Event, Fail> {
    match cmd {
        Command::Send {
            chat,
            author,
            text,
            idem: _,
        } => {
            let message = app.store.append(new_message(
                app,
                &chat,
                &author,
                MessageKind::Text,
                text,
                None,
            )?)?;
            Ok(Event::Message { message })
        }

        Command::Ask {
            chat,
            author,
            text,
            options,
        } => {
            let message = app.store.append(new_message(
                app,
                &chat,
                &author,
                MessageKind::Ask,
                text,
                Some(options),
            )?)?;
            Ok(Event::Message { message })
        }

        Command::Resolve {
            message,
            option,
            by: _,
        } => {
            let message = app
                .store
                .resolve(&message, &option)?
                .ok_or_else(|| Fail::not_found(format!("no message {message:?}")))?;
            Ok(Event::MessageUpdated { message })
        }

        // Ephemeral: broadcast, never stored.
        Command::Status {
            chat,
            author,
            state,
            label,
            tool,
            target,
        } => {
            let chat = resolve_chat(app, &chat)?;
            let author = resolve_member(app, &author)?;
            Ok(Event::Status {
                chat,
                author,
                state,
                label,
                tool,
                target,
            })
        }
    }
}

fn new_message(
    app: &App,
    chat: &str,
    author: &str,
    kind: MessageKind,
    text: String,
    options: Option<Vec<String>>,
) -> Result<Message, Fail> {
    Ok(Message {
        id: format!("m_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]),
        channel_id: resolve_chat(app, chat)?,
        author_id: resolve_member(app, author)?,
        seq: 0, // assigned by the store, under the same lock as the insert
        kind,
        text,
        options,
        resolved_option: None,
        created_at: now_millis(),
    })
}

fn resolve_chat(app: &App, needle: &str) -> Result<String, Fail> {
    app.store
        .find_chat(needle)?
        .ok_or_else(|| Fail::not_found(format!("no chat matching {needle:?}")))
}

fn resolve_chat_for(app: &App, needle: &str, member: &str) -> Result<String, Fail> {
    app.store
        .find_chat_for_member(needle, member)?
        .ok_or_else(|| Fail::not_found(format!("no chat matching {needle:?}")))
}

fn resolve_member(app: &App, needle: &str) -> Result<String, Fail> {
    app.store
        .find_member(needle)?
        .ok_or_else(|| Fail::not_found(format!("no member matching {needle:?}")))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

// ---------------------------------------------------------------------------
// The stream
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamQuery {
    token: String,
}

async fn stream(
    ws: WebSocketUpgrade,
    State(app): State<App>,
    axum::extract::Query(query): axum::extract::Query<StreamQuery>,
) -> Result<Response, Fail> {
    let user = app
        .store
        .user_for_token(&query.token)?
        .or(app.store.machine_for_token(&query.token)?)
        .ok_or_else(|| Fail::unauthorized("your session has expired".into()))?;
    Ok(ws.on_upgrade(move |socket| pump(socket, app, user)))
}

/// One task per connected client: forward every event for as long as it lives.
async fn pump(socket: WebSocket, app: App, user: User) {
    let (mut tx, mut rx) = socket.split();
    let mut events = app.events.subscribe();

    let send = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => {
                    let chat = match &event {
                        Event::Message { message } | Event::MessageUpdated { message } => {
                            &message.channel_id
                        }
                        Event::ChatUpdated { chat } => &chat.id,
                        Event::Status { chat, .. } => chat,
                        Event::SessionReset { chat, .. } => chat,
                    };
                    if !app.store.is_member_of_chat(chat, &user.id).unwrap_or(false) {
                        continue;
                    }
                    let Ok(text) = serde_json::to_string(&event) else {
                        continue;
                    };
                    if tx.send(WsMessage::Text(text.into())).await.is_err() {
                        break;
                    }
                }
                // A lagging client loses the oldest frames rather than the
                // connection; it resyncs on its next fetch.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Commands don't come over the socket yet — HTTP is the write path.
    while let Some(Ok(msg)) = rx.next().await {
        if matches!(msg, WsMessage::Close(_)) {
            break;
        }
    }
    send.abort();
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Fail {
    status: StatusCode,
    message: String,
}

impl Fail {
    fn not_found(message: String) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message,
        }
    }

    /// A refused invite is the caller's problem, not a server fault — an
    /// expired or already-used code must read differently from a crash.
    fn bad_request(message: String) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }

    fn unauthorized(message: String) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message,
        }
    }
}

impl From<rusqlite::Error> for Fail {
    fn from(err: rusqlite::Error) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: err.to_string(),
        }
    }
}

impl IntoResponse for Fail {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    struct Fixture {
        app: Router,
        user_a: String,
        user_b: String,
        machine_a: String,
        machine_b: String,
        chat_a: String,
        message_a: String,
        agent_a: String,
        agent_b: String,
    }

    fn fixture() -> Fixture {
        let store = Arc::new(Store::open(":memory:").unwrap());
        let session_a = store.bootstrap_workspace().unwrap();
        let session_b = store.bootstrap_workspace().unwrap();
        let chat_a = store
            .create_chat("same-name", None, &session_a.user.id, now_millis())
            .unwrap();
        let chat_b = store
            .create_chat("same-name", None, &session_b.user.id, now_millis())
            .unwrap();
        let (events, _) = broadcast::channel(32);
        let state = App {
            store: store.clone(),
            events,
        };
        let message_a = store
            .append(
                new_message(
                    &state,
                    &chat_a.id,
                    &session_a.user.id,
                    MessageKind::Text,
                    "workspace a only".into(),
                    None,
                )
                .unwrap(),
            )
            .unwrap();

        let invite_a = store
            .create_invite(
                &chat_a.id,
                &session_a.user.id,
                HistoryGrant::All,
                now_millis(),
            )
            .unwrap()
            .unwrap();
        let agent_a = store
            .upsert_agent("same-agent", &session_a.user.id, Some("a-host"))
            .unwrap();
        store
            .redeem(&invite_a.code, &agent_a.id, now_millis())
            .unwrap()
            .unwrap();

        let invite_b = store
            .create_invite(
                &chat_b.id,
                &session_b.user.id,
                HistoryGrant::All,
                now_millis(),
            )
            .unwrap()
            .unwrap();
        let agent_b = store
            .upsert_agent("same-agent", &session_b.user.id, Some("b-host"))
            .unwrap();
        store
            .redeem(&invite_b.code, &agent_b.id, now_millis())
            .unwrap()
            .unwrap();

        let pairing_a = store.create_pairing(&session_a.user.id).unwrap();
        let machine_a = store.redeem_pairing(&pairing_a.code).unwrap().unwrap();
        let pairing_b = store.create_pairing(&session_b.user.id).unwrap();
        let machine_b = store.redeem_pairing(&pairing_b.code).unwrap().unwrap();

        Fixture {
            app: router(state),
            user_a: session_a.token,
            user_b: session_b.token,
            machine_a,
            machine_b,
            chat_a: chat_a.id,
            message_a: message_a.id,
            agent_a: agent_a.id,
            agent_b: agent_b.id,
        }
    }

    fn request(method: Method, uri: &str, token: Option<&str>, body: &str) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        if !body.is_empty() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }
        builder.body(Body::from(body.to_owned())).unwrap()
    }

    #[tokio::test]
    async fn messages_are_private_to_chat_members() {
        let f = fixture();
        let anonymous = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/messages/{}", f.message_a),
                None,
                "",
            ))
            .await
            .unwrap();
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);

        let outsider = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/messages/{}", f.message_a),
                Some(&f.user_b),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(outsider.status(), StatusCode::NOT_FOUND);

        let member = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/messages/{}", f.message_a),
                Some(&f.user_a),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn same_named_chats_and_agents_do_not_cross_workspaces() {
        let f = fixture();
        assert_ne!(f.agent_a, f.agent_b);

        let own = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                "/chats/same-name/messages",
                Some(&f.user_a),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(own.status(), StatusCode::OK);

        let hidden_memberships = f
            .app
            .clone()
            .oneshot(request(
                Method::GET,
                &format!("/members/{}/memberships", f.agent_a),
                Some(&f.user_b),
                "",
            ))
            .await
            .unwrap();
        assert_eq!(hidden_memberships.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn only_the_paired_machine_can_advance_its_agent() {
        let f = fixture();
        let uri = format!("/chats/{}/unread?as={}", f.chat_a, f.agent_a);

        let user_impersonation = f
            .app
            .clone()
            .oneshot(request(Method::GET, &uri, Some(&f.user_a), ""))
            .await
            .unwrap();
        assert_eq!(user_impersonation.status(), StatusCode::UNAUTHORIZED);

        let wrong_machine = f
            .app
            .clone()
            .oneshot(request(Method::GET, &uri, Some(&f.machine_b), ""))
            .await
            .unwrap();
        assert_eq!(wrong_machine.status(), StatusCode::NOT_FOUND);

        let paired_machine = f
            .app
            .clone()
            .oneshot(request(Method::GET, &uri, Some(&f.machine_a), ""))
            .await
            .unwrap();
        assert_eq!(paired_machine.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn machines_cannot_mutate_user_owned_chat_settings() {
        let f = fixture();
        let response = f
            .app
            .clone()
            .oneshot(request(
                Method::PATCH,
                &format!("/chats/{}", f.chat_a),
                Some(&f.machine_a),
                r#"{"name":"hijacked"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn trigger_changes_require_a_chat_member_user() {
        let f = fixture();
        let uri = format!("/chats/{}/members/{}", f.chat_a, f.agent_a);

        let outsider = f
            .app
            .clone()
            .oneshot(request(
                Method::POST,
                &uri,
                Some(&f.user_b),
                r#"{"trigger":"mention"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(outsider.status(), StatusCode::NOT_FOUND);

        let member = f
            .app
            .clone()
            .oneshot(request(
                Method::POST,
                &uri,
                Some(&f.user_a),
                r#"{"trigger":"mention"}"#,
            ))
            .await
            .unwrap();
        assert_eq!(member.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn cors_allows_tauri_and_rejects_random_websites() {
        let f = fixture();
        let preflight = |origin: &'static str| {
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/chats")
                .header(header::ORIGIN, origin)
                .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
                .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "authorization")
                .body(Body::empty())
                .unwrap()
        };

        let allowed = f
            .app
            .clone()
            .oneshot(preflight("http://localhost:1420"))
            .await
            .unwrap();
        assert_eq!(
            allowed.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN),
            Some(&HeaderValue::from_static("http://localhost:1420"))
        );

        let rejected = f
            .app
            .clone()
            .oneshot(preflight("https://malicious.example"))
            .await
            .unwrap();
        assert!(rejected
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .is_none());
    }
}

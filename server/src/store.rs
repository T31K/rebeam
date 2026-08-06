//! SQLite persistence.
//!
//! Messages *are* the log: one append-only table with a per-chat monotonic
//! `seq`, which is all a reconnecting client needs to catch up. Status events
//! never reach this module — see [`rebeam_core::Event::is_durable`].

use std::{
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use rebeam_core::{
    Chat, ChatKind, HistoryGrant, Invite, Member, MemberKind, Membership, Message, MessageKind,
    Presence, Trigger,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub email: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct AuthSession {
    pub token: String,
    pub user: User,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingCode {
    pub code: String,
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(SCHEMA)?;
        migrate(&conn)?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.seed()?;
        Ok(store)
    }

    // -- authentication ---------------------------------------------------

    /// Create an anonymous, device-owned workspace. There is no password or
    /// account: possession of the returned device credential is the boundary.
    pub fn bootstrap_workspace(&self) -> rusqlite::Result<AuthSession> {
        let id = format!("u_{}", uuid::Uuid::new_v4().simple());
        let user = User {
            id: id.clone(),
            email: format!("{id}@local.invalid"),
            name: "Local workspace".into(),
        };
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (id, email, name, password_hash, created_at) VALUES (?1, ?2, ?3, '', ?4)",
            params![user.id, user.email, user.name, now_millis()],
        )?;
        Self::ensure_user_member(&conn, &user)?;
        Self::new_session(&conn, user)
    }

    pub fn user_for_token(&self, token: &str) -> rusqlite::Result<Option<User>> {
        let conn = self.conn.lock().unwrap();
        let user = conn
            .query_row(
                "SELECT users.id, users.email, users.name
             FROM sessions JOIN users ON users.id = sessions.user_id
             WHERE sessions.token_hash = ?1",
                params![token_hash(token)],
                |r| {
                    Ok(User {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        name: r.get(2)?,
                    })
                },
            )
            .optional()?;
        if let Some(user) = &user {
            Self::ensure_user_member(&conn, user)?;
        }
        Ok(user)
    }

    pub fn revoke_session(&self, token: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM sessions WHERE token_hash = ?1",
            params![token_hash(token)],
        )?;
        Ok(())
    }

    pub fn machine_for_token(&self, token: &str) -> rusqlite::Result<Option<User>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT users.id, users.email, users.name FROM machine_tokens
             JOIN users ON users.id = machine_tokens.workspace_id WHERE machine_tokens.token_hash = ?1",
            params![token_hash(token)],
            |r| Ok(User { id: r.get(0)?, email: r.get(1)?, name: r.get(2)? }),
        ).optional()
    }

    pub fn agent_belongs_to(&self, agent: &str, workspace: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT 1 FROM members WHERE id = ?1 AND kind = 'agent' AND owner_id = ?2",
                params![agent, workspace],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn machine_status(&self, workspace: &str) -> rusqlite::Result<(i64, i64)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*), COALESCE(SUM(last_seen >= ?2), 0) FROM machine_tokens WHERE workspace_id = ?1", params![workspace, now_millis() - 45_000], |r| Ok((r.get(0)?, r.get(1)?)))
    }

    pub fn touch_machine(&self, token: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn.execute(
            "UPDATE machine_tokens SET last_seen = ?2 WHERE token_hash = ?1",
            params![token_hash(token), now_millis()],
        )? > 0)
    }

    pub fn create_pairing(&self, workspace: &str) -> rusqlite::Result<PairingCode> {
        let code = format!(
            "RP-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12].to_ascii_uppercase()
        );
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pairings (code, workspace_id, expires_at) VALUES (?1, ?2, ?3)",
            params![code, workspace, now_millis() + 600_000],
        )?;
        Ok(PairingCode { code })
    }

    pub fn redeem_pairing(&self, code: &str) -> rusqlite::Result<Result<String, String>> {
        let conn = self.conn.lock().unwrap();
        let found: Option<(String, i64, Option<i64>)> = conn
            .query_row(
                "SELECT workspace_id, expires_at, redeemed_at FROM pairings WHERE code = ?1",
                params![code],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((workspace, expires, redeemed)) = found else {
            return Ok(Err("no such pairing code".into()));
        };
        if redeemed.is_some() || now_millis() >= expires {
            return Ok(Err("that pairing code has expired".into()));
        }
        let token = format!("rm_{}", uuid::Uuid::new_v4().simple());
        conn.execute(
            "UPDATE pairings SET redeemed_at = ?2 WHERE code = ?1",
            params![code, now_millis()],
        )?;
        conn.execute("INSERT INTO machine_tokens (token_hash, workspace_id, created_at, last_seen) VALUES (?1, ?2, ?3, ?3)", params![token_hash(&token), workspace, now_millis()])?;
        Ok(Ok(token))
    }

    fn new_session(conn: &Connection, user: User) -> rusqlite::Result<AuthSession> {
        let token = format!("rb_{}", uuid::Uuid::new_v4().simple());
        conn.execute(
            "INSERT INTO sessions (token_hash, user_id, created_at) VALUES (?1, ?2, ?3)",
            params![token_hash(&token), user.id, now_millis()],
        )?;
        Ok(AuthSession { token, user })
    }

    /// A signed-in person is also a chat member. Keeping these IDs equal lets
    /// membership checks be the single visibility boundary for the MVP.
    fn ensure_user_member(conn: &Connection, user: &User) -> rusqlite::Result<()> {
        let position: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM members",
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO members (id, name, kind, presence, position)
             VALUES (?1, ?2, 'human', 'online', ?3)",
            params![user.id, user.name, position],
        )?;
        Ok(())
    }

    // -- directory ---------------------------------------------------------

    pub fn chats(&self) -> rusqlite::Result<Vec<Chat>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT id, name, kind, topic, avatar_seed FROM chats ORDER BY position")?;
        let rows = stmt.query_map([], |r| {
            Ok(Chat {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: match r.get::<_, String>(2)?.as_str() {
                    "dm" => ChatKind::Dm,
                    _ => ChatKind::Group,
                },
                member_ids: Vec::new(),
                topic: r.get(3)?,
                avatar_seed: r.get(4)?,
            })
        })?;
        let mut chats: Vec<Chat> = rows.collect::<rusqlite::Result<_>>()?;

        let mut members = conn.prepare("SELECT member_id FROM chat_members WHERE chat_id = ?1")?;
        for chat in &mut chats {
            chat.member_ids = members
                .query_map(params![chat.id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(chats)
    }

    /// Only chats the signed-in member belongs to. Seeded demo chats are
    /// therefore invisible to every new account.
    pub fn chats_for_member(&self, member: &str) -> rusqlite::Result<Vec<Chat>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chats.id, chats.name, chats.kind, chats.topic, chats.avatar_seed
             FROM chats JOIN chat_members ON chat_members.chat_id = chats.id
             WHERE chat_members.member_id = ?1 ORDER BY chats.position",
        )?;
        let rows = stmt.query_map(params![member], |r| {
            Ok(Chat {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: if r.get::<_, String>(2)? == "dm" {
                    ChatKind::Dm
                } else {
                    ChatKind::Group
                },
                member_ids: Vec::new(),
                topic: r.get(3)?,
                avatar_seed: r.get(4)?,
            })
        })?;
        let mut chats: Vec<Chat> = rows.collect::<rusqlite::Result<_>>()?;
        let mut members = conn.prepare("SELECT member_id FROM chat_members WHERE chat_id = ?1")?;
        for chat in &mut chats {
            chat.member_ids = members
                .query_map(params![chat.id], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<_>>()?;
        }
        Ok(chats)
    }

    pub fn update_chat(
        &self,
        id: &str,
        name: Option<&str>,
        topic: Option<&str>,
        avatar_seed: Option<&str>,
    ) -> rusqlite::Result<Option<Chat>> {
        {
            let conn = self.conn.lock().unwrap();
            if let Some(name) = name {
                conn.execute(
                    "UPDATE chats SET name = ?2 WHERE id = ?1",
                    params![id, name],
                )?;
            }
            if let Some(topic) = topic {
                let topic = (!topic.is_empty()).then_some(topic);
                conn.execute(
                    "UPDATE chats SET topic = ?2 WHERE id = ?1",
                    params![id, topic],
                )?;
            }
            if let Some(avatar_seed) = avatar_seed {
                let avatar_seed = (!avatar_seed.is_empty()).then_some(avatar_seed);
                conn.execute(
                    "UPDATE chats SET avatar_seed = ?2 WHERE id = ?1",
                    params![id, avatar_seed],
                )?;
            }
        }
        Ok(self.chats()?.into_iter().find(|chat| chat.id == id))
    }

    pub fn members(&self) -> rusqlite::Result<Vec<Member>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, presence, bio, owner_id, host FROM members ORDER BY position",
        )?;
        let rows = stmt.query_map([], row_to_member)?;
        rows.collect()
    }

    pub fn members_for_member(&self, member: &str) -> rusqlite::Result<Vec<Member>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, presence, bio, owner_id, host FROM members
             WHERE id = ?1 OR id IN (
               SELECT DISTINCT other.member_id FROM chat_members AS mine
               JOIN chat_members AS other ON other.chat_id = mine.chat_id
               WHERE mine.member_id = ?1
             ) ORDER BY position",
        )?;
        let rows = stmt.query_map(params![member], row_to_member)?;
        let members = rows.collect::<rusqlite::Result<_>>()?;
        Ok(members)
    }

    pub fn is_member_of_chat(&self, chat: &str, member: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        Ok(conn
            .query_row(
                "SELECT 1 FROM chat_members WHERE chat_id = ?1 AND member_id = ?2",
                params![chat, member],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    // -- the log -----------------------------------------------------------

    pub fn messages(&self, chat: &str) -> rusqlite::Result<Vec<Message>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at
             FROM messages WHERE chat_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map(params![chat], row_to_message)?;
        rows.collect()
    }

    /// Append a message, assigning its per-chat `seq` under the same lock —
    /// which is what makes the sequence monotonic without any coordination.
    pub fn append(&self, mut msg: Message) -> rusqlite::Result<Message> {
        let conn = self.conn.lock().unwrap();
        let next: i64 = conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE chat_id = ?1",
            params![msg.channel_id],
            |r| r.get(0),
        )?;
        msg.seq = next;
        conn.execute(
            "INSERT INTO messages
               (id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                msg.id,
                msg.channel_id,
                msg.author_id,
                msg.seq,
                kind_str(msg.kind),
                msg.text,
                msg.options
                    .as_ref()
                    .map(|o| serde_json::to_string(o).unwrap()),
                msg.resolved_option,
                msg.created_at,
            ],
        )?;
        Ok(msg)
    }

    pub fn resolve(&self, id: &str, option: &str) -> rusqlite::Result<Option<Message>> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE messages SET resolved_option = ?2 WHERE id = ?1 AND resolved_option IS NULL",
            params![id, option],
        )?;
        conn.query_row(
            "SELECT id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at
             FROM messages WHERE id = ?1",
            params![id],
            row_to_message,
        )
        .optional()
    }

    pub fn message(&self, id: &str) -> rusqlite::Result<Option<Message>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at
             FROM messages WHERE id = ?1",
            params![id],
            row_to_message,
        )
        .optional()
    }

    /// Resolve a chat by id or by (case-insensitive) name — so the CLI can say
    /// `-c warroom` instead of `-c g_warroom`.
    pub fn find_chat(&self, needle: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM chats
             WHERE id = ?1 COLLATE NOCASE
                OR name = ?1 COLLATE NOCASE
                OR REPLACE(LOWER(name), ' ', '') = REPLACE(LOWER(?1), ' ', '')
             LIMIT 1",
            params![needle],
            |r| r.get(0),
        )
        .optional()
    }

    pub fn find_chat_for_member(
        &self,
        needle: &str,
        member: &str,
    ) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT chats.id FROM chats
             JOIN chat_members ON chat_members.chat_id = chats.id
             WHERE chat_members.member_id = ?2 AND (
               chats.id = ?1 COLLATE NOCASE
               OR chats.name = ?1 COLLATE NOCASE
               OR REPLACE(LOWER(chats.name), ' ', '') = REPLACE(LOWER(?1), ' ', '')
             ) LIMIT 1",
            params![needle, member],
            |r| r.get(0),
        )
        .optional()
    }

    pub fn find_member(&self, needle: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id FROM members WHERE id = ?1 COLLATE NOCASE OR name = ?1 COLLATE NOCASE LIMIT 1",
            params![needle],
            |r| r.get(0),
        )
        .optional()
    }

    // -- membership --------------------------------------------------------

    /// The head of a chat's log. The top of every unread window.
    pub fn head_seq(&self, chat: &str) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM messages WHERE chat_id = ?1",
            params![chat],
            |r| r.get(0),
        )
    }

    /// Resolve a history grant to a seq floor, at redemption time.
    fn floor_for(conn: &Connection, chat: &str, grant: HistoryGrant) -> rusqlite::Result<i64> {
        Ok(match grant {
            HistoryGrant::All => 0,
            HistoryGrant::None => conn.query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM messages WHERE chat_id = ?1",
                params![chat],
                |r| r.get(0),
            )?,
            // The last seq strictly before the cutoff, so the first readable
            // message is the first one at or after it.
            HistoryGrant::Since { ms } => conn.query_row(
                "SELECT COALESCE(MAX(seq), 0) FROM messages
                 WHERE chat_id = ?1 AND created_at < ?2",
                params![chat, ms],
                |r| r.get(0),
            )?,
        })
    }

    pub fn membership(&self, chat: &str, member: &str) -> rusqlite::Result<Option<Membership>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT chat_id, member_id, invited_by, joined_at, history_floor_seq, cursor_seq, trigger
             FROM chat_members WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member],
            row_to_membership,
        )
        .optional()
    }

    pub fn memberships_of(&self, member: &str) -> rusqlite::Result<Vec<Membership>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT chat_id, member_id, invited_by, joined_at, history_floor_seq, cursor_seq, trigger
             FROM chat_members WHERE member_id = ?1",
        )?;
        let rows = stmt.query_map(params![member], row_to_membership)?;
        let out: Vec<Membership> = rows.collect::<rusqlite::Result<_>>()?;
        Ok(out)
    }

    /// Messages a member is allowed to see and has not seen, oldest first.
    /// Clamped to the floor, so a rewound cursor can never reach withheld
    /// history, and capped so a long backlog cannot blow up one turn.
    pub fn unread(&self, chat: &str, member: &str, limit: usize) -> rusqlite::Result<Vec<Message>> {
        let Some(m) = self.membership(chat, member)? else {
            return Ok(Vec::new());
        };
        let from = m.cursor_seq.max(m.history_floor_seq);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, chat_id, author_id, seq, kind, text, options, resolved_option, created_at
             FROM messages
             WHERE chat_id = ?1 AND seq > ?2 AND kind != 'system'
             ORDER BY seq DESC LIMIT ?3",
        )?;
        let mut rows: Vec<Message> = stmt
            .query_map(params![chat, from, limit as i64], row_to_message)?
            .collect::<rusqlite::Result<_>>()?;
        // Take the newest N, then restore reading order.
        rows.reverse();
        Ok(rows)
    }

    /// Mark everything up to `seq` as read. Never moves backwards — a late
    /// turn finishing after a newer one must not replay what it already saw.
    pub fn advance_cursor(&self, chat: &str, member: &str, seq: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_members SET cursor_seq = MAX(cursor_seq, ?3)
             WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member, seq],
        )?;
        Ok(())
    }

    pub fn reset_cursor(&self, chat: &str, member: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_members SET cursor_seq = history_floor_seq
             WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member],
        )?;
        Ok(())
    }

    pub fn set_trigger(&self, chat: &str, member: &str, trigger: Trigger) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE chat_members SET trigger = ?3 WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member, trigger_str(trigger)],
        )?;
        Ok(())
    }

    /// Remove a member from a chat. The same gesture for humans and agents,
    /// and the only revocation there is.
    pub fn kick(&self, chat: &str, member: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let gone = conn.execute(
            "DELETE FROM chat_members WHERE chat_id = ?1 AND member_id = ?2",
            params![chat, member],
        )?;
        conn.execute(
            "DELETE FROM invites WHERE chat_id = ?1 AND redeemed_by = ?2",
            params![chat, member],
        )?;
        Ok(gone > 0)
    }

    /// A new group, with its creator already in it. Everyone else arrives by
    /// invite — including the agents.
    pub fn create_chat(
        &self,
        name: &str,
        topic: Option<&str>,
        creator: &str,
        now: i64,
    ) -> rusqlite::Result<Chat> {
        let conn = self.conn.lock().unwrap();
        let id = format!("g_{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        let position: i64 = conn.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM chats",
            [],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT INTO chats (id, name, kind, topic, position) VALUES (?1, ?2, 'group', ?3, ?4)",
            params![id, name, topic, position],
        )?;
        conn.execute(
            "INSERT INTO chat_members
               (chat_id, member_id, joined_at, history_floor_seq, cursor_seq, trigger)
             VALUES (?1, ?2, ?3, 0, 0, 'all')",
            params![id, creator, now],
        )?;
        Ok(Chat {
            id,
            name: name.to_string(),
            kind: ChatKind::Group,
            member_ids: vec![creator.into()],
            topic: topic.map(str::to_string),
            avatar_seed: None,
        })
    }

    // -- invites -----------------------------------------------------------

    pub fn create_invite(
        &self,
        chat: &str,
        invited_by: &str,
        history: HistoryGrant,
        now: i64,
    ) -> rusqlite::Result<Result<Invite, String>> {
        let conn = self.conn.lock().unwrap();

        // Expired codes are dead weight, not pending invites.
        conn.execute(
            "DELETE FROM invites
             WHERE chat_id = ?1 AND redeemed_by IS NULL AND expires_at <= ?2",
            params![chat, now],
        )?;

        // Inviting three agents to one group means three live codes, so the
        // cap is a runaway backstop rather than a workflow limit. The tight
        // caps in OpenClaw and Hermes guard *inbound* requests from strangers;
        // here the inviter is already inside.
        let pending: usize = conn.query_row(
            "SELECT COUNT(*) FROM invites
             WHERE chat_id = ?1 AND redeemed_by IS NULL AND expires_at > ?2",
            params![chat, now],
            |r| r.get::<_, i64>(0).map(|n| n as usize),
        )?;
        if pending >= Invite::MAX_PENDING {
            return Ok(Err(format!(
                "{} people already have open invites to this chat",
                Invite::MAX_PENDING
            )));
        }

        let invite = Invite {
            code: new_code(),
            chat: chat.to_string(),
            invited_by: invited_by.to_string(),
            history,
            expires_at: now + Invite::TTL_MS,
            redeemed_by: None,
        };
        conn.execute(
            "INSERT INTO invites (code, chat_id, invited_by, history, expires_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                rebeam_core::normalize_code(&invite.code),
                invite.chat,
                invite.invited_by,
                serde_json::to_string(&invite.history).unwrap(),
                invite.expires_at,
                now,
            ],
        )?;
        Ok(Ok(invite))
    }

    /// Who issued an invite. The joining agent's owner, and therefore where
    /// its approvals will route once it is lent to somebody else's chat.
    pub fn invite_inviter(&self, code: &str) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT invited_by FROM invites WHERE code = ?1",
            params![rebeam_core::normalize_code(code)],
            |r| r.get(0),
        )
        .optional()
    }

    /// Exchange a code for a membership. Single-use: the same code redeemed
    /// twice fails the second time.
    pub fn redeem(
        &self,
        code: &str,
        member: &str,
        now: i64,
    ) -> rusqlite::Result<Result<Membership, String>> {
        let key = rebeam_core::normalize_code(code);
        let conn = self.conn.lock().unwrap();

        let found: Option<(String, String, String, i64, Option<String>)> = conn
            .query_row(
                "SELECT chat_id, invited_by, history, expires_at, redeemed_by
                 FROM invites WHERE code = ?1",
                params![key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?;

        let Some((chat, invited_by, history, expires_at, redeemed_by)) = found else {
            return Ok(Err("no such invite".into()));
        };
        if redeemed_by.is_some() {
            return Ok(Err("that invite was already used".into()));
        }
        if now >= expires_at {
            return Ok(Err("that invite has expired".into()));
        }

        let grant: HistoryGrant = serde_json::from_str(&history).unwrap_or_default();
        let floor = Self::floor_for(&conn, &chat, grant)?;

        // Every agent hears the room. The gateway supplies the roster and
        // explicitly tells agents to stay silent when another member is being
        // addressed; per-agent loop budgets stop accidental reply storms.
        let trigger = Trigger::All;

        conn.execute(
            "UPDATE invites SET redeemed_by = ?2 WHERE code = ?1",
            params![key, member],
        )?;
        conn.execute(
            "INSERT INTO chat_members
               (chat_id, member_id, invited_by, joined_at, history_floor_seq, cursor_seq, trigger)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6)
             ON CONFLICT (chat_id, member_id) DO UPDATE SET
               history_floor_seq = MIN(history_floor_seq, excluded.history_floor_seq)",
            params![chat, member, invited_by, now, floor, trigger_str(trigger)],
        )?;

        Ok(Ok(Membership {
            chat,
            member: member.to_string(),
            invited_by: Some(invited_by),
            joined_at: now,
            history_floor_seq: floor,
            cursor_seq: floor,
            trigger,
        }))
    }

    /// Mint an agent identity for a joining host, or reuse the existing one so
    /// the same agent can join a second chat without becoming a second member.
    pub fn upsert_agent(
        &self,
        name: &str,
        owner_id: &str,
        host: Option<&str>,
    ) -> rusqlite::Result<Member> {
        // The name is the mention handle, so it is normalized before it is
        // ever stored — a member called "Hermes 1" could never be triggered.
        let name = &rebeam_core::handle(name);
        let conn = self.conn.lock().unwrap();
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM members
                 WHERE name = ?1 COLLATE NOCASE AND owner_id = ?2",
                params![name, owner_id],
                |r| r.get(0),
            )
            .optional()?;

        let id = match existing {
            Some(id) => {
                conn.execute(
                    "UPDATE members SET owner_id = ?2, host = COALESCE(?3, host), presence = 'online' WHERE id = ?1",
                    params![id, owner_id, host],
                )?;
                id
            }
            None => {
                // The id is derived from the name for readability, but the
                // two are independent: seeded members have ids that do not
                // match their names (`claude-main` is `a_claude`), so a new
                // agent's slug can collide with an id already in use.
                let base = format!("a_{}", slug(name));
                let mut id = base.clone();
                let mut n = 2;
                while conn
                    .query_row("SELECT 1 FROM members WHERE id = ?1", params![id], |_| {
                        Ok(())
                    })
                    .optional()?
                    .is_some()
                {
                    id = format!("{base}-{n}");
                    n += 1;
                }
                let position: i64 = conn.query_row(
                    "SELECT COALESCE(MAX(position), 0) + 1 FROM members",
                    [],
                    |r| r.get(0),
                )?;
                conn.execute(
                    "INSERT INTO members (id, name, kind, presence, owner_id, host, position)
                     VALUES (?1, ?2, 'agent', 'online', ?3, ?4, ?5)",
                    params![id, name, owner_id, host, position],
                )?;
                id
            }
        };

        conn.query_row(
            "SELECT id, name, kind, presence, bio, owner_id, host FROM members WHERE id = ?1",
            params![id],
            row_to_member,
        )
    }

    fn seed(&self) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM members", [], |r| r.get(0))?;
        if count > 0 {
            return Ok(());
        }
        conn.execute_batch(SEED)?;
        Ok(())
    }
}

fn row_to_message(r: &rusqlite::Row<'_>) -> rusqlite::Result<Message> {
    let options: Option<String> = r.get(6)?;
    Ok(Message {
        id: r.get(0)?,
        channel_id: r.get(1)?,
        author_id: r.get(2)?,
        seq: r.get(3)?,
        kind: match r.get::<_, String>(4)?.as_str() {
            "ask" => MessageKind::Ask,
            "system" => MessageKind::System,
            _ => MessageKind::Text,
        },
        text: r.get(5)?,
        options: options.and_then(|o| serde_json::from_str(&o).ok()),
        resolved_option: r.get(7)?,
        created_at: r.get(8)?,
    })
}

fn kind_str(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::Text => "text",
        MessageKind::Ask => "ask",
        MessageKind::System => "system",
    }
}

fn row_to_member(r: &rusqlite::Row<'_>) -> rusqlite::Result<Member> {
    Ok(Member {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: match r.get::<_, String>(2)?.as_str() {
            "human" => MemberKind::Human,
            _ => MemberKind::Agent,
        },
        presence: match r.get::<_, String>(3)?.as_str() {
            "online" => Presence::Online,
            _ => Presence::Offline,
        },
        bio: r.get(4)?,
        owner_id: r.get(5)?,
        host: r.get(6)?,
    })
}

fn row_to_membership(r: &rusqlite::Row<'_>) -> rusqlite::Result<Membership> {
    Ok(Membership {
        chat: r.get(0)?,
        member: r.get(1)?,
        invited_by: r.get(2)?,
        joined_at: r.get(3)?,
        history_floor_seq: r.get(4)?,
        cursor_seq: r.get(5)?,
        trigger: match r.get::<_, String>(6)?.as_str() {
            "all" => Trigger::All,
            "never" => Trigger::Never,
            _ => Trigger::Mention,
        },
    })
}

fn trigger_str(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Mention => "mention",
        Trigger::All => "all",
        Trigger::Never => "never",
    }
}

/// A code from the unambiguous alphabet. The alphabet is exactly 32 long, so
/// masking a random byte to 5 bits is uniform — no modulo skew.
fn new_code() -> String {
    let bytes = uuid::Uuid::new_v4();
    let chars: String = bytes.as_bytes()[..Invite::LEN]
        .iter()
        .map(|b| rebeam_core::CODE_ALPHABET[(b & 0x1f) as usize] as char)
        .collect();
    format!("RB-{}-{}", &chars[..4], &chars[4..])
}

fn slug(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect()
}

fn token_hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Add the membership and identity columns to databases created before they
/// existed. SQLite has no `ADD COLUMN IF NOT EXISTS`, and re-adding one is the
/// only error we expect here, so a failure means "already present".
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    for stmt in MIGRATIONS {
        let _ = conn.execute(stmt, []);
    }
    Ok(())
}

const MIGRATIONS: &[&str] = &[
    "ALTER TABLE chat_members ADD COLUMN invited_by TEXT",
    "ALTER TABLE chat_members ADD COLUMN joined_at INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE chat_members ADD COLUMN history_floor_seq INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE chat_members ADD COLUMN cursor_seq INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE chat_members ADD COLUMN trigger TEXT NOT NULL DEFAULT 'mention'",
    "ALTER TABLE members ADD COLUMN owner_id TEXT",
    "ALTER TABLE members ADD COLUMN host TEXT",
    "ALTER TABLE machine_tokens ADD COLUMN last_seen INTEGER",
];

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS users (
  id TEXT PRIMARY KEY,
  email TEXT NOT NULL UNIQUE COLLATE NOCASE,
  name TEXT NOT NULL,
  password_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
  token_hash TEXT PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS pairings (
  code TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, expires_at INTEGER NOT NULL, redeemed_at INTEGER
);
CREATE TABLE IF NOT EXISTS machine_tokens (
  token_hash TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, created_at INTEGER NOT NULL, last_seen INTEGER
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions(user_id);
CREATE TABLE IF NOT EXISTS members (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
  presence TEXT NOT NULL DEFAULT 'offline', bio TEXT, position INTEGER DEFAULT 0,
  owner_id TEXT, host TEXT
);
CREATE TABLE IF NOT EXISTS chats (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL,
  topic TEXT, avatar_seed TEXT, position INTEGER DEFAULT 0
);
-- One member's standing in one chat. Permissions, context, and revocation all
-- hang off this row.
CREATE TABLE IF NOT EXISTS chat_members (
  chat_id TEXT NOT NULL,
  member_id TEXT NOT NULL,
  invited_by TEXT,
  joined_at INTEGER NOT NULL DEFAULT 0,
  history_floor_seq INTEGER NOT NULL DEFAULT 0,
  cursor_seq INTEGER NOT NULL DEFAULT 0,
  trigger TEXT NOT NULL DEFAULT 'mention',
  PRIMARY KEY (chat_id, member_id)
);
CREATE TABLE IF NOT EXISTS invites (
  code TEXT PRIMARY KEY,
  chat_id TEXT NOT NULL,
  invited_by TEXT NOT NULL,
  history TEXT NOT NULL,
  expires_at INTEGER NOT NULL,
  created_at INTEGER NOT NULL,
  redeemed_by TEXT
);
CREATE INDEX IF NOT EXISTS invites_chat ON invites (chat_id, redeemed_by);
CREATE TABLE IF NOT EXISTS messages (
  id TEXT PRIMARY KEY,
  chat_id TEXT NOT NULL,
  author_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  options TEXT,
  resolved_option TEXT,
  created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS messages_chat_seq ON messages (chat_id, seq);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_metadata_round_trips_and_can_be_cleared() {
        let store = Store::open(":memory:").unwrap();
        let workspace = store.bootstrap_workspace().unwrap().user;
        let chat = store
            .create_chat("before", None, &workspace.id, now_millis())
            .unwrap();

        let updated = store
            .update_chat(
                &chat.id,
                Some("after"),
                Some("a durable topic"),
                Some("seed-42"),
            )
            .unwrap()
            .unwrap();
        assert_eq!(updated.name, "after");
        assert_eq!(updated.topic.as_deref(), Some("a durable topic"));
        assert_eq!(updated.avatar_seed.as_deref(), Some("seed-42"));

        let cleared = store
            .update_chat(&chat.id, None, Some(""), Some(""))
            .unwrap()
            .unwrap();
        assert_eq!(cleared.topic, None);
        assert_eq!(cleared.avatar_seed, None);
    }
}

const SEED: &str = r#"
INSERT INTO members (id, name, kind, presence, bio, position) VALUES
  ('u_t31k', 't31k', 'human', 'online', NULL, 0),
  ('a_claude', 'claude-main', 'agent', 'online', 'coding, deploys, general dogsbody', 1),
  ('a_kimi', 'kimi-research', 'agent', 'online', 'web research, long-doc summarization', 2),
  ('a_codex', 'codex-ci', 'agent', 'offline', 'test runs, CI babysitting', 3);

INSERT INTO chats (id, name, kind, topic, position) VALUES
  ('c_dm_claude', 'claude-main', 'dm', NULL, 0),
  ('c_dm_kimi', 'kimi-research', 'dm', NULL, 1),
  ('c_dm_codex', 'codex-ci', 'dm', NULL, 2),
  ('g_warroom', 'War Room', 'group', 'everyone, everything urgent', 3),
  ('g_shipit', 'Ship It', 'group', 'deploys and approvals', 4),
  ('g_research', 'Research Corner', 'group', 'kimi''s territory', 5);

INSERT INTO chat_members (chat_id, member_id) VALUES
  ('c_dm_claude', 'u_t31k'), ('c_dm_claude', 'a_claude'),
  ('c_dm_kimi', 'u_t31k'), ('c_dm_kimi', 'a_kimi'),
  ('c_dm_codex', 'u_t31k'), ('c_dm_codex', 'a_codex'),
  ('g_warroom', 'u_t31k'), ('g_warroom', 'a_claude'), ('g_warroom', 'a_kimi'), ('g_warroom', 'a_codex'),
  ('g_shipit', 'u_t31k'), ('g_shipit', 'a_claude'), ('g_shipit', 'a_codex'),
  ('g_research', 'u_t31k'), ('g_research', 'a_kimi');
"#;

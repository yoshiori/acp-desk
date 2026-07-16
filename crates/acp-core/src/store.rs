//! SQLite persistence for sessions, messages, and usage (DESIGN.md §6).
//!
//! Connections are cheap and not shared: each session thread and each query
//! site opens its own `Store` on the same file; WAL mode keeps concurrent
//! readers and the single writer happy. Content is stored as raw JSON blocks
//! so new ACP content types don't force schema migrations.

use std::path::Path;

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

/// Migrations, applied in order; `PRAGMA user_version` tracks progress.
/// Append new statements — never edit shipped ones.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE sessions (
      id            TEXT PRIMARY KEY,        -- ACP session id
      agent_name    TEXT NOT NULL,
      cwd           TEXT NOT NULL,
      title         TEXT,
      created_at    INTEGER NOT NULL,        -- unix seconds
      updated_at    INTEGER NOT NULL
    ) STRICT;

    CREATE TABLE messages (
      id             INTEGER PRIMARY KEY,
      session_id     TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
      acp_message_id TEXT,
      role           TEXT NOT NULL CHECK (role IN
                       ('user','assistant','thought','tool','system')),
      content_json   TEXT NOT NULL,          -- serialized content blocks
      status         TEXT,                   -- tool rows: final call status
      created_at     INTEGER NOT NULL
    ) STRICT;
    CREATE INDEX messages_by_session ON messages(session_id, id);

    CREATE TABLE usage_events (
      id            INTEGER PRIMARY KEY,
      session_id    TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
      used_tokens   INTEGER NOT NULL,
      context_size  INTEGER NOT NULL,
      cost_amount   REAL,
      cost_currency TEXT,
      created_at    INTEGER NOT NULL
    ) STRICT;
"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: String,
    pub agent_name: String,
    pub cwd: String,
    pub title: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MessageRow {
    pub role: String,
    /// JSON array of content blocks; today always `[{"type":"text",...}]`.
    pub content_json: String,
    pub acp_message_id: Option<String>,
    pub status: Option<String>,
}

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Writer (session thread) and readers (UI queries) share the file;
        // without a timeout a locked database surfaces as an instant error.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    /// In-memory store for tests.
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Self::migrate(&conn)?;
        Ok(Self { conn })
    }

    fn migrate(conn: &Connection) -> anyhow::Result<()> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        for (index, statements) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(statements)?;
            conn.pragma_update(None, "user_version", (index + 1) as i64)?;
        }
        Ok(())
    }

    /// Registers a session, or bumps `updated_at` if it already exists
    /// (a session/load reconnect reuses the ACP session id).
    pub fn record_session(
        &self,
        id: &str,
        agent_name: &str,
        cwd: &str,
        now: i64,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, agent_name, cwd, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET updated_at = ?4",
            params![id, agent_name, cwd, now],
        )?;
        Ok(())
    }

    /// Appends one message and touches the session. The first user message
    /// doubles as the session title (what the sidebar will show).
    pub fn append_message(
        &self,
        session_id: &str,
        message: &MessageRow,
        now: i64,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO messages
               (session_id, acp_message_id, role, content_json, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id,
                message.acp_message_id,
                message.role,
                message.content_json,
                message.status,
                now
            ],
        )?;
        self.conn.execute(
            "UPDATE sessions SET updated_at = ?2,
               title = COALESCE(title, CASE WHEN ?3 = 'user' THEN ?4 END)
             WHERE id = ?1",
            params![
                session_id,
                now,
                message.role,
                title_of(&message.content_json)
            ],
        )?;
        Ok(())
    }

    pub fn record_usage(
        &self,
        session_id: &str,
        used_tokens: u64,
        context_size: u64,
        cost_amount: Option<f64>,
        cost_currency: Option<&str>,
        now: i64,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO usage_events
               (session_id, used_tokens, context_size, cost_amount, cost_currency, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                session_id,
                used_tokens as i64,
                context_size as i64,
                cost_amount,
                cost_currency,
                now
            ],
        )?;
        Ok(())
    }

    /// Sessions newest-first, for the sidebar.
    pub fn list_sessions(&self) -> anyhow::Result<Vec<SessionRow>> {
        let mut statement = self.conn.prepare(
            "SELECT id, agent_name, cwd, title, created_at, updated_at
             FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok(SessionRow {
                    id: row.get(0)?,
                    agent_name: row.get(1)?,
                    cwd: row.get(2)?,
                    title: row.get(3)?,
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// A session's transcript in insertion order.
    pub fn load_messages(&self, session_id: &str) -> anyhow::Result<Vec<MessageRow>> {
        let mut statement = self.conn.prepare(
            "SELECT role, content_json, acp_message_id, status
             FROM messages WHERE session_id = ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map([session_id], |row| {
                Ok(MessageRow {
                    role: row.get(0)?,
                    content_json: row.get(1)?,
                    acp_message_id: row.get(2)?,
                    status: row.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }
}

/// Serializes plain text as a one-block content array.
pub fn text_content_json(text: &str) -> String {
    serde_json::json!([{ "type": "text", "text": text }]).to_string()
}

/// First text block of a content array, truncated for use as a title.
fn title_of(content_json: &str) -> Option<String> {
    const MAX_TITLE_CHARS: usize = 80;
    let blocks: serde_json::Value = serde_json::from_str(content_json).ok()?;
    let text = blocks.get(0)?.get("text")?.as_str()?;
    Some(text.chars().take(MAX_TITLE_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_message(role: &str, text: &str) -> MessageRow {
        MessageRow {
            role: role.to_string(),
            content_json: text_content_json(text),
            acp_message_id: None,
            status: None,
        }
    }

    #[test]
    fn migrations_are_idempotent_across_reopens() {
        let dir = std::env::temp_dir().join(format!("acp-store-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        let _ = std::fs::remove_file(&path);

        Store::open(&path).unwrap();
        let store = Store::open(&path).unwrap();
        assert_eq!(store.list_sessions().unwrap(), vec![]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn records_and_lists_sessions_newest_first() {
        let store = Store::open_in_memory().unwrap();
        store.record_session("s1", "Claude Code", "/tmp", 100).unwrap();
        store.record_session("s2", "Claude Code", "/tmp", 200).unwrap();

        let ids: Vec<String> = store
            .list_sessions()
            .unwrap()
            .into_iter()
            .map(|s| s.id)
            .collect();
        assert_eq!(ids, ["s2", "s1"]);
    }

    #[test]
    fn re_recording_a_session_only_bumps_updated_at() {
        let store = Store::open_in_memory().unwrap();
        store.record_session("s1", "Claude Code", "/tmp", 100).unwrap();
        store.record_session("s1", "Claude Code", "/tmp", 300).unwrap();

        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].created_at, 100);
        assert_eq!(sessions[0].updated_at, 300);
    }

    #[test]
    fn first_user_message_becomes_the_title() {
        let store = Store::open_in_memory().unwrap();
        store.record_session("s1", "Claude Code", "/tmp", 100).unwrap();
        store
            .append_message("s1", &text_message("user", "fix the login bug"), 110)
            .unwrap();
        store
            .append_message("s1", &text_message("user", "second question"), 120)
            .unwrap();

        let sessions = store.list_sessions().unwrap();
        assert_eq!(sessions[0].title.as_deref(), Some("fix the login bug"));
        assert_eq!(sessions[0].updated_at, 120);
    }

    #[test]
    fn assistant_messages_do_not_set_the_title() {
        let store = Store::open_in_memory().unwrap();
        store.record_session("s1", "Claude Code", "/tmp", 100).unwrap();
        store
            .append_message("s1", &text_message("assistant", "hello!"), 110)
            .unwrap();
        assert_eq!(store.list_sessions().unwrap()[0].title, None);
    }

    #[test]
    fn loads_messages_in_insertion_order() {
        let store = Store::open_in_memory().unwrap();
        store.record_session("s1", "Claude Code", "/tmp", 100).unwrap();
        store.append_message("s1", &text_message("user", "hi"), 1).unwrap();
        store
            .append_message(
                "s1",
                &MessageRow {
                    role: "tool".to_string(),
                    content_json: text_content_json("Run ls"),
                    acp_message_id: None,
                    status: Some("completed".to_string()),
                },
                2,
            )
            .unwrap();
        store
            .append_message("s1", &text_message("assistant", "done"), 3)
            .unwrap();

        let roles: Vec<String> = store
            .load_messages("s1")
            .unwrap()
            .into_iter()
            .map(|m| m.role)
            .collect();
        assert_eq!(roles, ["user", "tool", "assistant"]);
    }

    #[test]
    fn deleting_a_session_cascades_to_its_rows() {
        let store = Store::open_in_memory().unwrap();
        store.record_session("s1", "Claude Code", "/tmp", 100).unwrap();
        store.append_message("s1", &text_message("user", "hi"), 1).unwrap();
        store
            .record_usage("s1", 100, 200_000, None, None, 2)
            .unwrap();

        store
            .conn
            .execute("DELETE FROM sessions WHERE id = 's1'", [])
            .unwrap();

        assert_eq!(store.load_messages("s1").unwrap(), vec![]);
        let usage: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(usage, 0);
    }

    #[test]
    fn rejects_messages_for_unknown_sessions() {
        let store = Store::open_in_memory().unwrap();
        assert!(
            store
                .append_message("ghost", &text_message("user", "hi"), 1)
                .is_err()
        );
    }

    #[test]
    fn records_usage_events() {
        let store = Store::open_in_memory().unwrap();
        store.record_session("s1", "Claude Code", "/tmp", 100).unwrap();
        store
            .record_usage("s1", 1500, 200_000, Some(0.12), Some("USD"), 110)
            .unwrap();
        // No reader API yet (arrives with the sidebar PR); just prove the row landed.
        let count: i64 = store
            .conn
            .query_row("SELECT COUNT(*) FROM usage_events", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}

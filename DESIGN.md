# acp-desk — Design Document

Standalone desktop chat app for ACP-compatible AI coding agents.

This document is the handoff spec. Read it cold and be able to continue.

---

## 1. Product goals

- **Primary goal**: give the user a "Claude Desktop"-style, single-window chat
  application whose backend is any agent that speaks the
  [Agent Client Protocol (ACP)](https://agentclientprotocol.com/). The user
  should be able to open the app, type a prompt, and have the assistant reply
  as a streamed message, with tool-use / permission requests surfaced in the UI.
- **Secondary goal**: make it trivial to swap the backend agent (Claude Code,
  Gemini CLI, Codex CLI, …). The app itself must not be tied to any one vendor.
- **Explicit non-goals** for v1:
  - Not an editor. No file tree, no code panes, no LSP. If the agent asks to
    edit a file, we display a diff — we do not become an editor.
  - Not a general MCP host. MCP servers are configured on the agent side; the
    app relays through ACP only.
  - No mobile / web build.

## 2. Why this exists

- Zed's Agent Panel already speaks ACP and works, but Zed is an editor.
  A user who just wants "Claude Desktop for Gemini/Codex" gets an editor as a
  by-product. `acp-desk` is the chat-only slice.
- Google shut down the individual-account OAuth path for Gemini CLI mid-2026
  (users are pushed to the Antigravity CLI, which does not yet speak ACP as of
  agy 1.1.2 — its `acp_version` protobuf string is unrelated ADS/analytics
  protobuf). This means Gemini support is best-effort via `GEMINI_API_KEY`.
  Building against the standard ACP surface insulates us from these agent-side
  churn events.

## 3. Stack

- **Language**: Rust for both client and (Tauri v2) backend.
- **UI shell**: [Tauri v2](https://v2.tauri.app/). Rejected alternatives:
  - Electron: violates the Rust preference, larger footprint.
  - `egui` / `iced`: chat UX (markdown, code-block styling, streaming inserts)
    is painful without a real HTML renderer.
- **Frontend framework**: **Svelte 5** (preferred) or **SolidJS**. Both are
  fine-grained-reactive, so streamed token inserts don't rerender the whole
  chat log. React is discouraged for this app for the same reason.
- **ACP client library**:
  [`agent-client-protocol`](https://crates.io/crates/agent-client-protocol) v1.2+.
  Confirmed working against `claude-agent-acp` 0.59 (see `poc/`).
- **Persistence**: SQLite via [`sqlx`](https://crates.io/crates/sqlx) (compile-time
  checked queries) or [`rusqlite`](https://crates.io/crates/rusqlite) (simpler,
  synchronous — fine because writes are cheap and rare).
- **Async runtime**: `tokio` current-thread on the Rust side. The crate itself
  is executor-agnostic (`futures` + `async-process`), so no lock-in.

## 4. Architecture

```
┌───────────────────────────────────────────────────────────┐
│ Frontend (Svelte in Tauri webview)                        │
│   - Chat log view                                         │
│   - Composer                                              │
│   - Permission dialog                                     │
│   - Agent selector                                        │
└───────────────────────────────────────────────────────────┘
                     ▲                    │
                     │ tauri events       │ tauri commands
                     │ (stream)           ▼
┌───────────────────────────────────────────────────────────┐
│ Rust backend (Tauri app)                                  │
│                                                           │
│   ┌────────────────┐    ┌────────────────────────────┐    │
│   │ SessionManager │───▶│ ACP Client (per session)   │    │
│   │  (SQLite)      │    │  ├─ AcpAgent (spawns child)│    │
│   │                │    │  ├─ notification handler   │    │
│   │                │    │  └─ permission handler     │    │
│   └────────────────┘    └────────────────────────────┘    │
│                                    │                      │
│                                    ▼                      │
│                            child process                  │
│                            (claude-agent-acp,             │
│                             gemini --acp, codex …)        │
└───────────────────────────────────────────────────────────┘
```

### 4.1 Rust backend modules

```
src-tauri/src/
├── main.rs                Tauri app entry point, command wiring
├── acp/
│   ├── mod.rs             re-exports
│   ├── connection.rs      AcpAgent spawn, initialize, session lifecycle
│   ├── notification.rs    SessionNotification → domain event mapping
│   └── permission.rs      permission request/response bridge to the UI
├── domain/
│   ├── mod.rs
│   ├── message.rs         Message, MessageChunk, MessageId
│   ├── session.rs         Session state machine
│   ├── tool_call.rs       ToolCallView, ToolCallStatus
│   └── usage.rs           TokenUsage, Cost
├── storage/
│   ├── mod.rs
│   ├── schema.sql
│   └── repo.rs            SessionRepo, MessageRepo — thin sqlite CRUD
├── config/
│   ├── mod.rs
│   └── agents.rs          user-configured agent commands (name, cmd, args)
└── ui/
    ├── mod.rs
    └── commands.rs        #[tauri::command] surface used by the frontend
```

### 4.2 Frontend layout (Svelte 5)

```
src/
├── App.svelte
├── lib/
│   ├── ChatView.svelte
│   ├── MessageBubble.svelte
│   ├── Composer.svelte
│   ├── PermissionDialog.svelte
│   ├── AgentPicker.svelte
│   └── ipc.ts             wrapper around @tauri-apps/api invoke/event
└── stores/
    ├── session.ts
    ├── messages.ts
    └── permissions.ts
```

## 5. ACP flow (concrete)

Verified in `poc/src/main.rs` against `claude-agent-acp` 0.59.0.

```rust
use agent_client_protocol::{AcpAgent, Agent, Client, ConnectionTo};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    ContentBlock, InitializeRequest, NewSessionRequest, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, TextContent,
};

let agent = AcpAgent::from_str("/absolute/path/to/agent-cli --acp-flags")?;
Client
    .builder()
    .on_receive_notification(
        async move |n: SessionNotification, _cx| { /* stream to UI */ Ok(()) },
        agent_client_protocol::on_receive_notification!(),
    )
    .on_receive_request(
        async move |req: RequestPermissionRequest, responder, _cx| {
            // block until user answers via the UI, then:
            responder.respond(RequestPermissionResponse::new(outcome))
        },
        agent_client_protocol::on_receive_request!(),
    )
    .connect_with(agent, |cx: ConnectionTo<Agent>| async move {
        cx.send_request(InitializeRequest::new(ProtocolVersion::V1))
            .block_task().await?;
        let sess = cx.send_request(NewSessionRequest::new(cwd))
            .block_task().await?;
        cx.send_request(PromptRequest::new(
            sess.session_id,
            vec![ContentBlock::Text(TextContent::new(text))],
        )).block_task().await?;
        Ok(())
    })
    .await?;
```

### Gotchas discovered in the PoC

1. **`on_receive_*!()` closure-wrapper macros are mandatory.** They are a
   workaround for `rust-lang/rust#109417` (async return-type-notation).
   Forgetting them = compile error, not a runtime bug. This dictates
   **MSRV 1.88** (crate uses edition 2024).
2. **`AcpAgent::from_str("cmd args")` resolves `cmd` against the current PATH.**
   Tauri/cargo may not inherit the shell's PATH, so store agent commands as
   **absolute paths** in config (e.g. `/home/…/.npm-global/bin/claude-agent-acp`).
3. **Notification stream is heterogeneous.** A single session emits interleaved
   `AvailableCommandsUpdate`, `UsageUpdate`, `AgentMessageChunk`, tool-call
   updates, and plan updates. The dispatcher must exhaustively match, not
   assume "everything is text".
4. **`AgentMessageChunk.message_id` groups chunks.** Client is responsible for
   merging chunks with the same `message_id` into one bubble in the UI. Do not
   assume one chunk = one message.
5. **`UsageUpdate.cost` arrives twice-ish.** Intermediate updates have
   `cost: None`; the final one has `cost: Some(Cost { amount, currency, .. })`.
   Render the final one as "turn cost" and keep a running sum for "session
   cost".
6. **Dropping the connection kills the child's whole process group on Unix.**
   Good: prevents `npx`-wrapper leaks. Bad: means "just detach and try again
   later" is not a supported use case — treat the connection as owned.
7. **stderr is captured up to 64 KiB** (`STDERR_CAPTURE_LIMIT`), with per-line
   truncation over 8 KiB. Surface this to the user when the child crashes.

### Not yet verified in the PoC

- Permission request end-to-end (needs a prompt that triggers a tool call,
  e.g. "run `ls`"). Wiring is in place but the UI dialog is future work.
- Cancellation (Ctrl-C mid-stream → `RequestCancellation`).
- Session resume across app restarts.

## 6. Data model (v0.2+)

```sql
CREATE TABLE agents (
  id            INTEGER PRIMARY KEY,
  name          TEXT NOT NULL,          -- "Claude Code (ACP)"
  command       TEXT NOT NULL,          -- "/…/claude-agent-acp"
  args_json     TEXT NOT NULL DEFAULT '[]',
  env_json      TEXT NOT NULL DEFAULT '{}'
);

CREATE TABLE sessions (
  id            TEXT PRIMARY KEY,        -- ACP session id
  agent_id      INTEGER NOT NULL REFERENCES agents(id),
  cwd           TEXT NOT NULL,
  title         TEXT,
  created_at    INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL
);

CREATE TABLE messages (
  id            INTEGER PRIMARY KEY,
  session_id    TEXT NOT NULL REFERENCES sessions(id),
  acp_message_id TEXT,                    -- from AgentMessageChunk.message_id
  role          TEXT NOT NULL CHECK (role IN ('user','assistant','system')),
  content_json  TEXT NOT NULL,            -- serialized ContentBlock[]
  created_at    INTEGER NOT NULL
);

CREATE TABLE usage_events (
  id            INTEGER PRIMARY KEY,
  session_id    TEXT NOT NULL REFERENCES sessions(id),
  used_tokens   INTEGER NOT NULL,
  cost_amount   REAL,
  cost_currency TEXT,
  ts            INTEGER NOT NULL
);
```

Rationale for storing raw `content_json`: ACP `ContentBlock` will grow (image,
resource-link, etc.); we don't want to migrate the schema every time.

## 7. Milestones

### v0.1 — "It talks" (target: 1–2 weeks of evenings)

- [x] PoC: Rust CLI, spawn `claude-agent-acp`, one prompt → streamed reply.
      **Done** (`poc/`).
- [ ] Tauri v2 scaffold with Svelte 5.
- [ ] Rust backend port of the PoC: `#[tauri::command] send_prompt`.
- [ ] Frontend: chat view rendering `AgentMessageChunk` merged by
      `message_id`, composer, streaming.
- [ ] Agent selector reading a hard-coded list of two entries
      (Claude via `claude-agent-acp`, Gemini via `gemini --acp`).
- [ ] No persistence, no history — restart-clean.

### v0.2 — "It remembers" (evenings)

- [ ] SQLite schema + migrations (`refinery` or hand-rolled).
- [ ] Session list sidebar, resumable sessions.
- [ ] Permission dialog wired to `on_receive_request` — real user approve/deny
      for tool calls.
- [ ] Tool call view: render `ToolCallStart` / `ToolCallUpdate` / `ToolCallEnd`
      with a collapsible details block.
- [ ] Usage / cost display in the header.
- [ ] User-editable agent list (name, absolute path, args, env).

### v1.0 — "It ships"

- [ ] Linux packaging (AppImage + .deb).
- [ ] macOS packaging (dmg + notarization).
- [ ] Auto-update via `tauri-plugin-updater`.
- [ ] Icon, screenshots, README polish.
- [ ] End-to-end test that runs a real `claude-agent-acp` in CI (Linux only).

## 8. Testing strategy

Per repo policy (TDD):

- **Protocol state machine** (session lifecycle, chunk merging, permission
  bridge): unit-tested with the crate's fake `Client`/`Agent` fixtures.
- **Frontend components**: Vitest + Svelte Testing Library. Composer / bubble
  rendering / streaming inserts.
- **End-to-end**: `cargo test --features e2e` spawns real `claude-agent-acp`
  in CI. Skipped by default because it needs Anthropic credentials.

TDD is applied to executable logic. Prompt strings, UI copy, and CSS are edited
directly — see `Preferences/mistakes.md` 2026-06-05.

## 9. Auth & credentials

**Explicit design decision: the app never touches API keys or OAuth tokens.**

The child agent process inherits the environment of the parent (`acp-desk`),
which inherits from the user's shell. Whatever `claude` or `gemini` finds via
its normal login flow is what our agents get. Rationale:

- Fewer secrets in our codebase.
- Users who already run `claude login` or set `GEMINI_API_KEY` don't have to
  re-enter anything.
- We can't leak a key we don't hold.

Documented caveat: on macOS, GUI apps launched from Finder do **not** inherit
the shell's environment. We must document "launch from terminal for now" as a
v0.1 limitation, then use `launchd`'s user-env plist or a shell wrapper in v1.0.

## 10. Open questions

1. **Frontend framework**: Svelte 5 vs SolidJS. Recommendation: Svelte 5
   (larger ecosystem, better dev tooling). Decide at scaffold time.
2. **Markdown renderer**: `markdown-it` in-webview, or render on the Rust side
   and ship HTML? In-webview is simpler; server-side is safer if we later
   render untrusted content. Default to in-webview for v0.1.
3. **How to handle agent crashes mid-turn**: current answer = surface the
   captured stderr tail and offer "retry / new session". Not designed yet.
4. **Multi-session concurrency**: can two sessions to two different agents run
   in parallel? Cheap to allow (each is its own child process); design assumes
   yes.
5. **Slash-command surface**: ACP's `AvailableCommandsUpdate` exposes what the
   agent knows about (in the PoC, `/agents-sdk`, `/cloudflare`, `/codex:*`, …).
   Should we surface these as UI-level slash commands, or forward `/foo` as
   literal text? Default: forward as text — the agent handles it.

## 11. Non-obvious references

- ACP spec: <https://agentclientprotocol.com/>
- Rust SDK: `github.com/agentclientprotocol/rust-sdk` (was previously
  `zed-industries/agent-client-protocol` — the schema-only crate stayed there).
- Client example that the PoC is based on:
  `src/agent-client-protocol/examples/yolo_one_shot_client.rs` in the SDK repo.
- Claude Code adapter (npm): `@agentclientprotocol/claude-agent-acp`
  (was `@zed-industries/claude-code-acp`).
- Zed's Agent Panel is the reference _client_ implementation — good UX
  inspiration, but do **not** copy code (Zed itself is GPL-family; the ACP crate
  is Apache-2.0).

## 12. Repo policy

- Branch prefix: `feature/…`, `bugfix/…`, `hotfix/…`.
- PRs required; do not auto-merge (see user CLAUDE.md).
- Gemini Code Assist auto-reviews the first push on this repo — expect an
  initial review comment, address before requesting human review.
- Commit messages / PR bodies / code comments in English; chat with the human
  user in Japanese (see user CLAUDE.md).

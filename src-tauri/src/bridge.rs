//! Bridges the Tauri command surface to the ACP session running on its own
//! thread. `run_session` is not `Send` (the ACP connection is
//! thread-affine), so each session gets a dedicated OS thread with a
//! current-thread tokio runtime; prompts go in through a channel, events
//! come back via `app.emit`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use acp_core::{AgentConfig, PermissionBroker, SessionCommand, UiEvent};
use futures::channel::mpsc::{UnboundedSender, unbounded};
use tauri::{AppHandle, Emitter};

/// Event channel name shared with the frontend (src/lib/ipc.ts).
pub const ACP_EVENT: &str = "acp:event";

#[derive(Default)]
pub struct AcpBridge {
    session: Mutex<Option<SessionHandle>>,
    /// Bumped on every start(); a session thread only emits while it is the
    /// newest generation, so a replaced session finishing its last turn
    /// cannot leak events into the next session's UI state.
    generation: Arc<AtomicU64>,
}

struct SessionHandle {
    agent_name: String,
    command_tx: UnboundedSender<SessionCommand>,
    permissions: Arc<PermissionBroker>,
}

impl AcpBridge {
    /// Starts a session with the named agent. The replaced session gets a
    /// best-effort Cancel and its command channel dropped: a mid-turn
    /// session cancels the turn, finishes it, and exits its loop, which
    /// (by ACP crate design) kills the child process group. Without the
    /// Cancel, a turn blocked on a permission dialog would never end and
    /// the old child would leak forever.
    pub fn start(&self, app: AppHandle, config: AgentConfig) {
        let (command_tx, command_rx) = unbounded::<SessionCommand>();
        let agent_name = config.name.clone();
        let permissions = Arc::new(PermissionBroker::default());
        let session_permissions = Arc::clone(&permissions);
        let my_generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let current_generation = Arc::clone(&self.generation);

        std::thread::spawn(move || {
            let emit = move |app: &AppHandle, event: &UiEvent| {
                if current_generation.load(Ordering::SeqCst) == my_generation {
                    emit_event(app, event);
                }
            };
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    emit(&app, &UiEvent::AgentError {
                        message: format!("failed to start async runtime: {error}"),
                    });
                    return;
                }
            };

            let cwd = std::env::current_dir()
                .or_else(|_| std::env::var("HOME").map(Into::into))
                .unwrap_or_else(|_| "/".into());

            let event_app = app.clone();
            let event_emit = emit.clone();
            let result = runtime.block_on(acp_core::run_session(
                config,
                cwd,
                command_rx,
                session_permissions,
                move |event| event_emit(&event_app, &event),
            ));

            if let Err(error) = result {
                // The crate captures up to 64 KiB of child stderr into this
                // error, which is the only diagnostic when the agent crashes.
                emit(&app, &UiEvent::AgentError {
                    message: format!("{error:#}"),
                });
            }
        });

        let replaced = self.session.lock().expect("bridge lock poisoned").replace(
            SessionHandle {
                agent_name,
                command_tx,
                permissions,
            },
        );
        if let Some(old) = replaced {
            let _ = old.command_tx.unbounded_send(SessionCommand::Cancel);
        }
    }

    /// Name of the agent whose session is still alive. A dead session
    /// (agent crashed, loop exited) reports None so callers treat a restart
    /// of the same agent as a fresh start instead of a no-op.
    pub fn current_agent(&self) -> Option<String> {
        self.session
            .lock()
            .expect("bridge lock poisoned")
            .as_ref()
            .filter(|handle| !handle.command_tx.is_closed())
            .map(|handle| handle.agent_name.clone())
    }

    pub fn send_prompt(&self, text: String) -> Result<(), String> {
        let guard = self.session.lock().expect("bridge lock poisoned");
        let handle = guard
            .as_ref()
            .ok_or_else(|| "no active session; start an agent first".to_string())?;
        handle
            .command_tx
            .unbounded_send(SessionCommand::Prompt(text))
            .map_err(|_| "agent session has ended; restart the agent".to_string())
    }

    /// Cancels the in-flight turn. A session that already ended has nothing
    /// to cancel, so a closed channel is success, not an error.
    pub fn cancel_turn(&self) -> Result<(), String> {
        let guard = self.session.lock().expect("bridge lock poisoned");
        let handle = guard
            .as_ref()
            .ok_or_else(|| "no active session; start an agent first".to_string())?;
        let _ = handle.command_tx.unbounded_send(SessionCommand::Cancel);
        Ok(())
    }

    /// Forwards the user's permission decision to the current session's
    /// broker. A replaced session took its broker with it, so answers to a
    /// stale dialog fail with "unknown request" instead of leaking across.
    pub fn respond_permission(&self, request_id: u64, option_id: &str) -> Result<(), String> {
        // Clone the broker out of the session lock so resolve (which takes
        // its own lock and wakes the handler) runs without nesting locks.
        let permissions = {
            let guard = self.session.lock().expect("bridge lock poisoned");
            let handle = guard
                .as_ref()
                .ok_or_else(|| "no active session; start an agent first".to_string())?;
            Arc::clone(&handle.permissions)
        };
        permissions.resolve(request_id, option_id)
    }
}

fn emit_event(app: &AppHandle, event: &UiEvent) {
    if let Err(error) = app.emit(ACP_EVENT, event) {
        eprintln!("failed to emit {ACP_EVENT}: {error}");
    }
}

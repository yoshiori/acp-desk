//! Bridges the Tauri command surface to the ACP session running on its own
//! thread. `run_session` is not `Send` (the ACP connection is
//! thread-affine), so each session gets a dedicated OS thread with a
//! current-thread tokio runtime; prompts go in through a channel, events
//! come back via `app.emit`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use acp_core::{AgentConfig, UiEvent};
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
    prompt_tx: UnboundedSender<String>,
}

impl AcpBridge {
    /// Starts a session with the named agent. Replacing an existing session
    /// drops its prompt channel, which ends the session loop and (by ACP
    /// crate design) kills the child process group.
    pub fn start(&self, app: AppHandle, config: AgentConfig) {
        let (prompt_tx, prompt_rx) = unbounded::<String>();
        let agent_name = config.name.clone();
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
                prompt_rx,
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

        *self.session.lock().expect("bridge lock poisoned") = Some(SessionHandle {
            agent_name,
            prompt_tx,
        });
    }

    /// Name of the agent whose session is still alive. A dead session
    /// (agent crashed, loop exited) reports None so callers treat a restart
    /// of the same agent as a fresh start instead of a no-op.
    pub fn current_agent(&self) -> Option<String> {
        self.session
            .lock()
            .expect("bridge lock poisoned")
            .as_ref()
            .filter(|handle| !handle.prompt_tx.is_closed())
            .map(|handle| handle.agent_name.clone())
    }

    pub fn send_prompt(&self, text: String) -> Result<(), String> {
        let guard = self.session.lock().expect("bridge lock poisoned");
        let handle = guard
            .as_ref()
            .ok_or_else(|| "no active session; start an agent first".to_string())?;
        handle
            .prompt_tx
            .unbounded_send(text)
            .map_err(|_| "agent session has ended; restart the agent".to_string())
    }
}

fn emit_event(app: &AppHandle, event: &UiEvent) {
    if let Err(error) = app.emit(ACP_EVENT, event) {
        eprintln!("failed to emit {ACP_EVENT}: {error}");
    }
}

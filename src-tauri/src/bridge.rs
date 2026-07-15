//! Bridges the Tauri command surface to the ACP session running on its own
//! thread. `run_session` is not `Send` (the ACP connection is
//! thread-affine), so each session gets a dedicated OS thread with a
//! current-thread tokio runtime; prompts go in through a channel, events
//! come back via `app.emit`.

use std::sync::Mutex;

use acp_core::{AgentConfig, UiEvent};
use futures::channel::mpsc::{UnboundedSender, unbounded};
use tauri::{AppHandle, Emitter};

/// Event channel name shared with the frontend (src/lib/ipc.ts).
pub const ACP_EVENT: &str = "acp:event";

#[derive(Default)]
pub struct AcpBridge {
    session: Mutex<Option<SessionHandle>>,
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

        std::thread::spawn(move || {
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
            let result = runtime.block_on(acp_core::run_session(
                config,
                cwd,
                prompt_rx,
                move |event| emit(&event_app, &event),
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

    pub fn current_agent(&self) -> Option<String> {
        self.session
            .lock()
            .expect("bridge lock poisoned")
            .as_ref()
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

fn emit(app: &AppHandle, event: &UiEvent) {
    if let Err(error) = app.emit(ACP_EVENT, event) {
        eprintln!("failed to emit {ACP_EVENT}: {error}");
    }
}

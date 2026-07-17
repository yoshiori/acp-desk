// Reactive glue between the pure reducer (chat-core.ts) and the UI.

import type { UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";

import {
  addSystemMessage,
  addUserMessage,
  applyEvent,
  hydrateFromTranscript,
  initialState,
  restoreUsage,
  settlePermission,
  type AcpEvent,
  type ChatState,
  type PermissionOption,
} from "./chat-core";
import * as ipc from "./ipc";

const WORKING_DIR_KEY = "acp-desk.workingDir";

export class ChatController {
  state = $state<ChatState>(initialState());
  agents = $state<ipc.AgentListing[]>([]);
  selectedAgent = $state<string | null>(null);
  sessions = $state<ipc.SessionSummary[]>([]);
  /** Working directory for new chats; null falls back to the backend's
   * own cwd. Remembered across launches (it is a UI preference). */
  workingDir = $state<string | null>(
    typeof localStorage === "undefined" ? null : localStorage.getItem(WORKING_DIR_KEY),
  );

  #unlisten: UnlistenFn | null = null;
  #disposed = false;
  /** True while a resume/new-chat transition is in flight. Overlapping
   * transitions may finish out of order, leaving the view on one session
   * while the backend runs another; later clicks are dropped instead. */
  #switching = false;

  async init(): Promise<void> {
    try {
      const unlisten = await ipc.onAcpEvent((event) => this.#onEvent(event));
      // dispose() may have run while the listener registration was in flight
      // (component unmounted before init resolved).
      if (this.#disposed) {
        unlisten();
        return;
      }
      this.#unlisten = unlisten;
      void this.refreshSessions();
      this.agents = await ipc.listAgents();
      const first = this.agents.find((agent) => agent.available);
      if (first) {
        await this.selectAgent(first.name);
      } else {
        addSystemMessage(
          this.state,
          "No ACP agent found on PATH. Install @agentclientprotocol/claude-agent-acp " +
            "or @google/gemini-cli and launch acp-desk from a terminal.",
        );
      }
    } catch (error) {
      addSystemMessage(this.state, `Failed to initialize: ${error}`);
    }
  }

  #onEvent(event: AcpEvent): void {
    applyEvent(this.state, event);
    // These are the moments a session row appears or changes (registration,
    // title from the first prompt, updated_at); keep the sidebar in step.
    if (event.type === "session_ready" || event.type === "turn_ended") {
      void this.refreshSessions();
    }
  }

  async refreshSessions(): Promise<void> {
    try {
      this.sessions = await ipc.listSessions();
    } catch (error) {
      // The sidebar is best-effort; a failed refresh keeps the last list.
      console.error("failed to refresh sessions", error);
    }
  }

  /** Restores a stored conversation: transcript from the local database,
   * live context via session/load on a fresh agent process. */
  async resumeSession(summary: ipc.SessionSummary): Promise<void> {
    if (this.#switching || summary.id === this.state.sessionId) return;
    this.#switching = true;
    try {
      const [transcript, usage] = await Promise.all([
        ipc.loadTranscript(summary.id),
        ipc.loadUsage(summary.id),
      ]);
      await ipc.resumeSession(summary.id);
      this.selectedAgent = summary.agentName;
      this.state = hydrateFromTranscript(transcript);
      // The backend confirms with session_ready once the load finishes; set
      // the id now so a second click is a no-op instead of a reload.
      this.state.sessionId = summary.id;
      if (usage) restoreUsage(this.state, usage);
      addSystemMessage(this.state, `Resumed session with ${summary.agentName}.`);
    } catch (error) {
      // The view must only switch after the resume succeeded: a failure
      // happens before the backend replaces the session, so the previous
      // session is still the one prompts go to — showing the restored
      // transcript instead would route input to the wrong agent.
      addSystemMessage(
        this.state,
        `Failed to resume "${summary.title ?? summary.agentName}": ${error}`,
      );
    } finally {
      this.#switching = false;
    }
  }

  /** Saves an agent config; returns an error message for the form, or null. */
  async saveAgent(spec: ipc.AgentSpec): Promise<string | null> {
    try {
      await ipc.saveAgent(spec);
      this.agents = await ipc.listAgents();
      return null;
    } catch (error) {
      return String(error);
    }
  }

  async deleteAgent(id: number): Promise<void> {
    try {
      await ipc.deleteAgent(id);
      this.agents = await ipc.listAgents();
    } catch (error) {
      addSystemMessage(this.state, `Failed to delete agent: ${error}`);
    }
  }

  /** Starts a fresh session with the selected agent even if one is alive. */
  async newChat(): Promise<void> {
    if (this.#switching || !this.selectedAgent) return;
    this.#switching = true;
    try {
      await ipc.startSession(this.selectedAgent, true, this.workingDir);
      this.state = initialState();
      addSystemMessage(this.state, `Starting ${this.selectedAgent}…`);
    } catch (error) {
      // Same as resumeSession: on failure the previous session is still
      // live, so its chat stays on screen and carries the error.
      addSystemMessage(this.state, `Failed to start ${this.selectedAgent}: ${error}`);
    } finally {
      this.#switching = false;
    }
  }

  /** Opens the native folder picker and applies the choice to future chats. */
  async pickWorkingDir(): Promise<void> {
    const dir = await openDialog({ directory: true, defaultPath: this.workingDir ?? undefined });
    if (typeof dir !== "string") return;
    this.workingDir = dir;
    localStorage.setItem(WORKING_DIR_KEY, dir);
  }

  async selectAgent(name: string): Promise<void> {
    try {
      const started = await ipc.startSession(name, false, this.workingDir);
      this.selectedAgent = name;
      // Only a fresh session gets a fresh chat. If the agent was already
      // running, the backend may hold open permission requests; wiping the
      // state here would drop their cards and leave the turn blocked.
      if (started) {
        this.state = initialState();
        addSystemMessage(this.state, `Starting ${name}…`);
      }
    } catch (error) {
      addSystemMessage(this.state, `Failed to start ${name}: ${error}`);
    }
  }

  async send(text: string): Promise<void> {
    const trimmed = text.trim();
    if (!trimmed || this.state.busy) return;
    addUserMessage(this.state, trimmed);
    try {
      await ipc.sendPrompt(trimmed);
    } catch (error) {
      this.state.busy = false;
      addSystemMessage(this.state, `Failed to send: ${error}`);
    }
  }

  async cancel(): Promise<void> {
    if (!this.state.busy) return;
    try {
      await ipc.cancelTurn();
      // busy stays true until the agent acknowledges with a turn_ended
      // (stop_reason "cancelled") — cancellation is asynchronous.
    } catch (error) {
      addSystemMessage(this.state, `Failed to cancel: ${error}`);
    }
  }

  async respondPermission(requestId: number, option: PermissionOption): Promise<void> {
    try {
      await ipc.respondPermission(requestId, option.optionId);
      settlePermission(this.state, requestId, option.name);
    } catch (error) {
      // Settle on failure too: the backend no longer accepts an answer for
      // this request, so leaving the card would keep a dead, disabled UI.
      settlePermission(this.state, requestId, `failed (${error})`);
    }
  }

  dispose(): void {
    this.#disposed = true;
    this.#unlisten?.();
    this.#unlisten = null;
  }
}

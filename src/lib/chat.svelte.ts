// Reactive glue between the pure reducer (chat-core.ts) and the UI.

import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  addSystemMessage,
  addUserMessage,
  applyEvent,
  initialState,
  settlePermission,
  type ChatState,
  type PermissionOption,
} from "./chat-core";
import * as ipc from "./ipc";

export class ChatController {
  state = $state<ChatState>(initialState());
  agents = $state<ipc.AgentListing[]>([]);
  selectedAgent = $state<string | null>(null);

  #unlisten: UnlistenFn | null = null;
  #disposed = false;

  async init(): Promise<void> {
    try {
      const unlisten = await ipc.onAcpEvent((event) => applyEvent(this.state, event));
      // dispose() may have run while the listener registration was in flight
      // (component unmounted before init resolved).
      if (this.#disposed) {
        unlisten();
        return;
      }
      this.#unlisten = unlisten;
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

  async selectAgent(name: string): Promise<void> {
    try {
      const started = await ipc.startSession(name);
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

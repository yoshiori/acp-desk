// Reactive glue between the pure reducer (chat-core.ts) and the UI.

import type { UnlistenFn } from "@tauri-apps/api/event";

import {
  addSystemMessage,
  addUserMessage,
  applyEvent,
  initialState,
  type ChatState,
} from "./chat-core";
import * as ipc from "./ipc";

export class ChatController {
  state = $state<ChatState>(initialState());
  agents = $state<ipc.AgentListing[]>([]);
  selectedAgent = $state<string | null>(null);

  #unlisten: UnlistenFn | null = null;

  async init(): Promise<void> {
    this.#unlisten = await ipc.onAcpEvent((event) => applyEvent(this.state, event));
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
  }

  async selectAgent(name: string): Promise<void> {
    try {
      await ipc.startSession(name);
      this.selectedAgent = name;
      this.state = initialState();
      addSystemMessage(this.state, `Starting ${name}…`);
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

  dispose(): void {
    this.#unlisten?.();
    this.#unlisten = null;
  }
}

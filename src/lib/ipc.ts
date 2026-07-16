// Thin wrapper around the Tauri IPC surface (src-tauri/src/lib.rs).

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AcpEvent } from "./chat-core";

export interface AgentListing {
  name: string;
  available: boolean;
}

export function listAgents(): Promise<AgentListing[]> {
  return invoke<AgentListing[]>("list_agents");
}

/** Resolves to true when a fresh session was started, false when the
 * agent's existing session is still alive and was left untouched. */
export function startSession(agentName: string): Promise<boolean> {
  return invoke<boolean>("start_session", { agentName });
}

export function sendPrompt(text: string): Promise<void> {
  return invoke<void>("send_prompt", { text });
}

export function respondPermission(requestId: number, optionId: string): Promise<void> {
  return invoke<void>("respond_permission", { requestId, optionId });
}

/** Event channel name shared with the backend (bridge.rs ACP_EVENT). */
const ACP_EVENT = "acp:event";

export function onAcpEvent(handler: (event: AcpEvent) => void): Promise<UnlistenFn> {
  return listen<AcpEvent>(ACP_EVENT, (event) => handler(event.payload));
}

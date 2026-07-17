// Thin wrapper around the Tauri IPC surface (src-tauri/src/lib.rs).

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import type { AcpEvent, StoredUsage, TranscriptRow } from "./chat-core";

export interface AgentListing {
  name: string;
  available: boolean;
}

/** Mirror of acp-core's SessionRow serde shape (camelCase fields). */
export interface SessionSummary {
  id: string;
  agentName: string;
  cwd: string;
  title: string | null;
  /** Unix seconds. */
  createdAt: number;
  updatedAt: number;
}

export function listAgents(): Promise<AgentListing[]> {
  return invoke<AgentListing[]>("list_agents");
}

/** Resolves to true when a fresh session was started, false when the
 * agent's existing session is still alive and was left untouched.
 * `force` starts fresh even then (the sidebar's "New chat"). */
export function startSession(agentName: string, force = false): Promise<boolean> {
  return invoke<boolean>("start_session", { agentName, force });
}

export function resumeSession(sessionId: string): Promise<void> {
  return invoke<void>("resume_session", { sessionId });
}

export function listSessions(): Promise<SessionSummary[]> {
  return invoke<SessionSummary[]>("list_sessions");
}

export function loadTranscript(sessionId: string): Promise<TranscriptRow[]> {
  return invoke<TranscriptRow[]>("load_transcript", { sessionId });
}

/** Latest persisted usage snapshot of a session; null when none exists. */
export function loadUsage(sessionId: string): Promise<StoredUsage | null> {
  return invoke<StoredUsage | null>("load_usage", { sessionId });
}

export function sendPrompt(text: string): Promise<void> {
  return invoke<void>("send_prompt", { text });
}

export function respondPermission(requestId: number, optionId: string): Promise<void> {
  return invoke<void>("respond_permission", { requestId, optionId });
}

export function cancelTurn(): Promise<void> {
  return invoke<void>("cancel_turn");
}

/** Event channel name shared with the backend (bridge.rs ACP_EVENT). */
const ACP_EVENT = "acp:event";

export function onAcpEvent(handler: (event: AcpEvent) => void): Promise<UnlistenFn> {
  return listen<AcpEvent>(ACP_EVENT, (event) => handler(event.payload));
}

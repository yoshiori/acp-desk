// Framework-free chat state logic. The Svelte store wraps this reducer, so
// everything that has behavior (chunk merging, tool-call status, usage) is
// testable with plain vitest and no compiler transforms.

/** Mirror of acp-core's UiEvent serde shape (tag "type", camelCase fields). */
export type AcpEvent =
  | { type: "agent_message_chunk"; messageId: string | null; text: string }
  | { type: "agent_thought_chunk"; messageId: string | null; text: string }
  | { type: "tool_call"; toolCallId: string; title: string; kind: string; status: string }
  | { type: "tool_call_update"; toolCallId: string; title: string | null; status: string | null }
  | { type: "available_commands"; commands: { name: string; description: string }[] }
  | {
      type: "usage";
      usedTokens: number;
      contextSize: number;
      costAmount: number | null;
      costCurrency: string | null;
    }
  | { type: "permission_decided"; toolTitle: string; decision: string }
  | { type: "session_ready"; sessionId: string }
  | { type: "turn_ended"; stopReason: string }
  | { type: "agent_error"; message: string };

export type ChatRole = "user" | "assistant" | "thought" | "tool" | "system";

export interface ChatMessage {
  key: number;
  role: ChatRole;
  text: string;
  /** ACP message id for chunk grouping; null for anonymous chunks. */
  messageId: string | null;
  /** Set on tool entries so later updates can find them. */
  toolCallId?: string;
  status?: string;
}

export interface Usage {
  usedTokens: number;
  contextSize: number;
  /** Cumulative session cost; the agent sends it with the final usage update. */
  costAmount: number | null;
  costCurrency: string | null;
}

export interface ChatState {
  messages: ChatMessage[];
  sessionId: string | null;
  /** True between sending a prompt and the matching turn_ended. */
  busy: boolean;
  usage: Usage | null;
  /** Whether an anonymous streaming message may still receive chunks. */
  streaming: boolean;
  nextKey: number;
}

export function initialState(): ChatState {
  return {
    messages: [],
    sessionId: null,
    busy: false,
    usage: null,
    streaming: false,
    nextKey: 0,
  };
}

export function addUserMessage(state: ChatState, text: string): void {
  pushMessage(state, { role: "user", text, messageId: null });
  state.busy = true;
}

export function addSystemMessage(state: ChatState, text: string): void {
  pushMessage(state, { role: "system", text, messageId: null });
}

/**
 * Applies one backend event, mutating the state in place.
 *
 * Chunk merging follows the ACP contract: chunks sharing a messageId are one
 * message; a messageId change starts a new one. Anonymous (null id) chunks
 * merge into the current streaming message of the same role, and turn_ended
 * closes it so the next turn starts fresh.
 */
export function applyEvent(state: ChatState, event: AcpEvent): void {
  switch (event.type) {
    case "agent_message_chunk":
      appendChunk(state, "assistant", event.messageId, event.text);
      break;
    case "agent_thought_chunk":
      appendChunk(state, "thought", event.messageId, event.text);
      break;
    case "tool_call":
      pushMessage(state, {
        role: "tool",
        text: event.title,
        messageId: null,
        toolCallId: event.toolCallId,
        status: event.status,
      });
      break;
    case "tool_call_update": {
      const entry = state.messages.findLast(
        (message) => message.toolCallId === event.toolCallId,
      );
      if (entry) {
        if (event.title !== null) entry.text = event.title;
        if (event.status !== null) entry.status = event.status;
      }
      break;
    }
    case "usage":
      state.usage = {
        usedTokens: event.usedTokens,
        contextSize: event.contextSize,
        costAmount: event.costAmount ?? state.usage?.costAmount ?? null,
        costCurrency: event.costCurrency ?? state.usage?.costCurrency ?? null,
      };
      break;
    case "permission_decided":
      addSystemMessage(
        state,
        `Tool call "${event.toolTitle}" was auto-rejected (${event.decision}). ` +
          "The permission dialog arrives in v0.2.",
      );
      break;
    case "session_ready":
      state.sessionId = event.sessionId;
      break;
    case "turn_ended":
      state.busy = false;
      state.streaming = false;
      break;
    case "agent_error":
      state.busy = false;
      addSystemMessage(state, `Agent error: ${event.message}`);
      break;
    case "available_commands":
      // v0.1 has no slash-command UI; slash input is forwarded as plain text.
      break;
  }
}

function pushMessage(state: ChatState, message: Omit<ChatMessage, "key">): ChatMessage {
  const entry = { ...message, key: state.nextKey++ };
  state.messages.push(entry);
  return entry;
}

function appendChunk(
  state: ChatState,
  role: "assistant" | "thought",
  messageId: string | null,
  text: string,
): void {
  const last = state.messages.at(-1);
  const continuesById =
    last?.role === role && messageId !== null && last.messageId === messageId;
  const continuesAnonymously =
    last?.role === role && messageId === null && last.messageId === null && state.streaming;
  if (last && (continuesById || continuesAnonymously)) {
    last.text += text;
  } else {
    pushMessage(state, { role, text, messageId });
  }
  state.streaming = true;
}

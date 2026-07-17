// Framework-free chat state logic. The Svelte store wraps this reducer, so
// everything that has behavior (chunk merging, tool-call status, usage) is
// testable with plain vitest and no compiler transforms.

/** Mirror of acp-core's DiffInfo / ToolCallDetail serde shapes. */
export interface DiffInfo {
  path: string;
  oldText: string | null;
  newText: string;
}

export interface ToolCallDetail {
  contentText: string | null;
  diffs: DiffInfo[];
  rawInputJson: string | null;
  rawOutputJson: string | null;
  locations: string[];
}

/** Mirror of acp-core's UiEvent serde shape (tag "type", camelCase fields). */
export type AcpEvent =
  | { type: "agent_message_chunk"; messageId: string | null; text: string }
  | { type: "agent_thought_chunk"; messageId: string | null; text: string }
  | {
      type: "tool_call";
      toolCallId: string;
      title: string;
      kind: string;
      status: string;
      detail: ToolCallDetail;
    }
  | {
      type: "tool_call_update";
      toolCallId: string;
      title: string | null;
      status: string | null;
      /** Detail fields follow ACP update semantics: non-null replaces. */
      contentText: string | null;
      diffs: DiffInfo[] | null;
      rawInputJson: string | null;
      rawOutputJson: string | null;
      locations: string[] | null;
    }
  | { type: "available_commands"; commands: { name: string; description: string }[] }
  | {
      type: "usage";
      usedTokens: number;
      contextSize: number;
      costAmount: number | null;
      costCurrency: string | null;
    }
  | {
      type: "permission_requested";
      requestId: number;
      toolTitle: string;
      options: PermissionOption[];
    }
  | { type: "session_ready"; sessionId: string }
  | { type: "user_message"; text: string }
  | { type: "turn_ended"; stopReason: string }
  | { type: "agent_error"; message: string };

export type ChatRole = "user" | "assistant" | "thought" | "tool" | "system";

/** `kind` is the ACP wire name (allow_once, reject_always, …) for styling. */
export interface PermissionOption {
  optionId: string;
  name: string;
  kind: string;
}

export interface PermissionRequest {
  requestId: number;
  toolTitle: string;
  options: PermissionOption[];
}

export interface ChatMessage {
  key: number;
  role: ChatRole;
  text: string;
  /** ACP message id for chunk grouping; null for anonymous chunks. */
  messageId: string | null;
  /** Set on tool entries so later updates can find them. */
  toolCallId?: string;
  status?: string;
  /** Collapsible tool-call detail; absent when the call carried none. */
  detail?: ToolCallDetail;
}

export interface Usage {
  usedTokens: number;
  contextSize: number;
  /** Cumulative session cost; the agent sends it with the final usage update. */
  costAmount: number | null;
  costCurrency: string | null;
  /** Cost of the most recent completed turn (delta of the cumulative cost).
   * Null on the first turn and whenever no cost update arrived. */
  lastTurnCost: number | null;
}

export interface ChatState {
  messages: ChatMessage[];
  sessionId: string | null;
  /** True between sending a prompt and the matching turn_ended. */
  busy: boolean;
  usage: Usage | null;
  /** Whether an anonymous streaming message may still receive chunks. */
  streaming: boolean;
  /** Cumulative cost at the end of the previous turn; null before the
   * first completed turn with a known cost. Feeds lastTurnCost. */
  costBaseline: number | null;
  /** Tool-call permission requests awaiting the user's decision. */
  pendingPermissions: PermissionRequest[];
  nextKey: number;
}

export function initialState(): ChatState {
  return {
    messages: [],
    sessionId: null,
    busy: false,
    usage: null,
    streaming: false,
    costBaseline: null,
    pendingPermissions: [],
    nextKey: 0,
  };
}

/** Mirror of acp-core's MessageRow serde shape (camelCase fields). */
export interface TranscriptRow {
  role: string;
  /** JSON array of content blocks as stored by the backend. */
  contentJson: string;
  acpMessageId: string | null;
  status: string | null;
}

/**
 * Rebuilds an idle ChatState from a stored transcript (the resume flow).
 * The database rows are already chunk-merged, so this is a plain mapping;
 * usage is left null because only the agent knows the restored context size.
 */
export function hydrateFromTranscript(rows: TranscriptRow[]): ChatState {
  const state = initialState();
  for (const row of rows) {
    const blocks = parseBlocks(row.contentJson);
    pushMessage(state, {
      role: isChatRole(row.role) ? row.role : "system",
      text: blocks === null ? "[unreadable message]" : textOfBlocks(blocks),
      messageId: row.acpMessageId,
      status: row.status ?? undefined,
      detail: blocks === null ? undefined : detailOfBlocks(blocks),
    });
  }
  return state;
}

function isChatRole(role: string): role is ChatRole {
  return ["user", "assistant", "thought", "tool", "system"].includes(role);
}

function parseBlocks(contentJson: string): unknown[] | null {
  try {
    const blocks: unknown = JSON.parse(contentJson);
    return Array.isArray(blocks) ? blocks : null;
  } catch {
    return null;
  }
}

function textOfBlocks(blocks: unknown[]): string {
  return blocks
    .filter(
      (block): block is { text: string } =>
        typeof block === "object" &&
        block !== null &&
        "text" in block &&
        typeof block.text === "string",
    )
    .map((block) => block.text)
    .join("");
}

function detailOfBlocks(blocks: unknown[]): ToolCallDetail | undefined {
  const block = blocks.find(
    (candidate): candidate is { type: string; detail: ToolCallDetail } =>
      typeof candidate === "object" &&
      candidate !== null &&
      "type" in candidate &&
      candidate.type === "tool_detail" &&
      "detail" in candidate,
  );
  return block?.detail;
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
        detail: event.detail,
      });
      break;
    case "tool_call_update": {
      const entry = state.messages.findLast(
        (message) => message.toolCallId === event.toolCallId,
      );
      if (entry) {
        if (event.title !== null) entry.text = event.title;
        if (event.status !== null) entry.status = event.status;
        // Loose != null: the wire contract sends null for "unchanged", but a
        // field omitted entirely (undefined) must mean the same thing.
        const hasDetailUpdate =
          event.contentText != null ||
          event.diffs != null ||
          event.rawInputJson != null ||
          event.rawOutputJson != null ||
          event.locations != null;
        if (hasDetailUpdate) {
          entry.detail ??= {
            contentText: null,
            diffs: [],
            rawInputJson: null,
            rawOutputJson: null,
            locations: [],
          };
          if (event.contentText != null) entry.detail.contentText = event.contentText;
          if (event.diffs != null) entry.detail.diffs = event.diffs;
          if (event.rawInputJson != null) entry.detail.rawInputJson = event.rawInputJson;
          if (event.rawOutputJson != null) entry.detail.rawOutputJson = event.rawOutputJson;
          if (event.locations != null) entry.detail.locations = event.locations;
        }
      }
      break;
    }
    case "usage":
      state.usage = {
        usedTokens: event.usedTokens,
        contextSize: event.contextSize,
        costAmount: event.costAmount ?? state.usage?.costAmount ?? null,
        costCurrency: event.costCurrency ?? state.usage?.costCurrency ?? null,
        lastTurnCost: state.usage?.lastTurnCost ?? null,
      };
      break;
    case "permission_requested":
      state.pendingPermissions.push({
        requestId: event.requestId,
        toolTitle: event.toolTitle,
        options: event.options,
      });
      break;
    case "session_ready":
      state.sessionId = event.sessionId;
      break;
    // Both turn_ended and agent_error mean the backend no longer waits for
    // an answer (the pending oneshot was consumed or dropped), so lingering
    // cards would only produce "unknown request" errors when clicked.
    case "turn_ended":
      state.busy = false;
      state.streaming = false;
      state.pendingPermissions = [];
      settleTurnCost(state);
      if (event.stopReason === "cancelled") {
        addSystemMessage(state, "Turn cancelled.");
      }
      break;
    case "agent_error":
      state.busy = false;
      state.pendingPermissions = [];
      addSystemMessage(state, `Agent error: ${event.message}`);
      break;
    case "available_commands":
      // v0.1 has no slash-command UI; slash input is forwarded as plain text.
      break;
    case "user_message":
      // Backend echo for the transcript recorder; the UI already showed its
      // own copy when the prompt was sent.
      break;
  }
}

/** Mirror of acp-core's UsageRow serde shape (camelCase fields). */
export interface StoredUsage {
  usedTokens: number;
  contextSize: number;
  costAmount: number | null;
  costCurrency: string | null;
}

/** Seeds the header from a persisted usage snapshot (the resume flow). The
 * baseline is set so the first turn after resume gets a correct delta. */
export function restoreUsage(state: ChatState, stored: StoredUsage): void {
  state.usage = { ...stored, lastTurnCost: null };
  state.costBaseline = stored.costAmount;
}

/** Closes a turn's cost accounting: the delta against the previous turn's
 * cumulative cost becomes lastTurnCost. An unchanged (or never-reported)
 * cumulative cost means no cost update arrived this turn — no delta. */
function settleTurnCost(state: ChatState): void {
  const cost = state.usage?.costAmount ?? null;
  if (cost === null || !state.usage) return;
  state.usage.lastTurnCost =
    state.costBaseline !== null && cost !== state.costBaseline
      ? cost - state.costBaseline
      : null;
  state.costBaseline = cost;
}

/**
 * Records a decided permission request: removes it from the pending queue
 * and leaves a system message with the outcome. Called after the backend
 * accepted the answer, so an unknown id (stale card) is a no-op.
 */
export function settlePermission(
  state: ChatState,
  requestId: number,
  decision: string,
): void {
  const index = state.pendingPermissions.findIndex((p) => p.requestId === requestId);
  if (index === -1) return;
  const [request] = state.pendingPermissions.splice(index, 1);
  addSystemMessage(state, `Tool call "${request.toolTitle}": ${decision}`);
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

import { describe, expect, it } from "vitest";

import {
  addUserMessage,
  applyEvent,
  initialState,
  settlePermission,
  type AcpEvent,
  type ChatState,
} from "./chat-core";

function chunk(text: string, messageId: string | null = null): AcpEvent {
  return { type: "agent_message_chunk", messageId, text };
}

function applyAll(state: ChatState, events: AcpEvent[]): void {
  for (const event of events) applyEvent(state, event);
}

describe("chunk merging by messageId", () => {
  it("merges chunks sharing a messageId into one message", () => {
    const state = initialState();
    applyAll(state, [chunk("Hel", "m1"), chunk("lo", "m1")]);
    expect(state.messages).toHaveLength(1);
    expect(state.messages[0].text).toBe("Hello");
  });

  it("starts a new message when the messageId changes", () => {
    const state = initialState();
    applyAll(state, [chunk("first", "m1"), chunk("second", "m2")]);
    expect(state.messages.map((m) => m.text)).toEqual(["first", "second"]);
  });

  it("merges contiguous anonymous chunks into one message", () => {
    const state = initialState();
    applyAll(state, [chunk("a"), chunk("b"), chunk("c")]);
    expect(state.messages).toHaveLength(1);
    expect(state.messages[0].text).toBe("abc");
  });

  it("starts a fresh anonymous message after turn_ended", () => {
    const state = initialState();
    applyAll(state, [
      chunk("turn one"),
      { type: "turn_ended", stopReason: "end_turn" },
      chunk("turn two"),
    ]);
    expect(state.messages.map((m) => m.text)).toEqual(["turn one", "turn two"]);
  });

  it("does not merge assistant chunks into a thought message", () => {
    const state = initialState();
    applyAll(state, [
      { type: "agent_thought_chunk", messageId: null, text: "hmm" },
      chunk("answer"),
    ]);
    expect(state.messages.map((m) => m.role)).toEqual(["thought", "assistant"]);
  });

  it("keeps user and assistant messages interleaved in order", () => {
    const state = initialState();
    addUserMessage(state, "hi");
    applyAll(state, [chunk("hello", "m1")]);
    expect(state.messages.map((m) => m.role)).toEqual(["user", "assistant"]);
  });
});

describe("tool calls", () => {
  it("adds a tool entry and updates its status in place", () => {
    const state = initialState();
    applyAll(state, [
      { type: "tool_call", toolCallId: "tc1", title: "Run ls", kind: "execute", status: "pending" },
      { type: "tool_call_update", toolCallId: "tc1", title: null, status: "completed" },
    ]);
    expect(state.messages).toHaveLength(1);
    expect(state.messages[0].status).toBe("completed");
    expect(state.messages[0].text).toBe("Run ls");
  });
});

describe("usage", () => {
  it("keeps the last known cost when a mid-turn update has none", () => {
    const state = initialState();
    applyAll(state, [
      { type: "usage", usedTokens: 100, contextSize: 200_000, costAmount: 0.1, costCurrency: "USD" },
      { type: "usage", usedTokens: 150, contextSize: 200_000, costAmount: null, costCurrency: null },
    ]);
    expect(state.usage).toEqual({
      usedTokens: 150,
      contextSize: 200_000,
      costAmount: 0.1,
      costCurrency: "USD",
    });
  });
});

describe("turn lifecycle", () => {
  it("busy flips on with a user message and off at turn_ended", () => {
    const state = initialState();
    addUserMessage(state, "hi");
    expect(state.busy).toBe(true);
    applyEvent(state, { type: "turn_ended", stopReason: "end_turn" });
    expect(state.busy).toBe(false);
  });

  it("agent_error clears busy and surfaces a system message", () => {
    const state = initialState();
    addUserMessage(state, "hi");
    applyEvent(state, { type: "agent_error", message: "child crashed" });
    expect(state.busy).toBe(false);
    expect(state.messages.at(-1)?.role).toBe("system");
    expect(state.messages.at(-1)?.text).toContain("child crashed");
  });

  it("a cancelled turn surfaces a system message", () => {
    const state = initialState();
    addUserMessage(state, "hi");
    applyEvent(state, { type: "turn_ended", stopReason: "cancelled" });
    expect(state.busy).toBe(false);
    expect(state.messages.at(-1)?.role).toBe("system");
    expect(state.messages.at(-1)?.text).toContain("cancelled");
  });

  it("a normally ended turn adds no system message", () => {
    const state = initialState();
    addUserMessage(state, "hi");
    applyEvent(state, { type: "turn_ended", stopReason: "end_turn" });
    expect(state.messages.at(-1)?.role).toBe("user");
  });

  it("session_ready records the session id", () => {
    const state = initialState();
    applyEvent(state, { type: "session_ready", sessionId: "s1" });
    expect(state.sessionId).toBe("s1");
  });

});

function permissionRequest(requestId: number, toolTitle = "Run ls"): AcpEvent {
  return {
    type: "permission_requested",
    requestId,
    toolTitle,
    options: [
      { optionId: "allow", name: "Allow", kind: "allow_once" },
      { optionId: "reject", name: "Reject", kind: "reject_once" },
    ],
  };
}

describe("permission requests", () => {
  it("queues incoming requests in arrival order", () => {
    const state = initialState();
    applyAll(state, [permissionRequest(1, "Run ls"), permissionRequest(2, "Edit file")]);
    expect(state.pendingPermissions.map((p) => p.requestId)).toEqual([1, 2]);
    expect(state.pendingPermissions[0].toolTitle).toBe("Run ls");
    expect(state.pendingPermissions[0].options[0].optionId).toBe("allow");
  });

  it("settling removes the request and records the decision as a system message", () => {
    const state = initialState();
    applyEvent(state, permissionRequest(1, "Run ls"));
    settlePermission(state, 1, "Allow");
    expect(state.pendingPermissions).toHaveLength(0);
    expect(state.messages.at(-1)?.role).toBe("system");
    expect(state.messages.at(-1)?.text).toContain("Run ls");
    expect(state.messages.at(-1)?.text).toContain("Allow");
  });

  it("settling an unknown request changes nothing", () => {
    const state = initialState();
    applyEvent(state, permissionRequest(1));
    settlePermission(state, 99, "Allow");
    expect(state.pendingPermissions).toHaveLength(1);
    expect(state.messages).toHaveLength(0);
  });

  it("agent_error clears pending requests", () => {
    const state = initialState();
    applyAll(state, [permissionRequest(1), { type: "agent_error", message: "child crashed" }]);
    expect(state.pendingPermissions).toHaveLength(0);
  });

  it("turn_ended clears pending requests", () => {
    const state = initialState();
    applyAll(state, [permissionRequest(1), { type: "turn_ended", stopReason: "cancelled" }]);
    expect(state.pendingPermissions).toHaveLength(0);
  });
});

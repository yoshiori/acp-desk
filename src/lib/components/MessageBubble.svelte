<script lang="ts">
  import type { ChatMessage } from "$lib/chat-core";

  let { message }: { message: ChatMessage } = $props();
</script>

{#if message.role === "tool"}
  <div class="tool" data-status={message.status}>
    <span class="tool-status">{message.status ?? "pending"}</span>
    <span class="tool-title">{message.text}</span>
  </div>
{:else}
  <div class="bubble {message.role}">
    {message.text}
  </div>
{/if}

<style>
  .bubble {
    max-width: 46em;
    padding: 0.6em 0.9em;
    border-radius: 10px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    line-height: 1.5;
  }
  .user {
    align-self: flex-end;
    background: #2563eb;
    color: #fff;
  }
  .assistant {
    align-self: flex-start;
    background: var(--surface);
  }
  .thought {
    align-self: flex-start;
    color: var(--muted);
    font-size: 0.85em;
    font-style: italic;
    background: transparent;
    border-left: 3px solid var(--border);
    border-radius: 0;
  }
  .system {
    align-self: center;
    color: var(--muted);
    font-size: 0.85em;
    background: transparent;
  }
  .tool {
    align-self: flex-start;
    display: flex;
    gap: 0.6em;
    align-items: baseline;
    font-size: 0.85em;
    color: var(--muted);
    font-family: var(--mono);
  }
  .tool-status {
    padding: 0.1em 0.5em;
    border: 1px solid var(--border);
    border-radius: 999px;
  }
  .tool[data-status="completed"] .tool-status {
    border-color: #16a34a;
    color: #16a34a;
  }
  .tool[data-status="failed"] .tool-status {
    border-color: #dc2626;
    color: #dc2626;
  }
</style>

<script lang="ts">
  import { onDestroy, onMount } from "svelte";

  import { ChatController } from "$lib/chat.svelte";
  import Composer from "$lib/components/Composer.svelte";
  import MessageBubble from "$lib/components/MessageBubble.svelte";
  import PermissionCard from "$lib/components/PermissionCard.svelte";

  const chat = new ChatController();

  let log: HTMLElement | undefined = $state();

  onMount(() => chat.init());
  onDestroy(() => chat.dispose());

  // Follow the stream: whenever content grows, keep the log pinned to the
  // bottom (streaming inserts land several times per second).
  $effect(() => {
    void chat.state.messages.length;
    void chat.state.messages.at(-1)?.text;
    void chat.state.pendingPermissions.length;
    log?.scrollTo({ top: log.scrollHeight });
  });

  const usageLabel = $derived.by(() => {
    const usage = chat.state.usage;
    if (!usage) return "";
    const tokens = `${usage.usedTokens.toLocaleString()} / ${usage.contextSize.toLocaleString()} tokens`;
    return usage.costAmount !== null
      ? `${tokens} · ${usage.costAmount.toFixed(3)} ${usage.costCurrency ?? ""}`
      : tokens;
  });
</script>

<div class="app">
  <header>
    <h1>acp-desk</h1>
    <select
      aria-label="Agent"
      value={chat.selectedAgent}
      onchange={(event) => chat.selectAgent(event.currentTarget.value)}
    >
      {#each chat.agents as agent (agent.name)}
        <option value={agent.name} disabled={!agent.available}>
          {agent.name}{agent.available ? "" : " (not found)"}
        </option>
      {/each}
    </select>
    <span class="usage">{usageLabel}</span>
  </header>

  <main bind:this={log}>
    {#each chat.state.messages as message (message.key)}
      <MessageBubble {message} />
    {/each}
    {#each chat.state.pendingPermissions as request (request.requestId)}
      <PermissionCard
        {request}
        onrespond={(requestId, option) => chat.respondPermission(requestId, option)}
      />
    {/each}
    {#if chat.state.busy && chat.state.pendingPermissions.length === 0}
      <div class="typing">…</div>
    {/if}
  </main>

  <Composer busy={chat.state.busy} onsend={(text) => chat.send(text)} />
</div>

<style>
  :global(:root) {
    --border: #d4d4d8;
    --surface: #f4f4f5;
    --muted: #71717a;
    --mono: ui-monospace, "Cascadia Code", monospace;
  }
  @media (prefers-color-scheme: dark) {
    :global(:root) {
      --border: #3f3f46;
      --surface: #27272a;
      --muted: #a1a1aa;
    }
    :global(body) {
      background: #18181b;
      color: #f4f4f5;
    }
  }
  :global(body) {
    margin: 0;
    font-family: system-ui, sans-serif;
  }

  .app {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }
  header {
    display: flex;
    align-items: center;
    gap: 1em;
    padding: 0.6em 1em;
    border-bottom: 1px solid var(--border);
  }
  h1 {
    font-size: 1em;
    margin: 0;
  }
  select {
    font: inherit;
    padding: 0.25em 0.5em;
    background: var(--surface);
    color: inherit;
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .usage {
    margin-left: auto;
    color: var(--muted);
    font-size: 0.8em;
    font-family: var(--mono);
  }
  main {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.7em;
    padding: 1em;
  }
  .typing {
    color: var(--muted);
  }
</style>

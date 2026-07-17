<script lang="ts">
  import { onDestroy, onMount } from "svelte";

  import { ChatController } from "$lib/chat.svelte";
  import AgentSettings from "$lib/components/AgentSettings.svelte";
  import Composer from "$lib/components/Composer.svelte";
  import MessageBubble from "$lib/components/MessageBubble.svelte";
  import PermissionCard from "$lib/components/PermissionCard.svelte";
  import Sidebar from "$lib/components/Sidebar.svelte";

  const chat = new ChatController();

  let log: HTMLElement | undefined = $state();
  let settingsOpen = $state(false);

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
    const cost = (label: string, amount: number) =>
      `${label} ${amount.toFixed(3)} ${usage.costCurrency ?? ""}`.trimEnd();
    const parts = [
      `${usage.usedTokens.toLocaleString()} / ${usage.contextSize.toLocaleString()} tokens`,
    ];
    if (usage.lastTurnCost !== null) parts.push(cost("turn", usage.lastTurnCost));
    if (usage.costAmount !== null) parts.push(cost("total", usage.costAmount));
    return parts.join(" · ");
  });
</script>

<div class="app">
  <Sidebar
    sessions={chat.sessions}
    activeId={chat.state.sessionId}
    onselect={(session) => chat.resumeSession(session)}
    onnew={() => chat.newChat()}
  />
  <div class="chat">
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
    <button
      type="button"
      class="settings"
      aria-label="Agent settings"
      onclick={() => (settingsOpen = true)}
    >
      ⚙
    </button>
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

    <Composer
      busy={chat.state.busy}
      onsend={(text) => chat.send(text)}
      oncancel={() => chat.cancel()}
    />
  </div>
</div>

{#if settingsOpen}
  <AgentSettings
    agents={chat.agents}
    onsave={(spec) => chat.saveAgent(spec)}
    ondelete={(id) => chat.deleteAgent(id)}
    onclose={() => (settingsOpen = false)}
  />
{/if}

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
    height: 100vh;
  }
  .chat {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
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
  .settings {
    font: inherit;
    background: none;
    border: none;
    color: var(--muted);
    cursor: pointer;
    padding: 0 0.2em;
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

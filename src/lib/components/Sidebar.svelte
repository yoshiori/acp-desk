<script lang="ts">
  import type { SessionSummary } from "$lib/ipc";

  let {
    sessions,
    activeId,
    onselect,
    onnew,
  }: {
    sessions: SessionSummary[];
    activeId: string | null;
    onselect: (session: SessionSummary) => void;
    onnew: () => void;
  } = $props();

  const MINUTE = 60;
  const HOUR = 60 * MINUTE;
  const DAY = 24 * HOUR;

  function relativeTime(unixSeconds: number): string {
    const elapsed = Math.max(0, Math.floor(Date.now() / 1000) - unixSeconds);
    if (elapsed < MINUTE) return "now";
    if (elapsed < HOUR) return `${Math.floor(elapsed / MINUTE)}m ago`;
    if (elapsed < DAY) return `${Math.floor(elapsed / HOUR)}h ago`;
    return new Date(unixSeconds * 1000).toLocaleDateString();
  }
</script>

<nav>
  <button type="button" class="new-chat" onclick={onnew}>+ New chat</button>
  <ul>
    {#each sessions as session (session.id)}
      <li>
        <button
          type="button"
          class="session"
          class:active={session.id === activeId}
          onclick={() => onselect(session)}
        >
          <span class="title">{session.title ?? "(no messages)"}</span>
          <span class="meta">{session.agentName} · {relativeTime(session.updatedAt)}</span>
        </button>
      </li>
    {/each}
  </ul>
</nav>

<style>
  nav {
    display: flex;
    flex-direction: column;
    width: 15em;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    overflow-y: auto;
    padding: 0.6em;
    gap: 0.6em;
  }
  .new-chat {
    font: inherit;
    padding: 0.4em;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    color: inherit;
    cursor: pointer;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 0.15em;
  }
  .session {
    font: inherit;
    display: flex;
    flex-direction: column;
    gap: 0.15em;
    width: 100%;
    text-align: left;
    padding: 0.45em 0.55em;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .session:hover {
    background: var(--surface);
  }
  .session.active {
    background: var(--surface);
    outline: 1px solid var(--border);
  }
  .title {
    font-size: 0.9em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .meta {
    font-size: 0.75em;
    color: var(--muted);
  }
</style>

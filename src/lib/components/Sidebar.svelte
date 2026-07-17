<script lang="ts">
  import type { SessionSummary } from "$lib/ipc";

  let {
    sessions,
    activeId,
    workingDir,
    onselect,
    onnew,
    onpickdir,
  }: {
    sessions: SessionSummary[];
    activeId: string | null;
    /** Directory new chats will run in; null means the app's own cwd. */
    workingDir: string | null;
    onselect: (session: SessionSummary) => void;
    onnew: () => void;
    onpickdir: () => void;
  } = $props();

  /** Shortens $HOME-prefixed paths the way shells display them. */
  function displayDir(dir: string): string {
    const home = dir.match(/^\/home\/[^/]+|^\/Users\/[^/]+/);
    return home ? `~${dir.slice(home[0].length)}` : dir;
  }

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
  <button
    type="button"
    class="cwd"
    title={workingDir ?? "Working directory: app default"}
    onclick={onpickdir}
  >
    📁 {workingDir ? displayDir(workingDir) : "(app default)"}
  </button>
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
  .cwd {
    font-family: var(--mono);
    font-size: 0.75em;
    padding: 0.3em 0.4em;
    border: none;
    background: transparent;
    color: var(--muted);
    cursor: pointer;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    direction: rtl;
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

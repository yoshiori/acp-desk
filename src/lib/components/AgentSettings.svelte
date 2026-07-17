<script lang="ts">
  import type { AgentListing, AgentSpec } from "$lib/ipc";

  let {
    agents,
    onsave,
    ondelete,
    onclose,
  }: {
    agents: AgentListing[];
    onsave: (spec: AgentSpec) => Promise<string | null>;
    ondelete: (id: number) => Promise<void>;
    onclose: () => void;
  } = $props();

  interface Draft {
    id: number | null;
    name: string;
    command: string;
    /** Space-separated in the UI; quoting is not supported yet. */
    argsText: string;
    /** One NAME=value per line in the UI. */
    envText: string;
    error: string | null;
  }

  function toDraft(agent: AgentListing): Draft {
    return {
      id: agent.id,
      name: agent.name,
      command: agent.command,
      argsText: agent.args.join(" "),
      envText: agent.env.map((pair) => `${pair.name}=${pair.value}`).join("\n"),
      error: null,
    };
  }

  function emptyDraft(): Draft {
    return { id: null, name: "", command: "", argsText: "", envText: "", error: null };
  }

  // Deliberate snapshot: the dialog is recreated on every open, and edits
  // must not be clobbered by list refreshes while the user is typing.
  // svelte-ignore state_referenced_locally
  let drafts = $state<Draft[]>(agents.map(toDraft));

  function toSpec(draft: Draft): AgentSpec {
    return {
      id: draft.id,
      name: draft.name.trim(),
      command: draft.command.trim(),
      args: draft.argsText.split(/\s+/).filter((part) => part.length > 0),
      env: draft.envText
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0)
        .map((line) => {
          const eq = line.indexOf("=");
          return eq === -1
            ? { name: line, value: "" }
            : { name: line.slice(0, eq), value: line.slice(eq + 1) };
        }),
    };
  }

  async function save(draft: Draft) {
    draft.error = await onsave(toSpec(draft));
  }

  async function remove(draft: Draft, index: number) {
    if (draft.id !== null) await ondelete(draft.id);
    drafts.splice(index, 1);
  }
</script>

<div class="overlay" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dialog" role="dialog" aria-label="Agent settings">
    <header>
      <h2>Agents</h2>
      <button type="button" class="close" onclick={onclose}>×</button>
    </header>

    {#each drafts as draft, index (draft.id ?? `new-${index}`)}
      <fieldset>
        <label>
          <span>Name</span>
          <input bind:value={draft.name} placeholder="My Agent" />
        </label>
        <label>
          <span>Command (absolute path)</span>
          <input bind:value={draft.command} placeholder="/usr/local/bin/agent" class="mono" />
        </label>
        <label>
          <span>Arguments (space-separated)</span>
          <input bind:value={draft.argsText} placeholder="--acp" class="mono" />
        </label>
        <label>
          <span>Environment (NAME=value per line)</span>
          <textarea bind:value={draft.envText} rows="2" placeholder="GEMINI_API_KEY=…" class="mono"
          ></textarea>
        </label>
        {#if draft.error}
          <p class="error">{draft.error}</p>
        {/if}
        <div class="row-actions">
          <button type="button" class="primary" onclick={() => save(draft)}>Save</button>
          <button type="button" onclick={() => remove(draft, index)}>Delete</button>
        </div>
      </fieldset>
    {/each}

    <button type="button" class="add" onclick={() => drafts.push(emptyDraft())}>
      + Add agent
    </button>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 40%);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 10;
  }
  .dialog {
    background: var(--dialog-bg, #fff);
    color: inherit;
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 1em 1.2em;
    width: min(34em, 90vw);
    max-height: 85vh;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 0.8em;
  }
  @media (prefers-color-scheme: dark) {
    .dialog {
      --dialog-bg: #18181b;
    }
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  h2 {
    margin: 0;
    font-size: 1.05em;
  }
  .close {
    font: inherit;
    font-size: 1.2em;
    background: none;
    border: none;
    color: inherit;
    cursor: pointer;
    line-height: 1;
  }
  fieldset {
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 0.8em;
    display: flex;
    flex-direction: column;
    gap: 0.6em;
    margin: 0;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25em;
    font-size: 0.85em;
    color: var(--muted);
  }
  input,
  textarea {
    font: inherit;
    font-size: 0.95rem;
    color: inherit;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 0.35em 0.5em;
  }
  .mono {
    font-family: var(--mono);
  }
  .error {
    margin: 0;
    color: #dc2626;
    font-size: 0.85em;
  }
  .row-actions {
    display: flex;
    gap: 0.5em;
  }
  button {
    font: inherit;
    font-size: 0.9em;
    padding: 0.3em 0.9em;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: transparent;
    color: inherit;
    cursor: pointer;
  }
  .primary {
    background: #2563eb;
    border-color: #2563eb;
    color: #fff;
  }
  .add {
    align-self: flex-start;
  }
</style>

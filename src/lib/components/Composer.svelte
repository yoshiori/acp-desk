<script lang="ts">
  let {
    busy,
    onsend,
    oncancel,
  }: { busy: boolean; onsend: (text: string) => void; oncancel: () => void } = $props();

  let draft = $state("");
  // One cancel per turn: disable Stop after the click and recover when the
  // turn actually ends (busy drops), since cancellation is asynchronous.
  let stopping = $state(false);
  $effect(() => {
    if (!busy) stopping = false;
  });

  function stop() {
    stopping = true;
    oncancel();
  }

  function submit() {
    if (!draft.trim() || busy) return;
    onsend(draft);
    draft = "";
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === "Enter" && !event.shiftKey && !event.isComposing) {
      event.preventDefault();
      submit();
    }
  }
</script>

<form
  class="composer"
  onsubmit={(event) => {
    event.preventDefault();
    submit();
  }}
>
  <textarea
    aria-label="Message"
    placeholder="Message the agent… (Enter to send, Shift+Enter for newline)"
    rows="2"
    bind:value={draft}
    {onkeydown}
  ></textarea>
  {#if busy}
    <button type="button" class="stop" disabled={stopping} onclick={stop}>
      {stopping ? "Stopping…" : "Stop"}
    </button>
  {:else}
    <button type="submit" disabled={!draft.trim()}>Send</button>
  {/if}
</form>

<style>
  .composer {
    display: flex;
    gap: 0.6em;
    padding: 0.8em;
    border-top: 1px solid var(--border);
  }
  textarea {
    flex: 1;
    resize: none;
    padding: 0.6em 0.8em;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    color: inherit;
    font: inherit;
  }
  button {
    padding: 0 1.2em;
    border: none;
    border-radius: 8px;
    background: #2563eb;
    color: #fff;
    font: inherit;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.stop {
    background: transparent;
    border: 1px solid #dc2626;
    color: #dc2626;
  }
</style>

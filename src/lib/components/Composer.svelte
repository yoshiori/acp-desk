<script lang="ts">
  let {
    busy,
    onsend,
  }: { busy: boolean; onsend: (text: string) => void } = $props();

  let draft = $state("");

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
  <button type="submit" disabled={busy || !draft.trim()}>
    {busy ? "…" : "Send"}
  </button>
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
</style>

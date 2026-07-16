<script lang="ts">
  import type { PermissionOption, PermissionRequest } from "$lib/chat-core";

  let {
    request,
    onrespond,
  }: {
    request: PermissionRequest;
    onrespond: (requestId: number, option: PermissionOption) => void;
  } = $props();

  // One answer per card: the backend treats a second answer as an error, so
  // disable the buttons as soon as the user picks.
  let answered = $state(false);

  function pick(option: PermissionOption) {
    if (answered) return;
    answered = true;
    onrespond(request.requestId, option);
  }
</script>

<div class="permission">
  <div class="question">
    Allow <span class="title">{request.toolTitle}</span>?
  </div>
  <div class="options">
    {#each request.options as option (option.optionId)}
      <button
        type="button"
        class={option.kind.startsWith("allow") ? "allow" : "reject"}
        disabled={answered}
        onclick={() => pick(option)}
      >
        {option.name}
      </button>
    {/each}
  </div>
</div>

<style>
  .permission {
    align-self: flex-start;
    max-width: 46em;
    padding: 0.7em 0.9em;
    border: 1px solid var(--border);
    border-left: 3px solid #d97706;
    border-radius: 10px;
    background: var(--surface);
  }
  .question {
    margin-bottom: 0.6em;
  }
  .title {
    font-family: var(--mono);
    font-size: 0.9em;
  }
  .options {
    display: flex;
    gap: 0.5em;
    flex-wrap: wrap;
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
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button.allow {
    background: #2563eb;
    border-color: #2563eb;
    color: #fff;
  }
</style>

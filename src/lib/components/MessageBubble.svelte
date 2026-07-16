<script lang="ts">
  import { openUrl } from "@tauri-apps/plugin-opener";

  import type { ChatMessage } from "$lib/chat-core";
  import { linkAction, renderMarkdown } from "$lib/markdown";

  let { message }: { message: ChatMessage } = $props();

  // Agent output renders as markdown; user text stays verbatim (echoing
  // exactly what was typed is more predictable than reinterpreting it).
  const isMarkdown = $derived(message.role === "assistant" || message.role === "thought");

  /** Links must leave the app through the system browser: following them
   * in the webview would replace the chat UI with the target page. The
   * decision uses the raw href attribute (see linkAction). */
  function onLinkClick(event: MouseEvent) {
    const target = event.target as HTMLElement | null;
    const anchor = target?.closest("a");
    if (!anchor) return;
    const href = anchor.getAttribute("href");
    switch (linkAction(href)) {
      case "external":
        event.preventDefault();
        if (href) openUrl(href).catch((error) => console.error("failed to open link", error));
        break;
      case "blocked":
        event.preventDefault();
        break;
      case "internal":
        break;
    }
  }
</script>

{#if message.role === "tool"}
  <div class="tool" data-status={message.status}>
    <span class="tool-status">{message.status ?? "pending"}</span>
    <span class="tool-title">{message.text}</span>
  </div>
{:else if isMarkdown}
  <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
  <div class="bubble markdown {message.role}" onclick={onLinkClick}>
    <!-- Safe: renderMarkdown escapes raw HTML (html: false) and refuses
         javascript: URLs; see src/lib/markdown.ts. -->
    {@html renderMarkdown(message.text)}
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

  /* Rendered markdown: {@html} content escapes Svelte's scoping, so the
     inner elements are styled through :global under the .markdown scope.
     The markdown-only palette lives here (not on :root) so everything this
     component needs is defined in this file. */
  .markdown {
    --code-bg: #e4e4e7;
    --link: #2563eb;
    white-space: normal;
  }
  @media (prefers-color-scheme: dark) {
    .markdown {
      --code-bg: #18181b;
      --link: #60a5fa;
    }
  }
  .markdown :global(:first-child) {
    margin-top: 0;
  }
  .markdown :global(:last-child) {
    margin-bottom: 0;
  }
  .markdown :global(p),
  .markdown :global(ul),
  .markdown :global(ol),
  .markdown :global(pre),
  .markdown :global(blockquote),
  .markdown :global(table) {
    margin: 0.5em 0;
  }
  .markdown :global(ul),
  .markdown :global(ol) {
    padding-left: 1.4em;
  }
  .markdown :global(pre) {
    padding: 0.6em 0.8em;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow-x: auto;
    background: var(--code-bg);
  }
  .markdown :global(code) {
    font-family: var(--mono);
    font-size: 0.9em;
  }
  .markdown :global(:not(pre) > code) {
    background: var(--code-bg);
    padding: 0.1em 0.35em;
    border-radius: 4px;
  }
  .markdown :global(h1),
  .markdown :global(h2),
  .markdown :global(h3),
  .markdown :global(h4) {
    margin: 0.7em 0 0.4em;
    line-height: 1.3;
  }
  .markdown :global(h1) {
    font-size: 1.25em;
  }
  .markdown :global(h2) {
    font-size: 1.15em;
  }
  .markdown :global(h3),
  .markdown :global(h4) {
    font-size: 1.05em;
  }
  .markdown :global(blockquote) {
    border-left: 3px solid var(--border);
    padding: 0 0 0 0.8em;
    color: var(--muted);
  }
  .markdown :global(a) {
    color: var(--link);
  }
  .markdown :global(table) {
    display: block;
    overflow-x: auto;
    border-collapse: collapse;
  }
  .markdown :global(th),
  .markdown :global(td) {
    border: 1px solid var(--border);
    padding: 0.25em 0.6em;
  }
  .markdown :global(hr) {
    border: none;
    border-top: 1px solid var(--border);
    margin: 0.8em 0;
  }
</style>

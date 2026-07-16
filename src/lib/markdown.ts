// Markdown rendering for agent output (DESIGN.md §"Markdown renderer").
//
// The agent's text is untrusted input: `html: false` makes markdown-it
// escape raw HTML instead of passing it through, and markdown-it's default
// validateLink already refuses javascript:/vbscript:/data: URLs. The result
// is safe to inject with {@html} as long as those settings stay.

import MarkdownIt from "markdown-it";

const renderer = new MarkdownIt({
  html: false,
  linkify: true,
  // Chat convention: a single newline is a visible line break, not a soft wrap.
  breaks: true,
});

export function renderMarkdown(text: string): string {
  return renderer.render(text);
}

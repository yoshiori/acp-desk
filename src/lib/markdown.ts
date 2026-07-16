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

export type LinkAction = "external" | "internal" | "blocked";

/**
 * Decides what a click on a rendered link should do, from the *raw* href
 * attribute (`anchor.href` would resolve relative/hash links against the
 * app URL and make everything look absolute):
 * - http(s) opens in the system browser,
 * - pure hash links may scroll in place,
 * - anything else (relative paths, other schemes) is blocked — following
 *   it would navigate the webview away from the chat UI.
 */
export function linkAction(rawHref: string | null): LinkAction {
  if (rawHref === null) return "internal";
  if (/^https?:/i.test(rawHref)) return "external";
  if (rawHref.startsWith("#")) return "internal";
  return "blocked";
}

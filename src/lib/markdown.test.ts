import { describe, expect, it } from "vitest";

import { renderMarkdown } from "./markdown";

describe("renderMarkdown", () => {
  it("renders paragraphs and emphasis", () => {
    const html = renderMarkdown("Hello **world**");
    expect(html).toContain("<p>");
    expect(html).toContain("<strong>world</strong>");
  });

  it("renders fenced code blocks without interpreting their content", () => {
    const html = renderMarkdown("```rust\nlet x = a < b;\n```");
    expect(html).toContain("<pre>");
    expect(html).toContain("let x = a &lt; b;");
  });

  it("renders inline code", () => {
    expect(renderMarkdown("run `cargo test` now")).toContain("<code>cargo test</code>");
  });

  it("escapes raw HTML instead of injecting it", () => {
    const html = renderMarkdown('<img src=x onerror="alert(1)"><script>alert(2)</script>');
    expect(html).not.toContain("<img");
    expect(html).not.toContain("<script");
    expect(html).toContain("&lt;");
  });

  it("refuses javascript: links", () => {
    const html = renderMarkdown("[click](javascript:alert(1))");
    expect(html).not.toContain('href="javascript:');
  });

  it("renders markdown links as anchors", () => {
    const html = renderMarkdown("[ACP](https://agentclientprotocol.com/)");
    expect(html).toContain('href="https://agentclientprotocol.com/"');
  });

  it("linkifies bare URLs", () => {
    const html = renderMarkdown("see https://example.com for details");
    expect(html).toContain('href="https://example.com"');
  });

  it("treats single newlines as line breaks (chat convention)", () => {
    expect(renderMarkdown("line one\nline two")).toContain("<br");
  });

  it("renders lists", () => {
    const html = renderMarkdown("- first\n- second");
    expect(html).toContain("<ul>");
    expect(html).toContain("<li>first</li>");
  });
});

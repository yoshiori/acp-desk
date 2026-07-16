import { describe, expect, it } from "vitest";

import { formatDiff } from "./diff";

describe("formatDiff", () => {
  it("marks every line as added for a new file (no old text)", () => {
    expect(formatDiff(null, "a\nb")).toEqual(["+a", "+b"]);
  });

  it("trims common prefix and suffix and keeps context lines", () => {
    const oldText = "one\ntwo\nthree\nfour\nfive";
    const newText = "one\ntwo\nTHREE\nfour\nfive";
    expect(formatDiff(oldText, newText)).toEqual([
      " one",
      " two",
      "-three",
      "+THREE",
      " four",
      " five",
    ]);
  });

  it("limits context to two lines on each side", () => {
    const before = ["a", "b", "c", "d"].join("\n");
    const after = ["a", "b", "c", "D"].join("\n");
    expect(formatDiff(before, after)).toEqual([" b", " c", "-d", "+D"]);
  });

  it("handles pure insertion", () => {
    expect(formatDiff("a\nc", "a\nb\nc")).toEqual([" a", "+b", " c"]);
  });

  it("handles pure deletion", () => {
    expect(formatDiff("a\nb\nc", "a\nc")).toEqual([" a", "-b", " c"]);
  });

  it("reports identical texts as no changes", () => {
    expect(formatDiff("same", "same")).toEqual(["(no changes)"]);
  });
});

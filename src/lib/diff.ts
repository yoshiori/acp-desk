// Minimal line diff for the tool-call detail view.
//
// ACP `Diff` blocks carry whole before/after texts. A real diff algorithm
// (LCS) is overkill for showing "what did the tool change": trimming the
// common prefix/suffix isolates the changed region for the typical
// single-block edit, and everything else still renders correctly, just with
// a larger changed region.

const CONTEXT_LINES = 2;

/** Returns display lines prefixed like a unified diff: " " context,
 * "-" removed, "+" added. A missing `oldText` means a brand-new file. */
export function formatDiff(oldText: string | null | undefined, newText: string): string[] {
  const newLines = newText.split("\n");
  if (oldText == null) {
    return newLines.map((line) => `+${line}`);
  }
  const oldLines = oldText.split("\n");

  let start = 0;
  while (
    start < oldLines.length &&
    start < newLines.length &&
    oldLines[start] === newLines[start]
  ) {
    start++;
  }
  let oldEnd = oldLines.length;
  let newEnd = newLines.length;
  while (oldEnd > start && newEnd > start && oldLines[oldEnd - 1] === newLines[newEnd - 1]) {
    oldEnd--;
    newEnd--;
  }

  const removed = oldLines.slice(start, oldEnd).map((line) => `-${line}`);
  const added = newLines.slice(start, newEnd).map((line) => `+${line}`);
  if (removed.length === 0 && added.length === 0) {
    return ["(no changes)"];
  }
  const before = newLines
    .slice(Math.max(0, start - CONTEXT_LINES), start)
    .map((line) => ` ${line}`);
  const after = newLines
    .slice(newEnd, Math.min(newLines.length, newEnd + CONTEXT_LINES))
    .map((line) => ` ${line}`);
  return [...before, ...removed, ...added, ...after];
}

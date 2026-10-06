import { expect, it } from "vitest";
import { prompt } from "./chatGuard";
// Pure ANSI parser golden table: faint placeholders and wrapped drafts must not
// become interchangeable; no existing guard coverage or test-only seam exists.
it.each([
  ["❯ \n────", "clear"],
  ["❯ \x1b[2mTry a question\x1b[22m\n────", "clear"],
  ["❯ hello\n────", "draft"],
  ["❯ \n  wrapped draft\n────", "draft"],
  ["older ❯ mention\nno prompt", "unknown"],
  ["❯ old\n────\n ❯ \n────", "clear"],
  ["❯ \x1b[38;2;2;100;100mreal draft\x1b[0m\n────", "draft"],
  ["❯ \x1b[2;38;2;255;255;255mplaceholder\x1b[m\n────", "clear"],
  ["❯ \x1b[2mplaceholder\x1b[22mtyped\n────", "draft"],
  ["❯ \x1b]8;;https://example.com\x07\x1b]8;;\x07\n────", "clear"],
])("classifies %j as %s", (screen, expected) => expect(prompt(screen)).toBe(expected));

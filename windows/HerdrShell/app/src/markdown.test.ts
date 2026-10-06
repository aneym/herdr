import { describe, expect, it, vi } from "vitest";
import { renderMarkdown } from "./markdown";

// Exercise the real prose parser's delimiter/code/link edge cases. Only the
// third-party browser sanitizer is bypassed; no DOM is available in this runner.
// Existing tests do not cover markdown, and no test-only production seam is needed.
vi.mock("dompurify", () => ({ default: { sanitize: (html: string) => html } }));

describe("markdown tilde delimiters", () => {
  it.each([
    ["~/x ... ~/y", "<p>~/x ... ~/y</p>\n"],
    ["Read ~/x then ~/y.", "<p>Read ~/x then ~/y.</p>\n"],
    ["~ordinary prose~", "<p>~ordinary prose~</p>\n"],
    ["~~removed~~ and ~literal~", "<p><del>removed</del> and ~literal~</p>\n"],
    ["~~**removed**~~", "<p><del><strong>removed</strong></del></p>\n"],
    ["`~/x ~~code~~`", "<p><code>~/x ~~code~~</code></p>\n"],
    ["[file](https://example.com/~/x)", '<p><a href="https://example.com/~/x">file</a></p>\n'],
    ["\\~escaped\\~", "<p>~escaped~</p>\n"],
    ["~~ unclosed", "<p>~~ unclosed</p>\n"],
  ])("renders %s", (input, html) => {
    expect(renderMarkdown(input)).toBe(html);
  });
});

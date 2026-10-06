import { Marked } from "marked";
import DOMPurify from "dompurify";
// Same rendering policy as Chat.tsx; that surface remains owned by the chat slice.
const markdown = new Marked({ renderer: { html: () => "" } });
export function renderMarkdown(text: string): string {
  return DOMPurify.sanitize(markdown.parse(text, { async: false }), { USE_PROFILES: { html: true }, FORBID_TAGS: ["img", "video", "audio", "iframe", "form", "input", "button", "style"] });
}

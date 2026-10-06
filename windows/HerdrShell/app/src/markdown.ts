import { Marked } from "marked";
import DOMPurify from "dompurify";
const markdown = new Marked({
  renderer: { html: () => "" },
  tokenizer: {
    del(src) {
      // Keep GFM's content rules, but require two tildes on both sides.
      const match = /^~~(?=[^\s~])((?:\\.|[^\\])*?(?:\\.|[^\s~\\]))~~(?=[^~]|$)/.exec(src);
      if (match) return { type: "del", raw: match[0], text: match[1], tokens: this.lexer.inlineTokens(match[1]) };
    },
  },
});
export function renderMarkdown(text: string): string {
  return DOMPurify.sanitize(markdown.parse(text, { async: false }), { USE_PROFILES: { html: true }, FORBID_TAGS: ["img", "video", "audio", "iframe", "form", "input", "button", "style"] });
}

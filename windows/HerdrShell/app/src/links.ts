import type { Terminal } from "@xterm/xterm";
import { WebLinksAddon } from "@xterm/addon-web-links";
// Ctrl-click opens a terminal link, as Cmd-click does on the Mac. Agents print most links as
// OSC 8 hyperlinks (panes report TERM_PROGRAM=ghostty and the attach stream forwards them), and
// xterm's own OSC 8 provider outranks every addon: without a linkHandler it asks window.confirm
// and calls window.open, which the WebView never hands to the browser.
export function installLinks(term: Terminal, open: (url: string) => void): void {
  const activate = (event: MouseEvent, uri: string) => { if (event.ctrlKey) open(uri); };
  term.options.linkHandler = { activate };
  term.loadAddon(new WebLinksAddon(activate));
}

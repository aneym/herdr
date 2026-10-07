import type { Terminal } from "@xterm/xterm";
// Copy from a terminal pane, as the Mac Shell's SurfaceView does.
//
// The pane arrives through `herdr terminal attach`, which redraws each row with cursor
// positioning and host autowrap off, so xterm sees a soft-wrapped line as separate rows
// and keeps the blank cells a redraw wrote. The server holds the pane's real, wrap-aware
// screen: copy reads the selected cells from it (`pane.selection.read`) and falls back to
// xterm's rows, trimmed, when it cannot.
//
// A program that owns the mouse (Claude Code, codex) receives drags, so xterm selects
// nothing. Keep a shadow of such a drag (a double click keeps the word, a triple click
// the line) so Ctrl+C has something to copy. When the program copies the selection itself
// (Claude's copy on select, OSC 52), its text is exact and the shadow does not replace it.

/** A viewport cell; ranges are inclusive, in reading order. */
export interface Cell { col: number; row: number }
export interface Range { start: Cell; end: Cell }
export type Api = (method: string, params: unknown) => Promise<unknown>;

export function ordered(a: Cell, b: Cell): Range {
  return a.row < b.row || (a.row === b.row && a.col <= b.col) ? { start: a, end: b } : { start: b, end: a };
}

/** Trailing blanks of every line go: they are padding the redraw wrote, not text. */
export function trimRows(text: string): string {
  return text.split("\n").map(line => line.replace(/\s+$/u, "")).join("\n");
}

/** The range's text from xterm's own rows (no wrap knowledge beyond xterm's). */
export function localText(term: Terminal, range: Range): string {
  const buffer = term.buffer.active;
  let out = "";
  for (let row = range.start.row; row <= range.end.row; row++) {
    const from = row === range.start.row ? range.start.col : 0;
    const to = row === range.end.row ? range.end.col + 1 : term.cols;
    const text = buffer.getLine(buffer.viewportY + row)?.translateToString(false, from, to) ?? "";
    const wraps = row < range.end.row && buffer.getLine(buffer.viewportY + row + 1)?.isWrapped;
    out += wraps ? text : text.replace(/\s+$/u, "") + (row < range.end.row ? "\n" : "");
  }
  return out;
}

const blank = (term: Terminal, cell: Cell) => {
  const line = term.buffer.active.getLine(term.buffer.active.viewportY + cell.row);
  return !(line?.getCell(cell.col)?.getChars() ?? "").trim();
};

/** The run of non-blank cells around a cell on its row, null on a blank cell. */
export function wordAt(term: Terminal, cell: Cell): Range | null {
  if (blank(term, cell)) return null;
  let lo = cell.col, hi = cell.col;
  while (lo > 0 && !blank(term, { col: lo - 1, row: cell.row })) lo--;
  while (hi < term.cols - 1 && !blank(term, { col: hi + 1, row: cell.row })) hi++;
  return { start: { col: lo, row: cell.row }, end: { col: hi, row: cell.row } };
}

interface PaneScroll { max_offset_from_bottom: number; offset_from_bottom: number }
const viewportTop = async (api: Api, paneId: string) => {
  const scroll = ((await api("pane.get", { pane_id: paneId })) as { pane?: { scroll?: PaneScroll } }).pane?.scroll;
  return scroll ? scroll.max_offset_from_bottom - scroll.offset_from_bottom : 0;
};

/** The range's text from the server's screen, or `fallback` when the server cannot answer. */
export async function exactText(api: Api, paneId: string, range: Range, fallback: string): Promise<string> {
  try {
    // Rows are absolute in the server's screen; output that scrolls between the two reads
    // moves the viewport, so read again rather than return the wrong rows.
    for (let attempt = 0; attempt < 3; attempt++) {
      const top = await viewportTop(api, paneId);
      const read = (await api("pane.selection.read", {
        pane_id: paneId,
        anchor: { row: top + range.start.row, col: range.start.col },
        cursor: { row: top + range.end.row, col: range.end.col },
      })) as { text?: string };
      if (typeof read.text !== "string") return fallback;
      if ((await viewportTop(api, paneId)) === top) return trimRows(read.text);
    }
  } catch { /* An older or remote server without the read keeps xterm's text. */ }
  return fallback;
}

export function decodeClipboard(b64: string): string {
  return new TextDecoder().decode(Uint8Array.from(atob(b64), ch => ch.charCodeAt(0)));
}

export class PaneCopy {
  private drag: { from: Cell; to: Cell } | null = null;
  private shadow: Range | null = null;
  private clock = 0;
  private selectedAt = 0;
  private programAt = 0;
  constructor(private term: Terminal, private api: Api, private paneId: () => string, private write: (text: string) => Promise<void>) {}

  /** Left press. `owned` is a program owning the mouse; Shift makes xterm select instead. */
  press(cell: Cell, clicks: number, owned: boolean) {
    this.shadow = null; this.drag = null;
    if (!owned) return;
    if (clicks === 2) this.keep(wordAt(this.term, cell));
    else if (clicks >= 3) this.keep({ start: { col: 0, row: cell.row }, end: { col: this.term.cols - 1, row: cell.row } });
    else this.drag = { from: cell, to: cell };
  }
  move(cell: Cell) { if (this.drag) this.drag.to = cell; }
  release(cell: Cell) {
    const drag = this.drag; this.drag = null;
    if (!drag) return;
    drag.to = cell;
    if (drag.from.col !== drag.to.col || drag.from.row !== drag.to.row) this.keep(ordered(drag.from, drag.to));
  }
  private keep(range: Range | null) {
    this.shadow = range && localText(this.term, range).trim() ? range : null;
    if (this.shadow) this.selectedAt = ++this.clock;
  }
  /** Typing ends a shadow selection, so a later Ctrl+C interrupts again. */
  clear() { this.shadow = null; }

  /** OSC 52 from the pane's program: the host clipboard gets it as written. */
  async programWrote(b64: string) {
    this.programAt = ++this.clock;
    await this.write(decodeClipboard(b64));
  }

  has(): boolean { return this.term.hasSelection() || this.shadow !== null; }

  /** Copies the selection; false when there is none, so the clipboard is left alone. */
  async copy(): Promise<boolean> {
    let range: Range | null = null;
    if (this.term.hasSelection()) {
      const position = this.term.getSelectionPosition();
      if (position) {
        // xterm's end column is exclusive; a selection ending at column 0 ends the row before.
        const end = position.end.x > 0 ? { col: position.end.x - 1, row: position.end.y } : { col: this.term.cols - 1, row: position.end.y - 1 };
        const top = this.term.buffer.active.viewportY;
        range = { start: { col: position.start.x, row: position.start.y - top }, end: { col: end.col, row: end.row - top } };
      }
      this.term.clearSelection();
    } else if (this.shadow) {
      range = this.shadow;
      // The program already copied this selection itself, with its own exact text.
      if (this.programAt > this.selectedAt) return true;
    }
    if (!range) return false;
    await this.write(await exactText(this.api, this.paneId(), range, localText(this.term, range)));
    return true;
  }
}

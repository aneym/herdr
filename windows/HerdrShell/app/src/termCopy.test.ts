// @vitest-environment happy-dom
import { Terminal } from "@xterm/xterm";
import { describe, expect, it } from "vitest";
import { handleKey } from "./keys";
import { PaneCopy } from "./termCopy";
import type { Api } from "./termCopy";
// Copy in a terminal pane on a real xterm buffer, the way PaneTerm wires it: the pane's
// program owns the mouse (Claude Code, codex), rows arrive as herdr's attach redraws them
// (cursor positioned, padded with blanks), and the server API is the faked network edge.
const redraw = async (term: Terminal, rows: string[]) => {
  const bytes = rows.map((row, i) => `\x1b[${i + 1};1H${row.padEnd(term.cols, " ")}`).join("");
  await new Promise<void>(resolve => term.write(bytes, resolve));
};
const offline: Api = () => Promise.reject(new Error("no server"));
async function pane(rows: string[], api: Api = offline) {
  const term = new Terminal({ cols: 30, rows: 4, scrollback: 0, allowProposedApi: true });
  await redraw(term, rows);
  const written: string[] = [];
  const copier = new PaneCopy(term, api, () => "w1:p1", async text => { written.push(text); });
  const sent: string[] = [];
  const key = (key: string, mods: KeyboardEventInit = {}) => handleKey(new KeyboardEvent("keydown", { key, ...mods }), {
    term, copier, mode: { mouse: true, sgrPixels: false, kittyFlags: 0, modifyOtherKeys: 0 },
    send: async text => { sent.push(text); }, shortcut: () => false,
  });
  return { term, copier, written, key };
}
const drag = (copier: PaneCopy, from: [number, number], to: [number, number]) => {
  copier.press({ col: from[0], row: from[1] }, 1, true);
  copier.move({ col: to[0], row: to[1] });
  copier.release({ col: to[0], row: to[1] });
};

describe("terminal copy on the PC", () => {
  it("Ctrl+C copies a drag the program received, without the redraw's padding", async () => {
    const { copier, written, key } = await pane(["COPY-ALPHA first line", "COPY-BETA second"]);
    drag(copier, [0, 0], [29, 1]);
    const result = key("c", { ctrlKey: true });
    expect(result.handled).toBe(true);
    await result.work;
    expect(written).toEqual(["COPY-ALPHA first line\nCOPY-BETA second"]);
  });

  it("Ctrl+C with nothing selected still reaches the program, and typing ends a selection", async () => {
    const { copier, written, key } = await pane(["alpha beta"]);
    expect(key("c", { ctrlKey: true }).handled).toBe(false);
    drag(copier, [0, 0], [4, 0]);
    expect(key("x").handled).toBe(false);
    expect(key("c", { ctrlKey: true }).handled).toBe(false);
    const shifted = key("C", { ctrlKey: true, shiftKey: true });
    await shifted.work;
    expect(written).toEqual([]);
  });

  it("double click keeps the word and triple click the line", async () => {
    const { copier, written } = await pane(["run cargo-nextest now"]);
    copier.press({ col: 6, row: 0 }, 2, true);
    await copier.copy();
    copier.press({ col: 2, row: 0 }, 3, true);
    await copier.copy();
    expect(written).toEqual(["cargo-nextest", "run cargo-nextest now"]);
  });

  it("a pane whose program does not own the mouse leaves selection to xterm", async () => {
    const { copier, written } = await pane(["alpha beta"]);
    copier.press({ col: 0, row: 0 }, 1, false);
    copier.release({ col: 4, row: 0 });
    expect(copier.has()).toBe(false);
    expect(await copier.copy()).toBe(false);
    expect(written).toEqual([]);
  });

  it("reads the selection from the server's wrap-aware screen at the pane's scroll position", async () => {
    const calls: [string, unknown][] = [];
    const api: Api = async (method, params) => {
      calls.push([method, params]);
      if (method === "pane.get") return { pane: { scroll: { max_offset_from_bottom: 120, offset_from_bottom: 20, viewport_rows: 4 } } };
      return { type: "pane_selection", pane_id: "w1:p1", text: "a long line the pane soft-wrapped   \nnext" };
    };
    const { copier, written } = await pane(["a long line the pane soft-wra", "pped", "next"], api);
    drag(copier, [0, 0], [3, 2]);
    await copier.copy();
    expect(calls.find(([method]) => method === "pane.selection.read")?.[1]).toEqual({ pane_id: "w1:p1", anchor: { row: 100, col: 0 }, cursor: { row: 102, col: 3 } });
    expect(written).toEqual(["a long line the pane soft-wrapped\nnext"]);
  });

  it("keeps the program's own copy (OSC 52) of the selection it made", async () => {
    const { copier, written, key } = await pane(["⏺ wrapped by claude"]);
    drag(copier, [2, 0], [18, 0]);
    await copier.programWrote(btoa(String.fromCharCode(...new TextEncoder().encode("wrapped by claude — exact"))));
    const result = key("c", { ctrlKey: true });
    expect(result.handled).toBe(true);
    await result.work;
    expect(written).toEqual(["wrapped by claude — exact"]);
  });

  it("keeps OSC 52 received while the shadow's server read is pending", async () => {
    let finishRead!: (value: unknown) => void;
    let started!: () => void;
    const reading = new Promise<void>(resolve => { started = resolve; });
    const api: Api = async method => {
      if (method === "pane.get") return { pane: { scroll: { max_offset_from_bottom: 100, offset_from_bottom: 0 } } };
      return new Promise(resolve => { finishRead = resolve; started(); });
    };
    const { copier, written, key } = await pane(["shadow text"], api);
    drag(copier, [0, 0], [29, 0]);
    const result = key("c", { ctrlKey: true });
    expect(result.handled).toBe(true);
    await reading;
    await copier.programWrote(btoa("program text"));
    finishRead({ text: "shadow text" });
    await result.work;
    expect(written[written.length - 1]).toBe("program text");
    expect(written).toEqual(["program text"]);
  });

  it("keeps the selection-time snapshot when the pane scrolls before copy", async () => {
    let gets = 0;
    const api: Api = async method => {
      if (method === "pane.get") return { pane: { scroll: { max_offset_from_bottom: gets++ === 0 ? 100 : 101, offset_from_bottom: 0 } } };
      return { text: "server text for shifted rows" };
    };
    const { term, copier, written, key } = await pane(["selected first", "selected second"], api);
    drag(copier, [0, 0], [29, 1]);
    await redraw(term, ["shifted first", "shifted second"]);
    const result = key("c", { ctrlKey: true });
    expect(result.handled).toBe(true);
    await result.work;
    expect(written).toEqual(["selected first\nselected second"]);
    expect(written).not.toContain("server text for shifted rows");
  });
});

// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import PaneTerm from "./PaneTerm";
import type { PaneController } from "./PaneTerm";
import { bridge, fromBase64, utf8Base64 } from "./bridge";
import type { AttachEvent } from "./bridge";
import { handleDropPaths } from "./dropPaths";

// Integration at native IPC: mounted terminals, real xterm bracketed paste and
// the real drop handler. Only native I/O and the layout-less DOM edge are fake.
it("drops into the addressed split, focuses it, preserves image upload and never sends Enter", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const panes = new Map<string, PaneController>();
  const streams = new Map<string, (event: AttachEvent) => void>();
  const focus = vi.fn();
  vi.spyOn(bridge, "attach").mockImplementation(async (_m, id, _c, _r, _mode, fn) => { streams.set(id, fn); return id === "left" ? 1 : 2; });
  const input = vi.spyOn(bridge, "input").mockResolvedValue();
  vi.spyOn(bridge, "resize").mockResolvedValue();
  vi.spyOn(bridge, "close").mockResolvedValue();
  vi.spyOn(bridge, "dropReadImage").mockImplementation(async path => path.endsWith(".png") ? "iVBORw0KGgo=" : null);
  const api = vi.spyOn(bridge, "api").mockResolvedValue({ paste_text: "'/host/image.png'" });
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
    const left = this.dataset.paneId === "right" ? 650 : 240;
    return { x: left, y: 40, left, top: 40, right: left + 400, bottom: 600, width: 400, height: 560, toJSON: () => ({}) };
  });
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<>{["left", "right"].map(id => <PaneTerm key={id} pane={{ pane_id: id, terminal_id: id, workspace_id: "w", tab_id: "t" }} machine="studio" focused={id === "left"} onFocus={focus} shortcut={() => false} register={(id, pane) => { if (pane) panes.set(id, pane); else panes.delete(id); }} />)}</>));
    const right = host.querySelector<HTMLElement>('[data-pane-id="right"]')!;
    vi.spyOn(document, "elementFromPoint").mockReturnValue(right);
    await act(async () => streams.get("right")!({ kind: "mode", b64: utf8Base64("\x1b[?2004hready"), mouse: false, sgrPixels: false, kittyFlags: 0, modifyOtherKeys: 0 }));
    await vi.waitFor(() => expect(panes.get("right")!.read()).toContain("ready"));
    input.mockClear();
    await act(async () => { expect(await handleDropPaths({ paths: ["C:\\my folder", "C:\\notes.txt"], x: 700, y: 200 }, [...panes.values()])).toEqual({ ok: true, pane_id: "right" }); });
    expect(focus).toHaveBeenLastCalledWith("right");
    expect(input.mock.calls).toEqual([[2, utf8Base64('\x1b[200~"C:\\my folder" C:\\notes.txt \x1b[201~')]]);
    expect(new TextDecoder().decode(fromBase64(input.mock.calls[0][1]))).not.toMatch(/[\r\n]/);
    input.mockClear();
    await act(async () => { await handleDropPaths({ paths: ["C:\\image.png"], x: 700, y: 200 }, [...panes.values()]); });
    expect(api).toHaveBeenCalledWith("studio", "clipboard.image.write", { extension: "png", data_base64: "iVBORw0KGgo=" });
    expect(input.mock.calls).toEqual([[2, utf8Base64("\x1b[200~'/host/image.png' \x1b[201~")]]);
    input.mockClear();
    expect(await handleDropPaths({ paths: ["C:\\folder"], x: 100, y: 200 }, [...panes.values()])).toEqual({ ok: true, pane_id: null });
    expect(input).not.toHaveBeenCalled();
  } finally { act(() => root.unmount()); host.remove(); vi.restoreAllMocks(); }
});

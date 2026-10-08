// @vitest-environment happy-dom
import { Terminal } from "@xterm/xterm";
import { afterEach, describe, expect, it } from "vitest";
import { controlKey, handleKey } from "./keys";

// Integration at the native IPC boundary: real key routing, bridge and xterm
// paste processing; only the clipboard/server boundary is fake.
describe("Windows clipboard image paste", () => {
  afterEach(() => { delete (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__; });
  async function pasteClipboard(text: string | null, image: string | null, result: unknown = { paste_text: "'/host/image.png'" }, fail = false) {
    const calls: { cmd: string; args: Record<string, unknown> }[] = [];
    (window as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ = {
      invoke: async (cmd: string, args: Record<string, unknown> = {}) => {
        calls.push({ cmd, args });
        if (cmd === "clipboard_read") { if (text === null) throw new Error("no text"); return text; }
        if (cmd === "clipboard_read_image") return image;
        if (cmd === "api_request") { if (fail) throw new Error("upload failed"); return result; }
        throw new Error(`unexpected command ${cmd}`);
      },
    };
    const term = new Terminal();
    const host = document.createElement("div");
    document.body.append(host);
    term.open(host);
    const pasted: string[] = [];
    term.onData(data => pasted.push(data));
    const target = { term, machine: "agent-host", mode: { mouse: false, sgrPixels: false, kittyFlags: 0, modifyOtherKeys: 0 }, send: async () => {}, shortcut: () => false, copier: { has: () => false, copy: async () => false, clear: () => {} } };
    const key = handleKey(controlKey("ctrl+v"), target);
    expect(key.handled).toBe(true);
    let error: unknown;
    try { await key.work; } catch (value) { error = value; }
    term.dispose();
    host.remove();
    return { calls, pasted, error };
  }
  it("uploads an image to its agent host and pastes only the server's text", async () => {
    const { calls, pasted, error } = await pasteClipboard(null, "iVBORw0KGgo=");
    expect(error).toBeUndefined();
    expect(calls).toContainEqual({ cmd: "api_request", args: { machine: "agent-host", method: "clipboard.image.write", params: { extension: "png", data_base64: "iVBORw0KGgo=" } } });
    expect(pasted).toEqual(["'/host/image.png'"]);
  });
  it.each(["plain text", ""])("keeps text precedence, including %j, without reading or uploading the image", async text => {
    const { calls, pasted, error } = await pasteClipboard(text, "iVBORw0KGgo=");
    expect(error).toBeUndefined();
    expect(calls.map(call => call.cmd)).toEqual(["clipboard_read"]);
    expect(pasted.join("")).toBe(text);
  });
  it("denies failed uploads without pasting a client-local path or fallback text", async () => {
    const { pasted, error } = await pasteClipboard(null, "iVBORw0KGgo=", undefined, true);
    expect(pasted).toEqual([]);
    expect(String(error)).toContain("upload failed");
  });
  it.each([{}, { paste_text: "" }, { paste_text: 123 }])("denies a malformed upload reply %j", async result => {
    const { pasted, error } = await pasteClipboard(null, "iVBORw0KGgo=", result);
    expect(pasted).toEqual([]);
    expect(error).toBeInstanceOf(Error);
  });
  it.each(["", "AAAA".repeat(Math.ceil((16 * 1024 * 1024 + 1) / 3))])("denies empty or oversized image data before upload", async image => {
    const { calls, pasted, error } = await pasteClipboard(null, image);
    expect(calls.some(call => call.cmd === "api_request")).toBe(false);
    expect(pasted).toEqual([]);
    expect(error).toBeInstanceOf(Error);
  });
});

import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Snapshot } from "./model";

export interface MachineStatus { name: string; state: "connecting" | "up" | "down"; error?: string }
export interface Mode { mouse: boolean; sgrPixels: boolean; kittyFlags: number; modifyOtherKeys: number }
export type AttachEvent = { kind: "bytes"; b64: string } | ({ kind: "mode"; b64: string } & Mode)
  | { kind: "bell"; count: number } | { kind: "notice"; message: string } | { kind: "closed"; reason: string };
export const bridge = {
  machines: () => invoke<MachineStatus[]>("machines_list"),
  snapshot: (machine: string) => invoke<Snapshot>("snapshot", { machine }),
  api: (machine: string, method: string, params: unknown) => invoke<unknown>("api_request", { machine, method, params }),
  machineEvents: (fn: (status: MachineStatus) => void) => listen<MachineStatus>("herdr://machine", e => fn(e.payload)),
  snapshots: (fn: (value: { machine: string; snapshot: Snapshot }) => void) => listen<{ machine: string; snapshot: Snapshot }>("herdr://snapshot", e => fn(e.payload)),
  attach: (machine: string, terminalId: string, cols: number, rows: number, mode: "attach" | "observe", fn: (event: AttachEvent) => void) => {
    const onEvent = new Channel<AttachEvent>();
    onEvent.onmessage = fn;
    return invoke<number>("attach_open", { machine, terminalId, cols, rows, mode, onEvent });
  },
  input: (handle: number, data: string) => invoke<void>("attach_input", { handle, data }),
  resize: (handle: number, cols: number, rows: number) => invoke<void>("attach_resize", { handle, cols, rows }),
  scroll: (handle: number, up: boolean, lines: number) => invoke<void>("attach_scroll", { handle, up, lines }),
  takeControl: (handle: number) => invoke<void>("attach_take_control", { handle }),
  close: (handle: number) => invoke<void>("attach_close", { handle }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  clipboardRead: () => invoke<string>("clipboard_read"),
  clipboardWrite: (text: string) => invoke<void>("clipboard_write", { text }),
  info: () => invoke<{ version: string; commit: string; built_at: string }>("app_info"),
  controlEvent: <T,>(cmd: string, fn: (payload: T) => void) => listen<T>(`ctl-${cmd}`, e => fn(e.payload)),
  readResult: (text: string) => invoke<void>("ctl_read_result", { text }),
  controlResult: (cmd: string, result: unknown) => invoke<void>(`ctl_${cmd}_result`, { result }),
};
export function toBase64(bytes: Uint8Array): string {
  let text = "";
  for (const byte of bytes) text += String.fromCharCode(byte);
  return btoa(text);
}
export const utf8Base64 = (text: string) => toBase64(new TextEncoder().encode(text));
export const fromBase64 = (text: string) => Uint8Array.from(atob(text), ch => ch.charCodeAt(0));

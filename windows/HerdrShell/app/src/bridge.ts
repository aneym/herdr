import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Snapshot } from "./model";
import { appTheme, terminalThemes } from "./theme";
import type { Mode as ThemeMode } from "./theme";

const themeSubscriptions = new Map<number, () => void>();
const colorChannels = (hex: string | undefined): [number, number, number] => {
  if (!hex || !/^#[0-9a-f]{6}$/i.test(hex)) throw new Error("Terminal default color must be #RRGGBB");
  return [1, 3, 5].map(offset => parseInt(hex.slice(offset, offset + 2), 16)) as [number, number, number];
};
function reportTheme(handle: number, mode: ThemeMode): Promise<void> {
  const palette = terminalThemes[mode];
  return invoke<void>("attach_theme", { handle, dark: mode === "dark", foreground: colorChannels(palette.foreground), background: colorChannels(palette.background) });
}

export interface UpdateStatus { current: string; staged: { sha: string; built_at: string } | null; available: boolean; previous: { sha: string } | null }
export interface MachineStatus { name: string; state: "connecting" | "up" | "down"; error?: string }
export interface Mode { mouse: boolean; sgrPixels: boolean; kittyFlags: number; modifyOtherKeys: number }
export type AttachEvent = { kind: "bytes"; b64: string } | ({ kind: "mode"; b64: string } & Mode)
  | { kind: "bell"; count: number } | { kind: "clipboard"; b64: string } | { kind: "notice"; message: string } | { kind: "closed"; reason: string };
export const bridge = {
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  updateApply: () => invoke<void>("update_apply"),
  updateRollback: () => invoke<void>("update_rollback"),
  fileStat: (machine: string, path: string) => invoke<{ exists: boolean; size: number; mtime_ms: number; inode: number }>("file_stat", { machine, path }),
  fileRead: (machine: string, path: string, offset: number, max: number) => invoke<{ size: number; mtime_ms: number; inode: number; offset: number; data_b64: string }>("file_read", { machine, path, offset, max }),
  fileList: (machine: string, path: string) => invoke<string[]>("file_list", { machine, path }),
  remoteHome: (machine: string) => invoke<string>("remote_home", { machine }),
  machines: () => invoke<MachineStatus[]>("machines_list"),
  snapshot: (machine: string) => invoke<Snapshot>("snapshot", { machine }),
  api: (machine: string, method: string, params: unknown) => invoke<unknown>("api_request", { machine, method, params }).catch((error: unknown) => {
    if (error && typeof error === "object" && "code" in error && "message" in error) {
      const server = error as { code: string; message: string; reason?: string };
      throw Object.assign(new Error(server.message), { toString: () => `herdr api error ${server.code}: ${server.message}` }, { code: server.code, reason: server.reason });
    }
    throw error;
  }),
  machineEvents: (fn: (status: MachineStatus) => void) => listen<MachineStatus>("herdr://machine", e => fn(e.payload)),
  snapshots: (fn: (value: { machine: string; snapshot: Snapshot }) => void) => listen<{ machine: string; snapshot: Snapshot }>("herdr://snapshot", e => fn(e.payload)),
  attach: (machine: string, terminalId: string, cols: number, rows: number, mode: "attach" | "observe", fn: (event: AttachEvent) => void) => {
    const onEvent = new Channel<AttachEvent>();
    onEvent.onmessage = fn;
    return invoke<number>("attach_open", { machine, terminalId, cols, rows, mode, onEvent }).then(async handle => {
      const theme = appTheme();
      const unsubscribe = theme.subscribe(current => { void reportTheme(handle, current).catch(error => console.error("Host theme report failed", error)); });
      themeSubscriptions.set(handle, unsubscribe);
      try { await reportTheme(handle, theme.mode); }
      catch (error) { console.error("Host theme report failed", error); }
      return handle;
    });
  },
  input: (handle: number, data: string) => invoke<void>("attach_input", { handle, data }),
  resize: (handle: number, cols: number, rows: number) => invoke<void>("attach_resize", { handle, cols, rows }),
  scroll: (handle: number, up: boolean, lines: number) => invoke<void>("attach_scroll", { handle, up, lines }),
  takeControl: (handle: number) => invoke<void>("attach_take_control", { handle }),
  close: (handle: number) => {
    themeSubscriptions.get(handle)?.();
    themeSubscriptions.delete(handle);
    return invoke<void>("attach_close", { handle });
  },
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

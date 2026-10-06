import type { ITheme } from "@xterm/xterm";
// Light and dark are both first-class, as in the Mac shell (Theme.swift). The effective mode
// is the Windows app theme, followed live through prefers-color-scheme, unless an override
// says otherwise. Chrome colors live in styles.css under :root[data-theme]; the terminal
// palettes live here because xterm takes them as options, not CSS.
export type Mode = "light" | "dark";
export type Appearance = "system" | Mode;

// Catppuccin Mocha, the Mac shell's dark Ghostty theme.
const dark: ITheme = {
  background: "#1e1e2e", foreground: "#cdd6f4", cursor: "#f5e0dc", cursorAccent: "#1e1e2e", selectionBackground: "#45475a",
  black: "#45475a", red: "#f38ba8", green: "#a6e3a1", yellow: "#f9e2af", blue: "#89b4fa", magenta: "#f5c2e7", cyan: "#94e2d5", white: "#bac2de",
  brightBlack: "#585b70", brightRed: "#f38ba8", brightGreen: "#a6e3a1", brightYellow: "#f9e2af", brightBlue: "#89b4fa", brightMagenta: "#f5c2e7", brightCyan: "#94e2d5", brightWhite: "#a6adc8",
};
// SF Paper, the Mac shell's light Ghostty theme (Alex, 2026-09-25).
const light: ITheme = {
  background: "#ffffff", foreground: "#3a3a38", cursor: "#0969da", cursorAccent: "#ffffff", selectionBackground: "#dbe5f1", selectionForeground: "#1f2328",
  black: "#24292f", red: "#cf222e", green: "#116329", yellow: "#7d4e00", blue: "#0969da", magenta: "#8250df", cyan: "#1b7c83", white: "#6e7781",
  brightBlack: "#57606a", brightRed: "#a40e26", brightGreen: "#1a7f37", brightYellow: "#633c01", brightBlue: "#218bff", brightMagenta: "#a475f9", brightCyan: "#3192aa", brightWhite: "#8c959f",
};
export const terminalThemes: Record<Mode, ITheme> = { dark, light };

export interface MediaLike { matches: boolean; addEventListener: (type: "change", fn: () => void) => void; removeEventListener: (type: "change", fn: () => void) => void }

export class ThemeStore {
  private appearance: Appearance = "system";
  private listeners = new Set<(mode: Mode) => void>();
  private current: Mode;
  constructor(private root: HTMLElement, private media: MediaLike | null) {
    this.current = this.compute();
    this.apply();
    media?.addEventListener("change", this.update);
  }
  get mode(): Mode { return this.current; }
  get override(): Appearance { return this.appearance; }
  setOverride(value: Appearance) {
    if (value !== "system" && value !== "light" && value !== "dark") throw new Error("Appearance must be system, light or dark");
    this.appearance = value;
    this.update();
  }
  subscribe(fn: (mode: Mode) => void): () => void { this.listeners.add(fn); return () => { this.listeners.delete(fn); }; }
  dispose() { this.media?.removeEventListener("change", this.update); this.listeners.clear(); }
  private compute(): Mode { return this.appearance !== "system" ? this.appearance : this.media?.matches === false ? "light" : "dark"; }
  private apply() { this.root.dataset.theme = this.current; this.root.style.colorScheme = this.current; }
  private update = () => {
    const next = this.compute();
    if (next === this.current) return;
    this.current = next;
    this.apply();
    this.listeners.forEach(fn => fn(next));
  };
}

let shared: ThemeStore | null = null;
// One store per window, created on first use so tests can import this module without a DOM.
export function appTheme(): ThemeStore {
  shared ??= new ThemeStore(document.documentElement, typeof matchMedia === "function" ? matchMedia("(prefers-color-scheme: dark)") : null);
  return shared;
}

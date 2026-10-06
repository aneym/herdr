// Light and dark are both first-class, as in the Mac shell (Theme.swift). The effective mode
// is the Windows app theme, followed live through prefers-color-scheme, unless an override
// says otherwise. Chrome colors come from the generated tokens.css under :root[data-theme];
// xterm takes its palettes from the generated tokens.ts.
export type Mode = "light" | "dark";
export type Appearance = "system" | Mode;

export { terminalThemes } from "./tokens";

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

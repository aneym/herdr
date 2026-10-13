// @vitest-environment happy-dom
// Alex, 2026-09-29: "an agent is working, it should be oragne. finished, green ... the idle styling is fine".
// Renders the sidebar's real state dot and face, then resolves each class's colour through styles.css
// and the generated tokens.css in both modes, the way WebView2 would.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it } from "vitest";
import { Status } from "./Sidebar";
import { faceDot } from "./faces";
const fsModule = "node:fs";
const { readFileSync } = await import(/* @vite-ignore */ fsModule) as { readFileSync(path: string, encoding: "utf8"): string };
const styles = readFileSync("src/styles.css", "utf8"), tokens = readFileSync("src/tokens.css", "utf8");
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let dispose = () => {};
afterEach(() => dispose());

const modeBlock = (mode: "dark" | "light") => tokens.slice(tokens.indexOf(mode === "dark" ? ':root, :root[data-theme="dark"]' : ':root[data-theme="light"]')).split("}")[0];
const token = (mode: "dark" | "light", name: string) => modeBlock(mode).match(new RegExp(`${name}:\\s*(#[0-9A-Fa-f]{6})`))?.[1];
/** The colour a `.a.b` rule sets for `prop`, resolved through the mode's tokens. */
function resolve(selector: string, prop: string, mode: "dark" | "light") {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const variable = styles.match(new RegExp(`${escaped}\\s*\\{[^}]*?${prop}:\\s*var\\((--[\\w-]+)\\)`))?.[1];
  return variable ? token(mode, variable) : undefined;
}
const rgb = (hex: string) => [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16));
const orange = (hex?: string) => { if (!hex) return false; const [r, g, b] = rgb(hex); return r > g && g > b && g > r * 0.3 && g < r * 0.75; };
const green = (hex?: string) => { if (!hex) return false; const [r, g, b] = rgb(hex); return g > r && g > b; };

function classesOf(element: JSX.Element) {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  act(() => root.render(element));
  dispose = () => { act(() => root.unmount()); host.remove(); };
  const classes = [...host.firstElementChild!.classList];
  dispose();
  return classes;
}

it("draws a working chat orange and an unread finish green in both modes; blocked and idle keep their colours", () => {
  for (const mode of ["dark", "light"] as const) {
    const colour = (status: string) => {
      const classes = classesOf(<Status solid status={status} />);
      expect(classes).toContain("state-dot");
      return resolve(`.status.${status}`, "color", mode) ?? token(mode, "--shell-muted");
    };
    expect(orange(colour("working")), `${mode} working ${colour("working")}`).toBe(true);
    expect(green(colour("done")), `${mode} done ${colour("done")}`).toBe(true);
    expect(colour("blocked")).toBe(token(mode, "--shell-bad"));
    expect(colour("idle")).toBe(token(mode, "--shell-muted"));
    expect(orange(resolve(`.face-dot.${faceDot("working")}`, "background", mode)), `${mode} working face dot`).toBe(true);
    expect(green(resolve(`.face-dot.${faceDot("done")}`, "background", mode)), `${mode} done face dot`).toBe(true);
  }
});

// Shapes as the TUI's state_icons and the Mac's statusGlyph draw them (Alex, 2026-09-29, reversed
// by him the same day: working a circle, done and blocked a square).
it("keeps the Mac's shapes: blocked and done are squares, working is a circle", () => {
  expect(styles).toMatch(/\.status\.state-dot\s*\{[^}]*border-radius:\s*50%/);
  for (const status of ["blocked", "done"]) expect(styles).toMatch(new RegExp(`\\.status\\.state-dot\\.${status}\\s*\\{[^}]*border-radius:\\s*var\\(--shell-radius-curve1_5\\)`));
  const glyph = (status: string) => { const host = document.createElement("div"); document.body.append(host); const root = createRoot(host); act(() => root.render(<Status status={status} />)); const text = host.textContent; act(() => root.unmount()); host.remove(); return text; };
  expect([glyph("working"), glyph("done"), glyph("blocked")]).toEqual(["●", "■", "■"]);
});

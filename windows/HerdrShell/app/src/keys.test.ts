import { describe, expect, it } from "vitest";
import { actionFor } from "./keys";
import type { Action, KeyEvent } from "./keys";
const event = (key: string, mods: Partial<KeyEvent> = {}): KeyEvent => ({ key, ctrlKey: false, shiftKey: false, altKey: false, metaKey: false, ...mods });
// Pure key normalization has modifier and shifted-symbol edge cases. This table
// owns the requested Windows binding contract; no existing test covers it.
describe("Windows shell bindings", () => {
  const ctrl = { ctrlKey: true }, shifted = { ctrlKey: true, shiftKey: true }, alt = { altKey: true }, split = { altKey: true, shiftKey: true };
  const cases: [string, Partial<KeyEvent>, Action][] = [
    ["T", shifted, "new_tab"], ["w", shifted, "close_pane"], ["Z", shifted, "zoom_pane"], ["P", shifted, "switcher"], ["A", shifted, "next_attention"],
    ["=", split, "split_right"], ["+", split, "split_right"], ["-", split, "split_down"], ["_", split, "split_down"],
    ["ArrowLeft", alt, "focus_pane_left"], ["ArrowRight", alt, "focus_pane_right"], ["ArrowUp", alt, "focus_pane_up"], ["ArrowDown", alt, "focus_pane_down"],
    ["F2", {}, "rename_tab"], ["b", ctrl, "toggle_sidebar"], ["Tab", ctrl, "next_tab"], ["Tab", shifted, "prev_tab"],
    ...Array.from({ length: 9 }, (_, i): [string, Partial<KeyEvent>, Action] => [String(i + 1), ctrl, `select_tab_${i + 1}`]),
  ];
  it.each(cases)("%s %j resolves to %s", (key, mods, expected) => {
    expect(actionFor(event(key, mods))).toBe(expected);
    expect(actionFor(event(key, { ...mods, metaKey: true }))).toBeNull();
  });
  it("leaves plain typing, terminal control keys and Alt letters untouched", () => {
    for (const key of "abcdefghijklmnopqrstuvwxyz") {
      expect(actionFor(event(key))).toBeNull();
      expect(actionFor(event(key, ctrl))).toBe(key === "b" ? "toggle_sidebar" : null);
      expect(actionFor(event(key, alt))).toBeNull();
    }
    for (const key of ["0", "Enter", "ArrowLeft", "F2", "1"]) expect(actionFor(event(key, shifted))).toBeNull();
    expect(actionFor(event("t", { ...shifted, altKey: true }))).toBeNull();
    expect(actionFor(event("=", alt))).toBeNull();
  });
});

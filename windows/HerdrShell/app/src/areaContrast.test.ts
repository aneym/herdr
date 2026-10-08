import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
const tokensCSS = readFileSync("src/tokens.css", "utf8");
import { areaDotNeedsRing } from "./areaContrast";

// Golden cases guard WCAG's nonlinear luminance algorithm, threshold and invalid input.
function sidebarSurface(mode: "dark" | "light") {
  return tokensCSS.split(`:root[data-theme="${mode}"]`)[1].match(/--shell-surface:\s*([^;]+);/)![1];
}
describe("area dot contrast", () => {
  it.each([
    ["#1F1F23", "dark", true], ["#4F5BD5", "dark", false],
    ["#FFFFFF", "light", true], ["#4F5BD5", "light", false],
    ["#1f1f23", "dark", true], ["invalid", "dark", false],
  ] as const)("%s on %s needs ring: %s", (color, mode, expected) => {
    expect(areaDotNeedsRing(color, sidebarSurface(mode))).toBe(expected);
  });
});

import { expect, it } from "vitest";
import { cycleMachine } from "./machines";
// Pure cyclic indexing owns wraparound, stale stored names, and empty-list behavior.
// A wrong offset can switch the wrong server; key-binding tests do not exercise it.
it.each([
  [[], "studio", 1, null], [[], "studio", -1, null],
  [["pc"], "pc", 1, "pc"], [["pc"], "pc", -1, "pc"],
  [["pc", "studio", "lab"], "pc", -1, "lab"],
  [["pc", "studio", "lab"], "lab", 1, "pc"],
  [["pc", "studio", "lab"], "studio", 1, "lab"],
  [["pc", "studio", "lab"], "studio", -1, "pc"],
  [["pc", "studio"], "deleted", 1, "pc"],
  [["pc", "studio"], "deleted", -1, "studio"],
] as [string[], string, 1 | -1, string | null][])("cycles %j from %s by %s to %s", (names, current, direction, expected) => {
  expect(cycleMachine(names, current, direction)).toBe(expected);
});

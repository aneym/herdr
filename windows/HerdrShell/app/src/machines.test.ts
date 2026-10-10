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
import { renameHosts } from "./machines";
// As the Mac's MachineRows.renameHosts: a case-only match takes the machine's name, but only when
// it is the one machine and the one host row of that spelling.
it("renames a host row to its machine's spelling only when unambiguous", () => {
  const rows = (names: string[]) => names.map(name => ({ name }));
  expect(renameHosts(rows(["Studio", "PC", "forge"]), ["studio", "pc"]).map(r => r.name)).toEqual(["studio", "pc", "forge"]);
  expect(renameHosts(rows(["pc", "PC"]), ["pc"]).map(r => r.name)).toEqual(["pc", "PC"]);
  expect(renameHosts(rows(["PC"]), ["pc", "Pc"]).map(r => r.name)).toEqual(["PC"]);
});

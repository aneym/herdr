// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useAgentCards } from "./AgentFace";
import { AGENTS_DIR } from "./faces";

// Exercise the real polling hook, parser and base64 bridge. Only Tauri's external file-helper
// transport is faked: prior face/parser tests cannot detect folder ordering or duplicate ownership.
const files = vi.hoisted(() => ({ names: [] as string[], texts: {} as Record<string, string> }));
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {},
  invoke: async (command: string, args: { path: string }) => {
    if (command === "file_list") return files.names;
    if (command === "file_read") {
      const text = files.texts[args.path];
      if (text === undefined) throw new Error("Unreadable card");
      return { data_b64: btoa(text) };
    }
    throw new Error(`Unexpected command: ${command}`);
  },
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const roots: ReturnType<typeof createRoot>[] = [];
afterEach(() => {
  act(() => { for (const root of roots.splice(0)) root.unmount(); });
  document.body.replaceChildren();
});

async function loadCards(names: string[], cards: Record<string, unknown>) {
  files.names = names;
  files.texts = Object.fromEntries(Object.entries(cards).map(([name, card]) =>
    [`${AGENTS_DIR}/${name}/agent.json`, typeof card === "string" ? card : JSON.stringify(card)]));
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  roots.push(root);
  function Probe() {
    const cards = useAgentCards("test-machine", true);
    return createElement("output", null, JSON.stringify(cards));
  }
  await act(async () => {
    root.render(createElement(Probe));
  });
  return JSON.parse(host.textContent || "{}");
}

describe("agent card folder precedence", () => {
  it.each([["b-two", "a-one"], ["a-one", "b-two"]])(
    "the first sorted folder wins regardless of file-list order (%s, %s)", async (first, second) => {
      expect(await loadCards([first, second], {
        "a-one": { name: "First", pane: "p1" },
        "b-two": { name: "Second", pane: "p1" },
      })).toEqual({ p1: { name: "First" } });
    },
  );
  it.each([
    { pane: "p1" },
    { name: "Invalid", pane: "none" },
    { name: "Invalid" },
    "{broken json",
  ])("an invalid earlier card does not claim a pane (%j)", async invalid => {
    expect(await loadCards(["b-two", "a-zero", "a-one"], {
      "a-zero": invalid,
      "a-one": { name: "First", pane: "p1" },
      "b-two": { name: "Second", pane: "p1" },
    })).toEqual({ p1: { name: "First" } });
  });
  it("uses code-unit ordering and still loads other panes after an unreadable folder", async () => {
    expect(await loadCards(["a-one", "missing", "B-two", "other"], {
      "a-one": { name: "Lowercase", pane: "p1" },
      "B-two": { name: "Uppercase", pane: "p1" },
      other: { name: "Other", pane: "p2" },
    })).toEqual({ p1: { name: "Uppercase" }, p2: { name: "Other" } });
  });
});

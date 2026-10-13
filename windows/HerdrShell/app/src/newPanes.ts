// Panes another client opened while this app was running. Alex, 2026-10-11: "on book anytime
// i start a new thread it loses focus and i need to click back in to take back control, new
// threads should be focused on the client that made them when they're first made."
// Every Shell showing the tab attaches a new pane as soon as its snapshot lists it, and a
// refused attach used to take over, so an idle machine took the new thread from its maker.
// A pane is another client's new pane when it appears in a snapshot after the first one from
// its machine, this app did not create it, and it appeared less than NEW_PANE_MS ago. Such a
// pane opens observing here instead of taking over; input or Take control still takes it.
// The Mac client keeps the same rule in NewPanes.swift.
import type { Snapshot } from "./model";

export const NEW_PANE_MS = 60_000;

export class NewPanes {
  private baselined = new Set<string>();
  private seen = new Set<string>();
  private appeared = new Map<string, number>();
  private mine = new Map<string, number>();

  observe(machine: string, snapshot: Snapshot, now = Date.now()) {
    const first = !this.baselined.has(machine);
    this.baselined.add(machine);
    for (const pane of snapshot.panes ?? []) {
      const key = `${machine}\n${pane.pane_id}`;
      if (this.seen.has(key)) continue;
      this.seen.add(key);
      if (!first) this.appeared.set(key, now);
    }
    for (const [key, at] of this.appeared) if (now - at >= NEW_PANE_MS) this.appeared.delete(key);
  }

  markMine(machine: string, paneId: string | null | undefined) {
    if (paneId) this.mine.set(`${machine}\n${paneId}`, Date.now());
  }

  // A pane this app made less than NEW_PANE_MS ago opens with a takeover attach, so keys typed as it opens are never
  // lost to a refused attach (an idle client can attach first).
  isMine(machine: string, paneId: string, now = Date.now()) {
    const at = this.mine.get(`${machine}\n${paneId}`);
    return at !== undefined && now - at < NEW_PANE_MS;
  }

  /** This app made the pane, however long ago. */
  madeHere(machine: string, paneId: string) {
    return this.mine.has(`${machine}\n${paneId}`);
  }

  othersNew(machine: string, paneId: string, now = Date.now()) {
    const key = `${machine}\n${paneId}`;
    const at = this.appeared.get(key);
    return !this.mine.has(key) && at !== undefined && now - at < NEW_PANE_MS;
  }
}

export const newPanes = new NewPanes();

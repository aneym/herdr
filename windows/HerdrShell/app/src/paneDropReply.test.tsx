// @vitest-environment happy-dom
// S10/S10b: a pane drop ends with the server's reply to pane.place or pane.swap, never with a guess from snapshots.
// While the drop is pending the boxes stay where they were at the release; snapshots are buffered, not drawn.
// Through the real TabView (window pointer listeners, PaneClip, PaneDrag). Faked: the Tauri bridge (external) and
// PaneTerm, whose xterm/WebGL terminal cannot run in happy-dom. Each case is a post-merge must-fix from review.
// S10c: membership and descriptors freeze too; a press, a host resize or a tab switch ends a pending drop quietly
// first; a settle stays retargetable until its latest retarget has really ended.
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import type { Layout, Pane, Rect, Snapshot } from "./model";
import type { PaneDragState } from "./paneDrag";
const api = vi.hoisted(() => vi.fn());
vi.mock("./bridge", () => ({ bridge: { api: (...args: unknown[]) => api(...args) } }));
vi.mock("./PaneTerm", () => ({ default: (props: { pane: { pane_id: string }; onCapPointerDown?: (event: unknown) => void }) => <div className="pane-cap" data-cap={props.pane.pane_id} onPointerDown={event => props.onCapPointerDown?.(event)} /> }));
vi.mock("./Chat", () => ({ default: () => null }));
const { default: TabView } = await import("./TabView");

// Tab t: a | (b / c) in a 100 x 40 cell area, drawn at 1000 x 800 px.
const r = (x: number, y: number, width: number, height: number): Rect => ({ x, y, width, height });
const tab = (a: Rect, b: Rect, c: Rect): Layout => ({ tab_id: "t", area: r(0, 0, 100, 40), panes: [{ pane_id: "a", rect: a }, { pane_id: "b", rect: b }, { pane_id: "c", rect: c }] });
const three = tab(r(0, 0, 50, 40), r(50, 0, 50, 20), r(50, 20, 50, 20));
// Another client drags the a | (b / c) divider: a, the source, narrows.
const sourceResized = tab(r(0, 0, 40, 40), r(40, 0, 60, 20), r(40, 20, 60, 20));
const B_CENTRE = { clientX: 750, clientY: 200 }, B_RIGHT_BAND = { clientX: 930, clientY: 200 };
let states: PaneDragState[] = [];
let root: Root | undefined;
let host: HTMLDivElement;
let drop: { resolve: (value: unknown) => void };
let dryRun: (params: object) => Promise<unknown>;
/** The tab host's ResizeObserver: a window resize or a sidebar toggle reaches the tab as a new host size. */
let resizeHost: (width: number, height: number) => void;
const animate = vi.fn((_frames: Keyframe[] | PropertyIndexedKeyframes | null, _options?: number | KeyframeAnimationOptions) => ({}) as Animation);
beforeEach(() => {
  vi.useFakeTimers(); api.mockReset(); animate.mockClear(); states = [];
  dryRun = () => new Promise(() => {});
  api.mockImplementation((_machine: string, method: string, params: { dry_run?: boolean; pane_id?: string; target?: { pane_id?: string } }) => {
    // The support probe places a pane beside itself; it answers same_pane.
    if (method === "pane.place" && params?.dry_run && params.target?.pane_id === params.pane_id) return Promise.resolve({ place: { changed: false, reason: "same_pane" } });
    if (method === "pane.place" && params?.dry_run) return dryRun(params);
    return new Promise(resolve => { drop = { resolve }; });
  });
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => setTimeout(() => callback(0), 16));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));
  vi.stubGlobal("matchMedia", () => ({ matches: false }));
  vi.stubGlobal("localStorage", { getItem: () => null, setItem: () => {} });
  vi.stubGlobal("ResizeObserver", class { constructor(private callback: (entries: { contentRect: { width: number; height: number } }[]) => void) {} observe() { resizeHost = (width, height) => this.callback([{ contentRect: { width, height } }]); resizeHost(1000, 800); } disconnect() {} });
  if (!document.elementFromPoint) document.elementFromPoint = () => null;
  Element.prototype.animate = animate;
  host = document.createElement("div"); document.body.append(host);
});
afterEach(() => { act(() => root?.unmount()); root = undefined; host.remove(); vi.useRealTimers(); vi.unstubAllGlobals(); });

function snapshot(layout: Layout): Snapshot {
  return { layouts: [layout], tabs: [], panes: layout.panes.map(p => ({ pane_id: p.pane_id, terminal_id: "term-" + p.pane_id, tab_id: layout.tab_id, workspace_id: "w" })) } as Snapshot;
}
const show = (next: Snapshot) => act(() => root!.render(<TabView snapshot={next} selected="t" machine="studio" focused={null} onFocus={() => {}} shortcut={() => false} register={() => {}} pin={() => {}} onDragChange={state => states.push(state)} />));
/** Each call is a new snapshot from the server: a new object, as every herdr://snapshot event brings. */
async function mount() {
  root ??= createRoot(host);
  const render = (next: Layout) => show(snapshot(structuredClone(next)));
  render(three);
  await settleTimers(0); // the pane.place support probe answers
  return render;
}
const settleTimers = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
const fire = (target: EventTarget, type: string, init: PointerEventInit) => act(() => { target.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, pointerId: 1, ...init })); });
async function dropAOn(point: { clientX: number; clientY: number }) {
  fire(host.querySelector('[data-cap="a"]')!, "pointerdown", { button: 0, buttons: 1, clientX: 200, clientY: 10 });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 300, clientY: 400 });
  fire(window, "pointermove", { button: -1, buttons: 1, ...point });
  await settleTimers(0);
  fire(window, "pointerup", { button: 0, buttons: 0, ...point });
  expect(last().phase).toBe("dropped");
}
const last = () => states[states.length - 1];
const clip = (id: string) => host.querySelector<HTMLElement>(`.pane-clip[data-pane="${id}"]`)!;
/** Pane boxes on a settle transition, 40 ms after a change (PaneClip starts the move two frames in). */
const settles = () => [...host.querySelectorAll<HTMLElement>(".pane-clip")].filter(el => el.style.transition.includes("transform")).map(el => el.dataset.pane).sort();
const drawn = (id: string) => [clip(id).style.transform, clip(id).style.width, clip(id).style.height];
/** A film of one pane box: [transform, width, height, settle transition on], one entry per change. */
let film: string[][] = [];
function shoot(id = "a") {
  const frame = [...drawn(id), String(clip(id).style.transition.includes("transform"))];
  if (JSON.stringify(film[film.length - 1]) !== JSON.stringify(frame)) film.push(frame);
}
/** Run the clock in 8 ms steps, shooting after each, so every frame the view would paint is on the film. */
async function roll(ms: number, id = "a") { for (let t = 0; t < ms; t += 8) { await settleTimers(8); shoot(id); } }
const FROZEN_A = ["translate(0px, 0px)", "499px", "800px"];
const chipSprangBack = () => animate.mock.calls.some(([frames]) => JSON.stringify(frames).includes("translate(") && JSON.stringify(frames).includes("opacity\":0"));

it("keeps the boxes where they were at the release while a swap is pending, then on a refusal shows the buffered snapshot at once and plays the cancel", async () => {
  const render = await mount();
  await dropAOn(B_CENTRE);
  expect(api).toHaveBeenCalledWith("studio", "pane.swap", { source_pane_id: "a", target_pane_id: "b" });
  render(sourceResized);
  await settleTimers(40);
  expect(settles()).toEqual([]);
  expect(drawn("a")).toEqual(FROZEN_A);
  expect(last().phase).toBe("dropped");
  await act(async () => { drop.resolve({ swap: { changed: false, reason: "no_neighbor", source_pane_id: "a", focused_pane_id: "a", layout: sourceResized } }); });
  expect(last().phase).toBe("idle");
  expect(drawn("a")).toEqual(["translate(0px, 0px)", "399px", "800px"]);
  await settleTimers(40);
  expect(settles()).toEqual([]);
  expect(chipSprangBack()).toBe(true);
  expect(last().end).toBe("cancel");
  expect(clip("a").classList.contains("lifted")).toBe(false);
});

it("settles a pending edge drop to the buffered snapshot on its reply, and a snapshot mid-settle retargets the running settle", async () => {
  const render = await mount();
  await dropAOn(B_RIGHT_BAND);
  expect(api).toHaveBeenLastCalledWith("studio", "pane.place", { pane_id: "a", target: { type: "pane", pane_id: "b" }, side: "right", focus: true, dry_run: false });
  film = []; shoot();
  render(sourceResized);
  await roll(40);
  expect(last().phase).toBe("dropped");
  expect(clip("a").classList.contains("lifted")).toBe(true);
  // The server applied the drop on top of the resize: b | a over c. Its snapshot has not landed yet.
  const placed = tab(r(70, 0, 30, 20), r(40, 0, 30, 20), r(40, 20, 60, 20));
  await act(async () => { drop.resolve({ place: { changed: true, dry_run: false, pane_id: "a", previous_pane_id: "a", placed_rect: placed.panes[0].rect, focused_pane_id: "a", target_layout: placed } }); });
  shoot();
  expect(last()).toMatchObject({ phase: "idle", end: "settle" });
  await roll(80);
  expect(settles()).toEqual(["a", "b", "c"]);
  // The drop's own snapshot lands mid-settle: the boxes head for it from where they are, with the transition kept on.
  render(placed); shoot();
  await roll(400);
  expect(film).toEqual([
    [...FROZEN_A, "false"], // released, then frozen through the foreign resize
    ["translate(0px, 0px)", "399px", "800px", "true"], // settling to the buffered snapshot
    ["translate(700px, 0px)", "300px", "399px", "true"], // retargeted mid-flight, never back to the release frame
    ["translate(700px, 0px)", "300px", "399px", "false"], // the settle ends where it was headed
  ]);
  expect(chipSprangBack()).toBe(false);
  // After the settle, the next snapshot applies as it is.
  const later = tab(r(80, 0, 20, 20), r(40, 0, 40, 20), r(40, 20, 60, 20));
  render(later);
  await settleTimers(40);
  expect(settles()).toEqual([]);
  expect(drawn("a")).toEqual(["translate(800px, 0px)", "200px", "399px"]);
});

it("ends a drop whose target was resized after its dry run on the reply, and takes the coalesced snapshot as is", async () => {
  dryRun = () => Promise.resolve({ place: { changed: true, dry_run: true, pane_id: "a", previous_pane_id: "a", placed_rect: r(75, 0, 25, 20), focused_pane_id: "a", target_layout: three } });
  const render = await mount();
  await dropAOn(B_RIGHT_BAND);
  expect(last().ghost).toEqual(r(750, 0, 250, 400)); // the dry run's placed_rect, cached
  // Another client widened b before the drop committed, so a lands narrower than the dry run said.
  const placed = tab(r(80, 0, 20, 20), r(0, 0, 80, 20), r(0, 20, 100, 20));
  await act(async () => { drop.resolve({ place: { changed: true, dry_run: false, pane_id: "a", previous_pane_id: "a", placed_rect: placed.panes[0].rect, focused_pane_id: "a", target_layout: placed } }); });
  expect(last()).toMatchObject({ phase: "idle", end: "settle" });
  expect(clip("a").classList.contains("lifted")).toBe(false);
  await settleTimers(40);
  expect(settles()).toEqual(["a", "b", "c"]);
  await settleTimers(400);
  const settled = ["a", "b", "c"].map(drawn);
  expect(settled[0]).toEqual(["translate(800px, 0px)", "200px", "399px"]);
  // Only the coalesced final snapshot arrives; it matches the reply, so nothing moves again.
  render(placed);
  await settleTimers(40);
  expect(["a", "b", "c"].map(drawn)).toEqual(settled);
  expect(last()).toMatchObject({ phase: "idle", end: "settle" });
});

it("settles a swap whose snapshot lands before the reply exactly once, never drawing the final layout before the settle starts", async () => {
  const render = await mount();
  await dropAOn(B_CENTRE);
  film = []; shoot();
  const swapped = tab(r(50, 0, 50, 20), r(0, 0, 50, 40), r(50, 20, 50, 20));
  render(swapped); shoot();
  await roll(40);
  expect(settles()).toEqual([]);
  expect(last().phase).toBe("dropped");
  await act(async () => { drop.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: swapped } }); });
  shoot();
  expect(last()).toMatchObject({ phase: "idle", end: "settle" });
  await roll(40);
  expect(settles()).toEqual(["a", "b"]);
  // The same layout again (a coalesced repeat) changes nothing mid-settle.
  render(swapped); shoot();
  await roll(400);
  // One settle: frozen, then moving to the final frame, then at rest there. The final frame never shows before it.
  expect(film).toEqual([[...FROZEN_A, "false"], ["translate(500px, 0px)", "500px", "399px", "true"], ["translate(500px, 0px)", "500px", "399px", "false"]]);
  expect(chipSprangBack()).toBe(false);
  const later = tab(r(60, 0, 40, 20), r(0, 0, 60, 40), r(60, 20, 40, 20));
  render(later);
  await settleTimers(40);
  expect(settles()).toEqual([]);
  expect(drawn("a")).toEqual(["translate(600px, 0px)", "400px", "399px"]);
});

it("settles to the reply when the only snapshot during the pending drop moved no pane, as a status change does", async () => {
  const render = await mount();
  await dropAOn(B_CENTRE);
  film = []; shoot();
  render(three); // same geometry, a new snapshot object
  await roll(40);
  const swapped = tab(r(50, 0, 50, 20), r(0, 0, 50, 40), r(50, 20, 50, 20));
  await act(async () => { drop.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: swapped } }); });
  shoot();
  await roll(400);
  render(swapped); shoot(); // the drop's own snapshot, after the settle: nothing moves
  await roll(40);
  expect(film).toEqual([[...FROZEN_A, "false"], ["translate(500px, 0px)", "500px", "399px", "true"], ["translate(500px, 0px)", "500px", "399px", "false"]]);
});

it("ends a drop with no reply in 4 s quietly, shows the buffered snapshot as it is, and ignores the late reply", async () => {
  const render = await mount();
  await dropAOn(B_CENTRE);
  render(sourceResized);
  await settleTimers(3900);
  expect(drawn("a")).toEqual(FROZEN_A);
  expect(last().phase).toBe("dropped");
  await settleTimers(100);
  expect(last()).toMatchObject({ phase: "idle", end: "quiet" });
  expect(drawn("a")).toEqual(["translate(0px, 0px)", "399px", "800px"]);
  expect(settles()).toEqual([]);
  const calls = api.mock.calls.length, emitted = states.length;
  const swapped = tab(r(50, 0, 50, 20), r(0, 0, 50, 40), r(50, 20, 50, 20));
  await act(async () => { drop.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: swapped } }); });
  await settleTimers(400);
  expect(api.mock.calls.length).toBe(calls);
  expect(states.length).toBe(emitted);
  expect(drawn("a")).toEqual(["translate(0px, 0px)", "399px", "800px"]);
  expect(settles()).toEqual([]);
  expect(chipSprangBack()).toBe(false);
});

it("under Reduce Motion freezes the same way, then the settle is one fade per moved pane at its new frame", async () => {
  vi.stubGlobal("matchMedia", () => ({ matches: true }));
  const render = await mount();
  await dropAOn(B_CENTRE);
  const swapped = tab(r(50, 0, 50, 20), r(0, 0, 50, 40), r(50, 20, 50, 20));
  render(swapped);
  await settleTimers(40);
  expect(drawn("a")).toEqual(FROZEN_A);
  animate.mockClear();
  await act(async () => { drop.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: swapped } }); });
  expect(drawn("a")).toEqual(["translate(500px, 0px)", "500px", "399px"]);
  expect(clip("a").style.transition).not.toContain("transform");
  render(swapped);
  await settleTimers(400);
  const fades = animate.mock.calls.flatMap(([frames, options], i) => JSON.stringify(frames) === JSON.stringify([{ opacity: 0 }, { opacity: 1 }])
    ? [[(animate.mock.contexts[i] as HTMLElement).dataset.pane, (options as KeyframeAnimationOptions).duration]] : []);
  expect(fades.sort()).toEqual([["a", 100], ["b", 100]]);
});

it("ends a pending drop quietly when another tab is selected, so its chip and ghost leave with the tab", async () => {
  await mount();
  await dropAOn(B_RIGHT_BAND);
  expect(host.querySelector(".pane-drag-chip")).not.toBeNull();
  const other: Layout = { tab_id: "u", area: r(0, 0, 100, 40), panes: [{ pane_id: "d", rect: r(0, 0, 100, 40) }] };
  const both = { layouts: [three, other], tabs: [], panes: [...(snapshot(three).panes ?? []), { pane_id: "d", terminal_id: "term-d", tab_id: "u", workspace_id: "w" }] } as unknown as Snapshot;
  act(() => root!.render(<TabView snapshot={both} selected="u" machine="studio" focused={null} onFocus={() => {}} shortcut={() => false} register={() => {}} pin={() => {}} onDragChange={state => states.push(state)} />));
  await settleTimers(0);
  expect(last()).toMatchObject({ phase: "idle", end: "quiet" });
  await settleTimers(400); // the chip fades out
  expect(host.querySelector(".pane-drag-chip")).toBeNull();
  expect(host.querySelector(".pane-drop-zone")).toBeNull();
  expect(chipSprangBack()).toBe(false);
  // A late reply for the old tab changes nothing and sends nothing.
  const calls = api.mock.calls.length, emitted = states.length;
  await act(async () => { drop.resolve({ place: { changed: true, dry_run: false, pane_id: "a", focused_pane_id: "a", target_layout: three } }); });
  await settleTimers(400);
  expect(api.mock.calls.length).toBe(calls);
  expect(states.length).toBe(emitted);
  expect(last()).toMatchObject({ phase: "idle", end: "quiet" });
});

// Another client closes c, or moves it to tab u: the tab is left a | b.
const ab: Layout = { tab_id: "t", area: r(0, 0, 100, 40), panes: [{ pane_id: "a", rect: r(0, 0, 50, 40) }, { pane_id: "b", rect: r(50, 0, 50, 40) }] };
const paneOf = (pane_id: string, tab_id: string): Pane => ({ pane_id, terminal_id: "term-" + pane_id, tab_id, workspace_id: "w" });
const clips = () => [...host.querySelectorAll<HTMLElement>(".pane-clip")].map(el => el.dataset.pane).sort();
const SWAPPED = tab(r(50, 0, 50, 20), r(0, 0, 50, 40), r(50, 20, 50, 20));
// happy-dom's TransitionEvent is a plain Event, so the property name goes on as the browser would set it.
const transitionEnd = (id: string, propertyName: string) => act(() => { clip(id).dispatchEvent(Object.assign(new Event("transitionend", { bubbles: true }), { propertyName })); });

it.each([
  ["closed", { layouts: [ab], tabs: [], panes: [paneOf("a", "t"), paneOf("b", "t")] }],
  ["moved to another tab", { layouts: [ab, { tab_id: "u", area: r(0, 0, 100, 40), panes: [{ pane_id: "c", rect: r(0, 0, 100, 40) }] }], tabs: [], panes: [paneOf("a", "t"), paneOf("b", "t"), paneOf("c", "u")] }],
])("keeps every release-time box while a drop is pending, even a pane %s by another client", async (_how, foreign) => {
  await mount();
  await dropAOn(B_RIGHT_BAND);
  const release = ["a", "b", "c"].map(drawn);
  show(foreign as unknown as Snapshot);
  await settleTimers(40);
  expect(clips()).toEqual(["a", "b", "c"]);
  expect(["a", "b", "c"].map(drawn)).toEqual(release);
  expect(last().phase).toBe("dropped");
  // The reply ends the freeze. The pane set changed under the drop, so the buffered snapshot applies without motion.
  await act(async () => { drop.resolve({ place: { changed: true, dry_run: false, pane_id: "a", focused_pane_id: "a", target_layout: ab } }); });
  await settleTimers(40);
  expect(clips()).toEqual(["a", "b"]);
  expect(settles()).toEqual([]);
  expect(drawn("b")).toEqual(["translate(500px, 0px)", "500px", "800px"]);
});

it("ends a pending drop quietly when another pane is pressed, and the new drag holds the flushed boxes, never the old release", async () => {
  // The cap band's height, which the shell's tokens.css sets on the page: the press at y=410 is on c's cap.
  document.documentElement.style.setProperty("--shell-space-pane-cap-height", "36px");
  const render = await mount();
  await dropAOn(B_CENTRE);
  const first = drop;
  render(sourceResized);
  await settleTimers(40);
  expect(drawn("a")).toEqual(FROZEN_A);
  // A press on c's cap ends the pending swap quietly first: the buffered snapshot shows, with no cancel.
  fire(host.querySelector('[data-cap="c"]')!, "pointerdown", { button: 0, buttons: 1, clientX: 700, clientY: 410 });
  expect(states.some(s => s.phase === "idle" && s.end === "quiet")).toBe(true);
  expect(drawn("a")).toEqual(["translate(0px, 0px)", "399px", "800px"]);
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 600, clientY: 500 });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 200, clientY: 400 });
  await settleTimers(0);
  expect(last().sourceRect).toEqual({ x: 400, y: 400, width: 600, height: 400 }); // c in the buffered snapshot
  fire(window, "pointerup", { button: 0, buttons: 0, clientX: 200, clientY: 400 });
  expect(api).toHaveBeenLastCalledWith("studio", "pane.swap", { source_pane_id: "c", target_pane_id: "a" });
  expect(last().phase).toBe("dropped");
  await settleTimers(40);
  // The second drop freezes the boxes it was released over, not the first drop's.
  expect(drawn("a")).toEqual(["translate(0px, 0px)", "399px", "800px"]);
  // The first drop's late reply changes nothing.
  const emitted = states.length;
  await act(async () => { first.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: SWAPPED } }); });
  await settleTimers(40);
  expect(states.length).toBe(emitted);
  expect(drawn("a")).toEqual(["translate(0px, 0px)", "399px", "800px"]);
  document.documentElement.style.removeProperty("--shell-space-pane-cap-height");
});

it("ends a pending drop quietly when the host resizes, before any box rescales, and ignores the late reply", async () => {
  const render = await mount();
  await dropAOn(B_CENTRE);
  render(sourceResized);
  await settleTimers(40);
  expect(drawn("a")).toEqual(FROZEN_A);
  act(() => resizeHost(1200, 800));
  expect(last()).toMatchObject({ phase: "idle", end: "quiet" });
  // The buffered snapshot at the new size: a is 40 of 100 cells of 1200 px.
  expect(drawn("a")).toEqual(["translate(0px, 0px)", "479px", "800px"]);
  await settleTimers(40);
  expect(settles()).toEqual([]);
  const emitted = states.length;
  await act(async () => { drop.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: SWAPPED } }); });
  await settleTimers(400);
  expect(states.length).toBe(emitted);
  expect(drawn("a")).toEqual(["translate(0px, 0px)", "479px", "800px"]);
  expect(chipSprangBack()).toBe(false);
});

it("keeps a settle retargetable for as long as a retarget extends it", async () => {
  const render = await mount();
  await dropAOn(B_CENTRE);
  await act(async () => { drop.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: SWAPPED } }); });
  film = []; shoot();
  await roll(152);
  // Another client nudges the divider mid-settle: the settle retargets and now runs until about 400 ms.
  render(tab(r(60, 0, 40, 20), r(0, 0, 60, 40), r(60, 20, 40, 20))); shoot();
  await roll(152);
  // 300 ms in: past the first settle's end but inside the retarget's, so this snapshot retargets too.
  render(tab(r(70, 0, 30, 20), r(0, 0, 70, 40), r(70, 20, 30, 20))); shoot();
  await roll(400);
  expect(film).toEqual([
    ["translate(0px, 0px)", "499px", "800px", "false"], // the release frame, just accepted
    ["translate(500px, 0px)", "500px", "399px", "true"], // settling to the reply's layout
    ["translate(600px, 0px)", "400px", "399px", "true"], // retargeted
    ["translate(700px, 0px)", "300px", "399px", "true"], // retargeted again, still moving
    ["translate(700px, 0px)", "300px", "399px", "false"], // at rest
  ]);
});

it("ends a settle when every moving property has ended, not when the first one does", async () => {
  const render = await mount();
  await dropAOn(B_CENTRE);
  await act(async () => { drop.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: SWAPPED } }); });
  await settleTimers(80);
  expect(settles()).toEqual(["a", "b"]);
  // a moves, narrows and shortens; its width transition ends first.
  transitionEnd("a", "width");
  expect(settles()).toEqual(["a", "b"]);
  for (const property of ["transform", "height"]) transitionEnd("a", property);
  expect(settles()).toEqual(["b"]);
  for (const property of ["transform", "width", "height"]) transitionEnd("b", property);
  expect(settles()).toEqual([]);
  // The settle is over before its timer: the next snapshot applies as it is.
  render(tab(r(60, 0, 40, 20), r(0, 0, 60, 40), r(60, 20, 40, 20)));
  await settleTimers(40);
  expect(settles()).toEqual([]);
  expect(drawn("a")).toEqual(["translate(600px, 0px)", "400px", "399px"]);
});

it("lands a press that ends a pending drop on the cap the flushed layout draws there, as the Mac does", async () => {
  // The cap band's height, which the shell's tokens.css sets on the page.
  document.documentElement.style.setProperty("--shell-space-pane-cap-height", "36px");
  const render = await mount();
  await dropAOn(B_CENTRE);
  const first = drop;
  // The server swapped a and b; the snapshot is buffered while the reply is outstanding.
  render(SWAPPED);
  await settleTimers(40);
  expect(drawn("a")).toEqual(FROZEN_A);
  // A press on a's cap as drawn at the release. After the flush b's cap is there, so the drag is b's.
  fire(host.querySelector('[data-cap="a"]')!, "pointerdown", { button: 0, buttons: 1, clientX: 200, clientY: 10 });
  expect(last()).toMatchObject({ phase: "pressed", source: "b" });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 500, clientY: 500 });
  fire(window, "pointermove", { button: -1, buttons: 1, clientX: 750, clientY: 600 });
  await settleTimers(0);
  fire(window, "pointerup", { button: 0, buttons: 0, clientX: 750, clientY: 600 });
  expect(api).toHaveBeenLastCalledWith("studio", "pane.swap", { source_pane_id: "b", target_pane_id: "c" });
  const emitted = states.length;
  await act(async () => { first.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: SWAPPED } }); });
  expect(states.length).toBe(emitted);
  document.documentElement.style.removeProperty("--shell-space-pane-cap-height");
});

it("times a settle from when its transition really starts, so slow frames never cut it short", async () => {
  // 30 Hz: the move starts two frames, about 67 ms, after the reply.
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => setTimeout(() => callback(0), 33));
  const render = await mount();
  await dropAOn(B_CENTRE);
  await act(async () => { drop.resolve({ swap: { changed: true, source_pane_id: "a", focused_pane_id: "a", layout: SWAPPED } }); });
  film = []; shoot();
  await roll(256);
  // 256 ms after the reply: the transition began near 66 ms and runs to about 316 ms, so it is still on.
  expect(clip("a").style.transition).toContain("transform");
  // A snapshot now retargets the running settle rather than snapping.
  render(tab(r(60, 0, 40, 20), r(0, 0, 60, 40), r(60, 20, 40, 20))); shoot();
  await roll(400);
  expect(film).toEqual([
    ["translate(0px, 0px)", "499px", "800px", "false"],
    ["translate(500px, 0px)", "500px", "399px", "true"],
    ["translate(600px, 0px)", "400px", "399px", "true"],
    ["translate(600px, 0px)", "400px", "399px", "false"],
  ]);
});

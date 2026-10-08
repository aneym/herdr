import { useCallback, useLayoutEffect, useEffect, useMemo, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { Layout, Snapshot, Rect } from "./model";
import { scaleRect } from "./model";
import PaneSurface from "./PaneSurface";
import type { PaneController } from "./PaneTerm";
import { bridge } from "./bridge";
import { dividers, extent, Resizer } from "./dividers";
import type { Divider, ResizeAnswer } from "./dividers";
import { PaneDrag, canDragPane, probePlace, transitionFor, prefersReducedMotion } from "./paneDrag";
import type { PaneDragState } from "./paneDrag";
import { motion } from "./tokens";
import { clipRect, blockCancelContextMenu } from "./paneClip";
export default function TabView({ snapshot, selected, machine, focused, onFocus, shortcut, register, pin, onError, registerDrag, onDragChange, onSpring, online = true }: { onSpring?: (tabId: string) => void; online?: boolean; registerDrag?: (drag: PaneDrag | null, lift?: () => boolean) => void; onDragChange?: (state: PaneDragState) => void; snapshot: Snapshot; selected: string | null; machine: string; focused: string | null; onFocus: (id: string) => void; shortcut: (event: KeyboardEvent) => boolean; register: (id: string, value: PaneController | null) => void; pin: (id: string, pinned: boolean) => void; onError?: (error: unknown) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const [paneState, setPaneState] = useState<PaneDragState | null>(null);
  const callbacks = useRef({ onError, onDragChange, onSpring }); callbacks.current = { onError, onDragChange, onSpring };
  const [size, setSize] = useState({ width: 0, height: 0 });
  const sizeNow = useRef(size); sizeNow.current = size;
  // While a divider drag runs, panes follow the layouts pane.resize answers with, not snapshots:
  // a snapshot from before the last resize would snap the divider back.
  const [held, setHeld] = useState<Layout | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const snapshotLayout = snapshot.layouts?.find(l => l.tab_id === selected);
  const latestLayout = held?.tab_id === selected ? held : snapshotLayout;
  // A pending pane drop shows the tab as it was at the release; an accepted one shows its settle target until the next
  // snapshot (or held resize answer) replaces latestLayout.
  const paneDrag = useMemo(() => new PaneDrag(machine, { onChange: state => { setPaneState(state); callbacks.current.onDragChange?.(state); }, onError: error => callbacks.current.onError?.(error), onSpring: tabId => callbacks.current.onSpring?.(tabId) }), [machine]);
  const layout = paneDrag.shownLayout(latestLayout);
  const shown = useRef(layout); shown.current = layout;
  // A window resize, a sidebar toggle or a sidebar resize all reach the tab as a new host size. A pending drop ends
  // quietly before the new size renders, so no pane box rescales and no terminal fits while it is frozen.
  useEffect(() => {
    if (!host.current) return;
    const observer = new ResizeObserver(([entry]) => {
      const next = { width: entry.contentRect.width, height: entry.contentRect.height };
      if (next.width !== sizeNow.current.width || next.height !== sizeNow.current.height) paneDrag.hostResized();
      setSize(next);
    });
    observer.observe(host.current); return () => observer.disconnect();
  }, [paneDrag]);
  const release = useRef<ReturnType<typeof setTimeout>>();
  const resizer = useMemo(() => new Resizer(
    async (pane, direction, amount) => ((await bridge.api(machine, "pane.resize", { pane_id: pane, direction, amount })) as { resize?: ResizeAnswer } | null)?.resize ?? null,
    id => shown.current?.splits?.find(s => s.id === id)?.ratio,
    next => setHeld(next),
    // Snapshots were held back; catch up with the latest once the drag has settled.
    () => { clearTimeout(release.current); release.current = setTimeout(() => { if (!resizer.busy) setHeld(null); }, 200); },
  ), [machine]);
  const stop = useRef<(() => void) | null>(null);
  // A drag belongs to one tab on one machine: switching either, or unmounting, drops it.
  useEffect(() => () => { stop.current?.(); resizer.cancel(); clearTimeout(release.current); setHeld(null); setDragging(null); }, [selected, resizer]);
  const tabPanes = useMemo(() => (snapshot.panes ?? []).filter(p => p.tab_id === selected), [snapshot.panes, selected]);
  // A pending drop draws the panes the tab had at the release. A drop reply's layout can already lack a pane the older
  // snapshot still lists, as when it moved to another tab.
  const panes = paneDrag.shownPanes(tabPanes).filter(p => (!layout?.zoomed || p.pane_id === layout.focused_pane_id)
    && (layout === latestLayout || !!layout?.panes.some(lp => lp.pane_id === p.pane_id)));
  const lines = layout && !layout.zoomed ? dividers(layout) : [];
  const grab = (event: ReactPointerEvent<HTMLDivElement>, d: Divider) => {
    // A press that ended a pending drop hit a divider drawn where it stood at the release; the real one is elsewhere.
    if (event.button !== 0 || !layout || resizer.busy || paneState?.phase === "dragging" || flushedBy.current === event.nativeEvent) return;
    event.preventDefault();
    const el = event.currentTarget;
    try { el.setPointerCapture(event.pointerId); } catch { /* A synthetic pointer has nothing to capture; window listeners still follow it. */ }
    const start = d.vertical ? event.clientX : event.clientY;
    const extentPx = extent(d) * (d.vertical ? size.width / Math.max(1, layout.area.width) : size.height / Math.max(1, layout.area.height));
    const along = (e: PointerEvent) => (d.vertical ? e.clientX : e.clientY) - start;
    const move = (e: PointerEvent) => resizer.moved(d, along(e), extentPx);
    const up = (e: PointerEvent) => { done(); resizer.ended(d, along(e), extentPx); };
    const done = () => { window.removeEventListener("pointermove", move, true); window.removeEventListener("pointerup", up, true); window.removeEventListener("pointercancel", cancel, true); stop.current = null; setDragging(null); };
    const cancel = () => { done(); resizer.ended(d, 0, 0); };
    window.addEventListener("pointermove", move, true);
    window.addEventListener("pointerup", up, true);
    window.addEventListener("pointercancel", cancel, true);
    stop.current = done;
    clearTimeout(release.current);
    // Hold the layout from the press on, so no snapshot redraws the panes mid-drag.
    setHeld(layout);
    setDragging(d.splitId);
    resizer.began(d);
  };

  const [visual, setVisual] = useState<PaneDragState | null>(null);
  const priorVisual = useRef<PaneDragState | null>(null);
  const chip = useRef<HTMLDivElement>(null);
  const zoneOverlay = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = priorVisual.current; priorVisual.current = paneState;
    if (!paneState) return;
    if (paneState.phase === "idle" && previous && ["dragging", "dropped"].includes(previous.phase)) {
      const cancelled = paneState.end === "cancel";
      const duration = cancelled ? motion.cancelMs : motion.fadeMs;
      const reduced = prefersReducedMotion();
      const ease = getComputedStyle(document.documentElement).getPropertyValue("--shell-motion-ease").trim();
      zoneOverlay.current?.animate([{ opacity: 1 }, { opacity: 0 }], { duration: reduced ? motion.reducedFadeMs : duration, easing: ease, fill: "forwards" });
      if (chip.current) {
        const destination = previous.sourceRect;
        chip.current.animate(cancelled && !reduced && destination && previous.pointer ? [{ opacity: 1, transform: "translate(0, 0)" }, { opacity: 0, transform: `translate(${destination.x - previous.pointer.x}px, ${destination.y - previous.pointer.y}px)` }] : [{ opacity: 1 }, { opacity: 0 }], { duration: reduced ? motion.reducedFadeMs : duration, easing: ease, fill: "forwards" });
      }
      const timer = setTimeout(() => setVisual(null), reduced ? motion.reducedFadeMs : duration);
      return () => clearTimeout(timer);
    }
    setVisual(paneState);
  }, [paneState]);
  useEffect(() => {
    if (visual?.phase !== "dragging" || !chip.current) return;
    const ease = getComputedStyle(document.documentElement).getPropertyValue("--shell-motion-ease").trim();
    chip.current.animate(prefersReducedMotion() ? [{ opacity: 0 }, { opacity: 1 }] : [{ opacity: 0, scale: .96 }, { opacity: 1, scale: 1 }], { duration: prefersReducedMotion() ? motion.reducedFadeMs : motion.fadeMs, easing: ease });
  }, [visual?.phase]);
  const support = useRef(new Map<string, boolean>());
  const probes = useRef(new Map<string, Promise<boolean | null>>());
  const [supported, setSupported] = useState(false);
  useEffect(() => {
    let alive = true;
    const cached = support.current.get(machine); setSupported(cached ?? false);
    const pane = layout?.panes[0]?.pane_id;
    if (cached === undefined && pane && online) {
      let probe = probes.current.get(machine);
      if (!probe) { probe = probePlace(machine, pane); probes.current.set(machine, probe); }
      void probe.then(value => { probes.current.delete(machine); if (value !== null) { support.current.set(machine, value); if (alive) setSupported(value); } });
    }
    return () => { alive = false; };
  }, [machine, layout, online]);
  // Before paint, so a drop reply landing next sees the snapshot this render shows.
  useLayoutEffect(() => { paneDrag.panesChanged(tabPanes); }, [paneDrag, tabPanes]);
  useLayoutEffect(() => { paneDrag.layoutChanged(latestLayout); }, [paneDrag, latestLayout]);
  // A press anywhere on the panes ends a pending drop quietly before the press is handled: a cap press then starts its
  // drag from the flushed layout, a terminal press focuses as usual.
  const flushedBy = useRef<Event | null>(null);
  const pressFirst = (event: ReactPointerEvent<HTMLElement>) => { if (paneDrag.state.phase === "dropped") { flushedBy.current = event.nativeEvent; paneDrag.cancel(); } };
  // The settle is over once every pane box that moved has stopped.
  const moving = useRef(new Set<string>());
  const onMotion = useCallback((id: string, on: boolean) => {
    if (on) moving.current.add(id);
    else if (moving.current.delete(id) && moving.current.size === 0) paneDrag.settleEnded();
  }, [paneDrag]);
  useEffect(() => { if (!online) paneDrag.cancel(); }, [online, paneDrag]);
  const latest = useRef({ layout, size, focused, supported, panes }); latest.current = { layout, size, focused, supported, panes };
  useEffect(() => {
    registerDrag?.(paneDrag, () => {
      const l = latest.current; const pane = l.panes.find(p => p.pane_id === l.focused);
      return !!pane && !!l.layout && paneDrag.lift({ pane: pane.pane_id, label: pane.title ?? pane.agent ?? "shell", layout: l.layout, size: l.size, supported: l.supported });
    });
    return () => { paneStop.current?.(); paneDrag.cancel(); registerDrag?.(null); };
  }, [paneDrag, registerDrag]);
  const paneStop = useRef<(() => void) | null>(null);
  const contextStop = useRef<(() => void) | null>(null);
  useEffect(() => () => contextStop.current?.(), []);
  useEffect(() => { if (paneState?.phase === "idle") paneStop.current?.(); }, [paneState?.phase]);
  const capPress = (event: ReactPointerEvent<HTMLDivElement>, id: string, label: string) => {
    if (event.button !== 0 || !layout || resizer.busy || !host.current) return;
    const bounds = host.current.getBoundingClientRect();
    const point = (e: { clientX: number; clientY: number }) => ({ x: e.clientX - bounds.left, y: e.clientY - bounds.top });
    if (!paneDrag.press({ pane: id, label, point: point(event), layout, size, supported })) return;
    const el = event.currentTarget;
    try { el.setPointerCapture(event.pointerId); } catch { /* Synthetic control events have no capture. */ }
    const move = (e: PointerEvent) => {
      // A right press while the left is held arrives as a chorded pointermove, never a pointerdown.
      if (e.button === 2 || e.buttons & 2) { rightCancel(e, !(e.buttons & 2)); return; }
      const hit = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-tab], [data-space]");
      paneDrag.move(point(e), hit?.dataset.tab && snapshot.tabs?.some(t => t.tab_id === hit.dataset.tab) ? { kind: "tab", tab_id: hit.dataset.tab } : hit?.dataset.space ? { kind: "space", workspace_id: hit.dataset.space } : null);
      if (paneDrag.state.phase === "dragging") { e.preventDefault(); e.stopPropagation(); }
    };
    const done = () => { window.removeEventListener("pointermove", move, true); window.removeEventListener("pointerup", up, true); window.removeEventListener("pointercancel", cancel, true); window.removeEventListener("pointerdown", right, true); window.removeEventListener("contextmenu", context, true); paneStop.current = null; try { el.releasePointerCapture(event.pointerId); } catch { /* Already released. */ } };
    const up = (e: PointerEvent) => {
      if (e.button === 0) { const result = paneDrag.release(); done(); if (result !== "click") { e.preventDefault(); e.stopPropagation(); } return; }
      // The last button up ends the drag even when it is not the left one: the left went up in a chord.
      if (e.buttons === 0) { if (e.button === 2) rightCancel(e, true); else { cancel(); e.preventDefault(); e.stopPropagation(); } }
    };
    const cancel = () => { paneDrag.cancel(); done(); };
    // The context-menu blocker outlives the drag until that right button's release.
    const rightCancel = (e: PointerEvent, releasing = false) => { e.preventDefault(); e.stopPropagation(); contextStop.current?.(); contextStop.current = blockCancelContextMenu(window, releasing); cancel(); };
    const right = (e: PointerEvent) => { if (e.button === 2) rightCancel(e); };
    const context = (e: Event) => { e.preventDefault(); };
    window.addEventListener("pointermove", move, true); window.addEventListener("pointerup", up, true); window.addEventListener("pointercancel", cancel, true); window.addEventListener("pointerdown", right, true); window.addEventListener("contextmenu", context, true); paneStop.current = done;
  };
  const boxes = panes.map((pane, index) => {
    const rect = layout?.panes.find(p => p.pane_id === pane.pane_id)?.rect;
    return layout?.zoomed ? { x: 0, y: 0, ...size } : rect && layout ? scaleRect(rect, layout.area, size.width, size.height) : { x: 0, y: size.height * index / panes.length, width: size.width, height: size.height / panes.length };
  });
  // As the Mac's PaneCap: the tab's pin sits on the top-right pane's cap only.
  const corner = boxes.reduce((best, b, i) => best < 0 || b.y < boxes[best].y - .5 || (Math.abs(b.y - boxes[best].y) <= .5 && b.x + b.width > boxes[best].x + boxes[best].width) ? i : best, -1);
  const tab = snapshot.tabs?.find(t => t.tab_id === selected);
  const animateLayout = paneDrag.shouldAnimateLayout(layout, size, resizer.busy || !!held);
  return <main ref={host} onPointerDownCapture={pressFirst} className={`tab-view ${paneState?.phase === "dragging" ? "pane-dragging" : ""}`}>{panes.map((pane, index) => {
    const box = boxes[index];
    return <PaneClip key={`${selected}:${pane.terminal_id}`} id={pane.pane_id} box={clipRect(box, size)} animateLayout={animateLayout} onMotion={onMotion} lifted={paneState?.source === pane.pane_id && ["dragging", "dropped"].includes(paneState.phase)}>{settling => <PaneSurface settling={settling} grabbable={canDragPane(layout, supported)} onCapPointerDown={event => capPress(event, pane.pane_id, pane.title ?? pane.agent ?? "shell")} onError={onError} hasAgent={!!pane.agent || !!snapshot.agents?.some(a => a.pane_id === pane.pane_id && a.agent)} pane={pane} machine={machine} focused={focused === pane.pane_id} onFocus={onFocus} shortcut={shortcut} register={register} {...(index === corner && tab ? { pinned: tab.pin_index != null, onPin: () => pin(tab.tab_id, tab.pin_index == null) } : {})} />}</PaneClip>;
  })}{layout && lines.map(d => {
    const r = scaleRect({ x: d.vertical ? d.pos : d.splitRect.x, y: d.vertical ? d.splitRect.y : d.pos, width: d.vertical ? 0 : d.splitRect.width, height: d.vertical ? d.splitRect.height : 0 }, layout.area, size.width, size.height);
    // The 1 px gap between panes sits just before the line; the grab strip centres on it.
    const style = d.vertical ? { left: r.x - 4.5, top: r.y, width: 7, height: r.height } : { left: r.x, top: r.y - 4.5, width: r.width, height: 7 };
    return <div key={d.splitId} data-split={d.splitId} className={`divider ${d.vertical ? "vertical" : "horizontal"} ${dragging === d.splitId ? "dragging" : ""}`} style={style} onPointerDown={event => grab(event, d)} />;
  })}{visual?.ghost && <div ref={zoneOverlay} className="pane-drop-zone" style={{ transform: `translate(${visual.ghost.x}px, ${visual.ghost.y}px)`, width: `calc(${visual.ghost.width}px - var(--shell-motion-zone-inset) * 2)`, height: `calc(${visual.ghost.height}px - var(--shell-motion-zone-inset) * 2)`, margin: "var(--shell-motion-zone-inset)", transition: transitionFor("zone") }} />}{visual && ["dragging", "dropped"].includes(visual.phase) && (visual.pointer || visual.ghost) && <div ref={chip} className="pane-drag-chip" style={{ left: `calc(${visual.pointer?.x ?? visual.ghost!.x}px + var(--shell-motion-chip-offset))`, top: `calc(${visual.pointer?.y ?? visual.ghost!.y}px + var(--shell-motion-chip-offset))`, transition: transitionFor("lift") }}>{visual.label}</div>}</main>;
}

// Animate the clip only: terminal content takes its final size without scaling glyphs.
// A settle starts from the box drawn last. A box change during a running settle retargets it: the CSS transition carries
// on from the clip's current frame to the new box, never snapping or restarting from the start. It ends when every
// property the latest retarget moved has finished its transition, or a settle's duration after that retarget.
export function PaneClip({ id, box, lifted, animateLayout, onMotion, children }: { id: string; box: Rect; lifted: boolean; animateLayout: boolean; onMotion?: (id: string, moving: boolean) => void; children: (settling: boolean) => React.ReactNode }) {
  const clip = useRef<HTMLDivElement>(null);
  const previous = useRef(box);
  // The frame the running transition heads for, and the properties whose transition toward it has not ended.
  const heading = useRef(box);
  const running = useRef(new Set<string>());
  const moving = useRef(false);
  const frames = useRef<number[]>([]);
  const timer = useRef<ReturnType<typeof setTimeout>>();
  const motionChanged = useRef(onMotion); motionChanged.current = onMotion;
  const [frame, setFrame] = useState(box);
  const [settling, setSettling] = useState(false);
  const [animate, setAnimate] = useState(false);
  const stop = () => { frames.current.forEach(cancelAnimationFrame); frames.current = []; clearTimeout(timer.current); };
  const begin = () => { if (!moving.current) { moving.current = true; motionChanged.current?.(id, true); } };
  const ended = () => {
    stop(); running.current.clear(); setSettling(false); setAnimate(false);
    if (moving.current) { moving.current = false; motionChanged.current?.(id, false); }
  };
  const headFor = (next: Rect) => {
    const from = heading.current;
    if (from.x !== next.x || from.y !== next.y) running.current.add("transform");
    if (from.width !== next.width) running.current.add("width");
    if (from.height !== next.height) running.current.add("height");
    heading.current = next; setAnimate(true); setFrame(next);
  };
  useEffect(() => () => { stop(); if (moving.current) motionChanged.current?.(id, false); }, []);
  useLayoutEffect(() => {
    const old = previous.current; previous.current = box;
    if (JSON.stringify(old) === JSON.stringify(box)) return;
    if (!animateLayout) { heading.current = box; setFrame(box); ended(); return; }
    if (prefersReducedMotion()) {
      // The reduced settle: the panes take their new frames at once and fade in, once per settle.
      if (!moving.current) clip.current?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: motion.reducedFadeMs });
      stop(); begin(); heading.current = box; setFrame(box); setAnimate(true);
      timer.current = setTimeout(ended, motion.reducedFadeMs);
      return;
    }
    clearTimeout(timer.current);
    if (!moving.current) {
      // Start: hold the drawn frame, then move to the newest target two frames in, once the transition is on.
      begin(); heading.current = old; setFrame(old); setAnimate(false); setSettling(true);
      frames.current = [requestAnimationFrame(() => { frames.current = [requestAnimationFrame(() => { frames.current = []; headFor(previous.current); })]; })];
    } else if (frames.current.length === 0) headFor(box);
    // A change before the move began needs nothing: the move reads the newest target when it starts.
    timer.current = setTimeout(ended, motion.settleMs + 50);
  }, [box.x, box.y, box.width, box.height]);
  const transitionEnded = (event: React.TransitionEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget || !running.current.delete(event.propertyName)) return;
    if (running.current.size === 0) ended();
  };
  return <div ref={clip} data-pane={id} className={`pane-box pane-clip ${lifted ? "lifted" : ""}`} onTransitionEnd={transitionEnded} style={{ left: 0, top: 0, transform: `translate(${frame.x}px, ${frame.y}px)`, width: frame.width, height: frame.height, transition: animate ? transitionFor("settle") : undefined }}><div style={{ position: "relative", width: box.width, height: box.height }}>{children(settling)}</div></div>;
}

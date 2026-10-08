import { useLayoutEffect, useEffect, useMemo, useRef, useState } from "react";
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
export default function TabView({ snapshot, selected, machine, focused, onFocus, shortcut, register, pin, onError, registerDrag, onDragChange, online = true }: { online?: boolean; registerDrag?: (drag: PaneDrag | null, lift?: () => boolean) => void; onDragChange?: (state: PaneDragState) => void; snapshot: Snapshot; selected: string | null; machine: string; focused: string | null; onFocus: (id: string) => void; shortcut: (event: KeyboardEvent) => boolean; register: (id: string, value: PaneController | null) => void; pin: (id: string, pinned: boolean) => void; onError?: (error: unknown) => void }) {
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  useEffect(() => { if (!host.current) return; const observer = new ResizeObserver(([entry]) => setSize({ width: entry.contentRect.width, height: entry.contentRect.height })); observer.observe(host.current); return () => observer.disconnect(); }, []);
  // While a divider drag runs, panes follow the layouts pane.resize answers with, not snapshots:
  // a snapshot from before the last resize would snap the divider back.
  const [held, setHeld] = useState<Layout | null>(null);
  const [dragging, setDragging] = useState<string | null>(null);
  const snapshotLayout = snapshot.layouts?.find(l => l.tab_id === selected);
  const layout = held?.tab_id === selected ? held : snapshotLayout;
  const shown = useRef(layout); shown.current = layout;
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
  const panes = (snapshot.panes ?? []).filter(p => p.tab_id === selected && (!layout?.zoomed || p.pane_id === layout.focused_pane_id));
  const lines = layout && !layout.zoomed ? dividers(layout) : [];
  const [paneState, setPaneState] = useState<PaneDragState | null>(null);
  const grab = (event: ReactPointerEvent<HTMLDivElement>, d: Divider) => {
    if (event.button !== 0 || !layout || resizer.busy || paneState?.phase === "dragging") return;
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
      const cancelled = previous.phase === "dragging";
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
  const callbacks = useRef({ onError, onDragChange }); callbacks.current = { onError, onDragChange };
  const paneDrag = useMemo(() => new PaneDrag(machine, { onChange: state => { setPaneState(state); callbacks.current.onDragChange?.(state); }, onError: error => callbacks.current.onError?.(error) }), [machine]);
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
  useEffect(() => { paneDrag.layoutChanged(layout); }, [paneDrag, layout]);
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
  useEffect(() => { if (paneState?.phase === "idle") paneStop.current?.(); }, [paneState?.phase]);
  const capPress = (event: ReactPointerEvent<HTMLDivElement>, id: string, label: string) => {
    if (event.button !== 0 || !layout || resizer.busy || !host.current) return;
    const bounds = host.current.getBoundingClientRect();
    const point = (e: { clientX: number; clientY: number }) => ({ x: e.clientX - bounds.left, y: e.clientY - bounds.top });
    if (!paneDrag.press({ pane: id, label, point: point(event), layout, size, supported })) return;
    const el = event.currentTarget;
    try { el.setPointerCapture(event.pointerId); } catch { /* Synthetic control events have no capture. */ }
    const move = (e: PointerEvent) => {
      const hit = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-tab], [data-space]");
      paneDrag.move(point(e), hit?.dataset.tab ? { kind: "tab", tab_id: hit.dataset.tab } : hit?.dataset.space ? { kind: "space", workspace_id: hit.dataset.space } : null);
      if (paneDrag.state.phase === "dragging") { e.preventDefault(); e.stopPropagation(); }
    };
    const done = () => { window.removeEventListener("pointermove", move, true); window.removeEventListener("pointerup", up, true); window.removeEventListener("pointercancel", cancel, true); window.removeEventListener("pointerdown", right, true); window.removeEventListener("contextmenu", context, true); paneStop.current = null; try { el.releasePointerCapture(event.pointerId); } catch { /* Already released. */ } };
    const up = (e: PointerEvent) => { if (e.button !== 0) return; const result = paneDrag.release(); done(); if (result !== "click") { e.preventDefault(); e.stopPropagation(); } };
    const cancel = () => { paneDrag.cancel(); done(); };
    const right = (e: PointerEvent) => { if (e.button === 2) { e.preventDefault(); e.stopPropagation(); cancel(); } };
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
  return <main ref={host} className={`tab-view ${paneState?.phase === "dragging" ? "pane-dragging" : ""}`}>{panes.map((pane, index) => {
    const box = boxes[index];
    return <PaneClip key={`${selected}:${pane.terminal_id}`} id={pane.pane_id} box={box} lifted={paneState?.source === pane.pane_id && ["dragging", "dropped"].includes(paneState.phase)}>{settling => <PaneSurface settling={settling} grabbable={canDragPane(layout, supported)} onCapPointerDown={event => capPress(event, pane.pane_id, pane.title ?? pane.agent ?? "shell")} onError={onError} hasAgent={!!pane.agent || !!snapshot.agents?.some(a => a.pane_id === pane.pane_id && a.agent)} pane={pane} machine={machine} focused={focused === pane.pane_id} onFocus={onFocus} shortcut={shortcut} register={register} {...(index === corner && tab ? { pinned: tab.pin_index != null, onPin: () => pin(tab.tab_id, tab.pin_index == null) } : {})} />}</PaneClip>;
  })}{layout && lines.map(d => {
    const r = scaleRect({ x: d.vertical ? d.pos : d.splitRect.x, y: d.vertical ? d.splitRect.y : d.pos, width: d.vertical ? 0 : d.splitRect.width, height: d.vertical ? d.splitRect.height : 0 }, layout.area, size.width, size.height);
    // The 1 px gap between panes sits just before the line; the grab strip centres on it.
    const style = d.vertical ? { left: r.x - 4.5, top: r.y, width: 7, height: r.height } : { left: r.x, top: r.y - 4.5, width: r.width, height: 7 };
    return <div key={d.splitId} data-split={d.splitId} className={`divider ${d.vertical ? "vertical" : "horizontal"} ${dragging === d.splitId ? "dragging" : ""}`} style={style} onPointerDown={event => grab(event, d)} />;
  })}{visual?.ghost && <div ref={zoneOverlay} className="pane-drop-zone" style={{ transform: `translate(${visual.ghost.x}px, ${visual.ghost.y}px)`, width: `calc(${visual.ghost.width}px - var(--shell-motion-zone-inset) * 2)`, height: `calc(${visual.ghost.height}px - var(--shell-motion-zone-inset) * 2)`, margin: "var(--shell-motion-zone-inset)", transition: transitionFor("zone") }} />}{visual && ["dragging", "dropped"].includes(visual.phase) && (visual.pointer || visual.ghost) && <div ref={chip} className="pane-drag-chip" style={{ left: `calc(${visual.pointer?.x ?? visual.ghost!.x}px + var(--shell-motion-chip-offset))`, top: `calc(${visual.pointer?.y ?? visual.ghost!.y}px + var(--shell-motion-chip-offset))`, transition: transitionFor("lift") }}>{visual.label}</div>}</main>;
}

// Animate the clip only: terminal content takes its final size without scaling glyphs.
function PaneClip({ id, box, lifted, children }: { id: string; box: Rect; lifted: boolean; children: (settling: boolean) => React.ReactNode }) {
  const clip = useRef<HTMLDivElement>(null);
  const previous = useRef(box);
  const [frame, setFrame] = useState(box);
  const [settling, setSettling] = useState(false);
  const [animate, setAnimate] = useState(false);
  useLayoutEffect(() => {
    const old = previous.current; previous.current = box;
    if (JSON.stringify(old) === JSON.stringify(box)) return;
    if (prefersReducedMotion()) {
      setFrame(box); setAnimate(true);
      clip.current?.animate([{ opacity: 0 }, { opacity: 1 }], { duration: motion.reducedFadeMs });
      return;
    }
    setFrame(old); setAnimate(false); setSettling(true);
    let second = 0;
    const first = requestAnimationFrame(() => { second = requestAnimationFrame(() => { setAnimate(true); setFrame(box); }); });
    const timer = setTimeout(() => setSettling(false), motion.settleMs + 50);
    return () => { cancelAnimationFrame(first); cancelAnimationFrame(second); clearTimeout(timer); };
  }, [box.x, box.y, box.width, box.height]);
  return <div ref={clip} data-pane={id} className={`pane-box pane-clip ${lifted ? "lifted" : ""}`} onTransitionEnd={event => { if (event.target === event.currentTarget) setSettling(false); }} style={{ left: 0, top: 0, transform: `translate(${frame.x}px, ${frame.y}px)`, width: frame.width, height: frame.height, transition: animate ? transitionFor("settle") : undefined }}><div style={{ position: "relative", width: box.width, height: box.height }}>{children(settling)}</div></div>;
}

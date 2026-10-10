// Auto take control (Alex, 2026-10-10: "take control should jsut automatically happen in all
// clients instead of me having to click the mneu button when we have focus"), as on the Mac
// (SurfaceRegistry.takeControl). Input on a pane this client does not control is queued; the
// client takes the terminal once whoever holds it has had no human input for SETTLE_MS, then
// writes the queue in order. Input arriving meanwhile joins the queue, so nothing is lost or
// reordered, and a controller that is still typing is not cut off mid-line.
export const SETTLE_MS = 1500;
const MAX_QUEUED = 4096;

export interface AutoControlPort {
  /** Milliseconds since the last human input reached the pane from any client; null when unknown. */
  holderInputAgeMs: () => Promise<number | null>;
  /** Take the terminal; true when this client now controls it. */
  take: () => Promise<boolean>;
  write: (bytes: Uint8Array) => Promise<void>;
  /** The holder is typing: the take waits about this long. */
  waiting?: (ms: number) => void;
}

export function autoControl(port: AutoControlPort) {
  const queue: Uint8Array[] = [];
  let busy = false;
  const run = async () => {
    busy = true;
    try {
      for (;;) {
        const age = await port.holderInputAgeMs().catch(() => null);
        if (age == null || age >= SETTLE_MS) break;
        port.waiting?.(SETTLE_MS - age);
        await new Promise(resolve => setTimeout(resolve, SETTLE_MS - age + 50));
      }
      if (!(await port.take())) { queue.length = 0; return; }
      while (queue.length) await port.write(queue.shift()!);
    } finally { busy = false; }
  };
  return {
    /** True from the first queued input until the queue has been written. */
    get busy() { return busy; },
    /** Queue `bytes` (none for a click) and take the terminal unless a take is already running. */
    input(bytes?: Uint8Array): Promise<void> {
      if (bytes && queue.length < MAX_QUEUED) queue.push(bytes);
      return busy ? Promise.resolve() : run();
    },
  };
}

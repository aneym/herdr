import { useEffect, useState } from "react";
import { bridge, fromBase64 } from "./bridge";
import { AGENTS_DIR, faceDot, faceHover, parseCard } from "./faces";
import type { AgentCard, Face } from "./faces";
// The standing agents' cards live on the machine beside lanes.json (herdr serves no agent name or
// picture), so they come over the file helper, as the docs column's files do.
export function useAgentCards(machine: string, up: boolean): Record<string, AgentCard> {
  const [cards, setCards] = useState<Record<string, AgentCard>>({});
  useEffect(() => {
    let disposed = false, busy = false, stamp = "";
    if (!up) return;
    const poll = async () => {
      if (disposed || busy || document.hidden) return;
      busy = true;
      try {
        const names = (await bridge.fileList(machine, AGENTS_DIR)).sort();
        if (disposed) return;
        // A folder without a card, or an unreadable one, names no row; the rest still load.
        const texts = await Promise.all(names.map(name => bridge.fileRead(machine, `${AGENTS_DIR}/${name}/agent.json`, 0, 65536)
          .then(chunk => new TextDecoder().decode(fromBase64(chunk.data_b64)), () => "")));
        const next: Record<string, AgentCard> = {};
        // Sorted folder order owns precedence: the first valid card for a pane wins, as on Mac.
        for (const parsed of texts.map(parseCard))
          if (parsed && !Object.prototype.hasOwnProperty.call(next, parsed.pane)) next[parsed.pane] = parsed.card;
        const nextStamp = JSON.stringify(next);
        if (!disposed && nextStamp !== stamp) { stamp = nextStamp; setCards(next); }
      } catch { /* No agents folder or helper down: rows keep their tinted initials. */ }
      finally { busy = false; }
    };
    void poll(); const timer = setInterval(() => void poll(), 10000);
    document.addEventListener("visibilitychange", poll);
    return () => { disposed = true; clearInterval(timer); document.removeEventListener("visibilitychange", poll); };
  }, [machine, up]);
  return cards;
}
/** The face in the row's 16pt glyph slot, with the state dot cut into its lower right. */
export default function AgentFace({ face, status, request }: { face: Face; status: string; request?: string }) {
  const [broken, setBroken] = useState<string | null>(null);
  const picture = face.avatar && broken !== face.avatar ? face.avatar : null;
  const dot = faceDot(status, request), hover = faceHover(status, request);
  return <span className={`face ${dot ? "has-dot" : ""}`} title={hover || undefined} aria-label={hover || undefined} data-face={`${face.initial}:${face.tint}${picture ? ":picture" : ""}`} data-dot={dot ?? ""}>
    {picture
      ? <span className="face-disc picture"><img src={picture} alt="" referrerPolicy="no-referrer" onError={() => setBroken(picture)} /></span>
      : <span className={`face-disc face-tint-${face.tint}`}><span className="face-initial">{face.initial}</span></span>}
    {dot && <span className={`face-dot ${dot}`} />}
  </span>;
}

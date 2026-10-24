import { useState } from "react";
import { faceDot, faceHover } from "./faces";
import type { Face } from "./faces";
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

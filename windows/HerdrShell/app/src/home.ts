interface HomeFacts { class: "owner" | "lane" | "utility"; state: "needs_you" | "working" | "finished"; sub: string; order: number; progress_at?: number | null }
interface HomeSnapshot { tabs?: { tab_id: string }[]; overlay?: { tabs?: Record<string, { home?: HomeFacts; parent?: string }> } }

export function homeSection(home: HomeFacts): string {
  if (home.class === "utility") return "UTILITIES";
  return home.state === "needs_you" ? "NEEDS YOU" : home.state === "working" ? "WORKING" : "FINISHED";
}

export function compareHomeTags(a: HomeFacts | undefined, b: HomeFacts | undefined): number {
  return (a?.order ?? 0) - (b?.order ?? 0);
}

export function representedTabIds(snapshot: HomeSnapshot): Set<string> {
  const tags = snapshot.overlay?.tabs ?? {};
  const represented = new Set(Object.entries(tags).filter(([, tag]) => tag.home).map(([id]) => id));
  return new Set((snapshot.tabs ?? []).filter(tab => {
    const seen = new Set<string>();
    let id: string | undefined = tab.tab_id;
    while (id && !seen.has(id)) {
      if (represented.has(id)) return true;
      seen.add(id);
      id = tags[id]?.parent;
    }
    return false;
  }).map(tab => tab.tab_id));
}

function progressAge(progressAt: number | null | undefined, now: number): string {
  if (progressAt == null || !Number.isFinite(progressAt)) return "unknown";
  const minutes = Math.max(0, Math.floor((now / 1000 - progressAt) / 60));
  return minutes === 0 ? "<1m" : minutes < 60 ? `${minutes}m` : `${Math.floor(minutes / 60)}h${minutes % 60}m`;
}

export function homeTooltip(home: HomeFacts, now: number): string {
  return [home.sub, `last progress ${progressAge(home.progress_at, now)}`].filter(Boolean).join(" · ");
}

export interface HomePresentation { id: string; section: string; at_epoch_ms: number }

export function observeHomePresentation() {
  const samples: HomePresentation[] = [];
  const current = new Map<string, string>();
  const scan = () => {
    for (const row of document.querySelectorAll<HTMLElement>('.home-section [data-row^="home:"]')) {
      const id = row.dataset.row!.slice(5);
      const section = row.closest('.home-section')?.querySelector('h2 span')?.textContent ?? '';
      if (current.get(id) === section) continue;
      const box = row.getBoundingClientRect();
      const clip = row.closest('nav')?.getBoundingClientRect();
      if (!clip || box.height === 0 || box.bottom <= clip.top || box.top >= clip.bottom) continue;
      current.set(id, section);
      const sample = { id, section, at_epoch_ms: 0 };
      requestAnimationFrame(() => requestAnimationFrame(() => {
        if (!row.isConnected || row.closest('.home-section')?.querySelector('h2 span')?.textContent !== section) return;
        sample.at_epoch_ms = performance.timeOrigin + performance.now();
        samples.push(sample);
        if (samples.length > 200) samples.shift();
      }));
    }
  };
  const observer = new MutationObserver(scan);
  observer.observe(document.body, { childList: true, subtree: true, characterData: true });
  scan();
  return { samples, stop: () => observer.disconnect() };
}

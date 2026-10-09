(() => {
  if (window === window.top) return;
  const kind = "herdr-desk-external-link";
  const send = href => window.parent.postMessage({ kind, href }, "*");
  // Relay through direct parents so the Shell can authenticate its desk iframe
  // as the sender even when a page embeds another cross-origin frame.
  window.addEventListener("message", event => {
    if (event.data?.kind !== kind || typeof event.data.href !== "string" || !/^(https?:|mailto:)/i.test(event.data.href)) return;
    for (let i = 0; i < window.frames.length; i++) {
      if (event.source === window.frames[i]) { send(event.data.href); break; }
    }
  });
  window.addEventListener("click", event => {
    if (!event.ctrlKey || !event.shiftKey || event.button !== 0) return;
    const anchor = event.composedPath().find(node => node instanceof Element && node.matches("a[href]"));
    if (!anchor || !/^(https?:|mailto:)/i.test(anchor.href)) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    send(anchor.href);
  }, true);
})();

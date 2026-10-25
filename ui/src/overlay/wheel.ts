// While ⌃ is held (the click-hold bind) every wheel event arrives with ctrlKey=true. Chromium/WebView2 treats that as
// page ZOOM and WebKit may too, so the text areas would not scroll. We cancel the default and scroll the nearest
// scrollable ancestor under the pointer ourselves (deltaMode-aware).

const LINE_PX = 16;

/** Wheel delta in pixels along y (`deltaMode`: 0 pixel, 1 line, 2 page). */
export function wheelPixels(deltaY: number, deltaMode: number, pageSize: number): number {
  return deltaMode === 1 ? deltaY * LINE_PX : deltaMode === 2 ? deltaY * pageSize : deltaY;
}

/** The nearest ancestor (or self) that can scroll vertically. */
export function scrollableFrom(start: Element | null): HTMLElement | null {
  for (let n = start; n; n = n.parentElement) {
    if (!(n instanceof HTMLElement)) continue;
    const oy = getComputedStyle(n).overflowY;
    if ((oy === 'auto' || oy === 'scroll') && n.scrollHeight > n.clientHeight) return n;
  }
  return null;
}

export function onCtrlWheel(e: WheelEvent) {
  if (!e.ctrlKey) return;
  e.preventDefault(); // no zoom
  const el = scrollableFrom(e.target as Element | null);
  if (el) el.scrollBy(0, wheelPixels(e.deltaY, e.deltaMode, el.clientHeight));
}

export function installCtrlWheel(): () => void {
  window.addEventListener('wheel', onCtrlWheel, { passive: false, capture: true });
  return () => window.removeEventListener('wheel', onCtrlWheel, { capture: true });
}

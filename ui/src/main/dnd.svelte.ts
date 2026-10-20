// F9: drag & drop for calls, built on pointer events (not HTML5 DnD: Tauri's native file-drop handler
// swallows HTML5 drags on some platforms, and pointer events are testable and uniform in WKWebView/WebView2).
// Drop targets mark themselves with `data-drop="<folder key>"` and read `dnd.over` for the highlight.
import { folderKey } from './api';
import { library, ui } from './state.svelte';

export type DragCall = { id: string; title: string };

class Dnd {
  call = $state<DragCall | null>(null);
  x = $state(0);
  y = $state(0);
  /** Folder key under the pointer (never the call's own folder). */
  over = $state<string | null>(null);
  get active(): boolean { return this.call !== null; }
}
export const dnd = new Dnd();

const THRESHOLD = 6;
const EDGE = 56;

function scroller(node: HTMLElement | null): HTMLElement | null {
  for (let p = node; p; p = p.parentElement) {
    const oy = getComputedStyle(p).overflowY;
    if ((oy === 'auto' || oy === 'scroll') && p.scrollHeight > p.clientHeight) return p;
  }
  return null;
}
const INTERACTIVE = 'button, select, input, textarea, [role=menu]';

function target(x: number, y: number, id: string): string | null {
  const key = (document.elementFromPoint(x, y) as HTMLElement | null)?.closest<HTMLElement>('[data-drop]')?.dataset.drop ?? null;
  const c = library.call(id);
  return key && c && key !== folderKey(c.folder) ? key : null;
}

/** Svelte action for a call row: drag to a folder, right-click / Menu key opens «Переместить в…». */
export function callRow(node: HTMLElement, getCall: () => (DragCall & { movable?: boolean }) | null) {
  let start: { x: number; y: number; call: DragCall; pid: number } | null = null;
  let dragging = false;
  let raf = 0;
  let last = { x: 0, y: 0 };

  /** Long lists: dragging near the top/bottom edge of the scrolling area scrolls it, so a call in Recent can reach a folder card. */
  function autoscroll() {
    raf = 0;
    if (!dragging || !start) return;
    const sc = scroller(document.elementFromPoint(last.x, last.y) as HTMLElement | null) ?? scroller(node);
    if (sc) {
      const r = sc.getBoundingClientRect();
      const d = last.y < r.top + EDGE ? -(1 - (last.y - r.top) / EDGE) : last.y > r.bottom - EDGE ? 1 - (r.bottom - last.y) / EDGE : 0;
      if (d) {
        sc.scrollTop += Math.round(Math.max(-1, Math.min(1, d)) * 16);
        dnd.over = target(last.x, last.y, start.call.id);
      }
    }
    raf = requestAnimationFrame(autoscroll);
  }

  const swallowClick = (e: Event) => { e.preventDefault(); e.stopPropagation(); };

  function finish(drop: boolean) {
    window.removeEventListener('pointermove', move, true);
    window.removeEventListener('pointerup', up, true);
    window.removeEventListener('pointercancel', cancel, true);
    window.removeEventListener('keydown', key, true);
    document.documentElement.classList.remove('dnd-on');
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    const call = dnd.call;
    const over = dnd.over;
    dnd.call = null;
    dnd.over = null;
    if (dragging) {
      // The click that follows pointerup must not open the call.
      window.addEventListener('click', swallowClick, { capture: true, once: true });
      setTimeout(() => window.removeEventListener('click', swallowClick, true), 0);
    }
    if (drop && dragging && call && over) void library.move(call.id, over);
    start = null;
    dragging = false;
  }
  function move(e: PointerEvent) {
    if (!start || e.pointerId !== start.pid) return;
    if (!dragging) {
      if (Math.hypot(e.clientX - start.x, e.clientY - start.y) < THRESHOLD) return;
      dragging = true;
      dnd.call = start.call;
      document.documentElement.classList.add('dnd-on');
      getSelection()?.removeAllRanges();
    }
    dnd.x = e.clientX;
    dnd.y = e.clientY;
    last = { x: e.clientX, y: e.clientY };
    dnd.over = target(e.clientX, e.clientY, start.call.id);
    if (!raf) raf = requestAnimationFrame(autoscroll);
    e.preventDefault();
  }
  const up = (e: PointerEvent) => { if (start && e.pointerId === start.pid) finish(true); };
  const cancel = () => finish(false);
  function key(e: KeyboardEvent) { if (e.key === 'Escape') { e.stopPropagation(); finish(false); } }

  function down(e: PointerEvent) {
    const call = getCall();
    if (!call || call.movable === false || e.button !== 0 || (e.target as HTMLElement).closest(INTERACTIVE)) return;
    start = { x: e.clientX, y: e.clientY, call: { id: call.id, title: call.title }, pid: e.pointerId };
    window.addEventListener('pointermove', move, true);
    window.addEventListener('pointerup', up, true);
    window.addEventListener('pointercancel', cancel, true);
    window.addEventListener('keydown', key, true);
  }
  function menu(e: MouseEvent) {
    const call = getCall();
    if (!call || call.movable === false) return;
    e.preventDefault();
    // Keyboard-triggered menus (Menu key / Shift+F10) report 0,0: anchor to the row instead.
    const r = node.getBoundingClientRect();
    ui.openMove(call.id, e.clientX || r.left + 16, e.clientY || r.bottom);
  }
  node.addEventListener('pointerdown', down);
  node.addEventListener('contextmenu', menu);
  node.setAttribute('draggable', 'false');
  node.style.userSelect = 'none';
  node.style.setProperty('-webkit-user-select', 'none');
  return {
    destroy() {
      node.removeEventListener('pointerdown', down);
      node.removeEventListener('contextmenu', menu);
      if (start) finish(false);
    },
  };
}

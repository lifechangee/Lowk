// Auto-follow for the overlay's two text areas (subtitle history, AI answer).
// Keeps the newest text in view while it grows; if the person scrolls up (wheel / trackpad, only possible while
// the overlay is clickable) following pauses, and resumes after `idleMs` without scrolling (Infinity = only when
// they scroll back to the end themselves). Reports `{ overflow, atEnd }` so the component can show «ещё ↓».
export interface FollowOpts {
  idleMs: number;
  onState?: (s: { overflow: boolean; atEnd: boolean }) => void;
}

/** Pixels of slack that still count as "at the end" (sub-pixel layout, fractional scrollTop). */
const SLACK = 4;

export function follow(node: HTMLElement, initial: FollowOpts) {
  let opts = initial;
  let pinned = true;
  let lastSet = -1;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let last = '';

  const dist = () => node.scrollHeight - node.clientHeight - node.scrollTop;
  const report = () => {
    const s = { overflow: node.scrollHeight - node.clientHeight > SLACK, atEnd: dist() <= SLACK };
    const key = `${s.overflow}${s.atEnd}`;
    if (key === last) return;
    last = key;
    opts.onState?.(s);
  };
  const toEnd = () => {
    node.scrollTop = node.scrollHeight;
    lastSet = node.scrollTop;
    report();
  };
  const resume = () => {
    clearTimeout(timer);
    timer = undefined;
    pinned = true;
    toEnd();
  };
  const onScroll = () => {
    if (dist() <= SLACK) {
      pinned = true;
      clearTimeout(timer);
    } else {
      if (node.scrollTop === lastSet) return; // our own scroll, the content grew since
      pinned = false;
      clearTimeout(timer);
      if (Number.isFinite(opts.idleMs)) timer = setTimeout(resume, opts.idleMs);
    }
    report();
  };
  const onChange = () => {
    if (pinned) toEnd();
    else report();
  };

  node.setAttribute('data-follow', '');
  node.addEventListener('scroll', onScroll, { passive: true });
  node.addEventListener('lowk-jump', resume);
  const mo = new MutationObserver(onChange);
  mo.observe(node, { childList: true, subtree: true, characterData: true });
  const ro = new ResizeObserver(onChange);
  ro.observe(node);
  toEnd();

  return {
    update(o: FollowOpts) {
      opts = o;
    },
    destroy() {
      clearTimeout(timer);
      mo.disconnect();
      ro.disconnect();
      node.removeEventListener('scroll', onScroll);
      node.removeEventListener('lowk-jump', resume);
    },
  };
}

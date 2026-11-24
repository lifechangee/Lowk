// Auto-follow for the overlay's two text areas (subtitle history, AI answer).
// Keeps the newest text in view while it grows; if the person scrolls up (wheel / trackpad, only possible while
// the overlay is clickable) following pauses, and resumes after `idleMs` without scrolling (Infinity = only when
// they scroll back to the end themselves). Reports `{ overflow, atEnd }` so the component can show «ещё ↓».
export interface FollowOpts {
  idleMs: number;
  /** Follow the end of this text element; later siblings (message footer) never pull it out of view. */
  endSelector?: string;
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
  let lastEnd = -1;
  let lastHeight = -1;

  const textEnd = () => {
    const text = opts.endSelector ? node.querySelector<HTMLElement>(opts.endSelector) : null;
    return text ? text.getBoundingClientRect().bottom - node.getBoundingClientRect().top + node.scrollTop : node.scrollHeight;
  };

  const dist = () => node.scrollHeight - node.clientHeight - node.scrollTop;
  const report = () => {
    const s = { overflow: node.scrollHeight - node.clientHeight > SLACK, atEnd: dist() <= SLACK };
    const key = `${s.overflow}${s.atEnd}`;
    if (key === last) return;
    last = key;
    opts.onState?.(s);
  };
  const toEnd = (end = textEnd()) => {
    node.scrollTop = Math.max(0, end - node.clientHeight);
    lastSet = node.scrollTop;
    report();
  };
  const resume = () => {
    clearTimeout(timer);
    timer = undefined;
    pinned = true;
    toEnd(node.scrollHeight); // explicit jump also reveals the message footer
  };
  const onScroll = () => {
    if (textEnd() - node.clientHeight - node.scrollTop <= SLACK) {
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
    const end = textEnd();
    const changed = !opts.endSelector || Math.abs(end - lastEnd) > 0.5 || node.clientHeight !== lastHeight;
    lastEnd = end;
    lastHeight = node.clientHeight;
    // Footer mutations (including Copy -> Copied) must preserve the reader's position.
    if (pinned && changed) toEnd(end);
    else report();
  };

  node.setAttribute('data-follow', '');
  node.addEventListener('scroll', onScroll, { passive: true });
  node.addEventListener('lowk-jump', resume);
  const mo = new MutationObserver(onChange);
  mo.observe(node, { childList: true, subtree: true, characterData: true });
  const ro = new ResizeObserver(onChange);
  ro.observe(node);
  onChange();

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

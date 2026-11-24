import { afterEach, describe, expect, it, vi } from 'vitest';
import { follow } from './follow';

/** Minimal scroller geometry: exercise the action's event/observer contract without a browser. */
function scroller() {
  let mutate = () => {};
  let resize = () => {};
  vi.stubGlobal('MutationObserver', class {
    constructor(cb: () => void) { mutate = cb; }
    observe() {}
    disconnect() {}
  });
  vi.stubGlobal('ResizeObserver', class {
    constructor(cb: () => void) { resize = cb; }
    observe() {}
    disconnect() {}
  });
  class Scroller extends EventTarget {
    textHeight = 400;
    footerHeight = 0;
    clientHeight = 100;
    top = 0;
    get scrollHeight() { return Math.max(this.clientHeight, this.textHeight + this.footerHeight); }
    get scrollTop() { return this.top; }
    set scrollTop(value: number) { this.top = Math.max(0, Math.min(value, this.scrollHeight - this.clientHeight)); }
    setAttribute() {}
    getBoundingClientRect() { return { top: 20 }; }
    querySelector() { return { getBoundingClientRect: () => ({ bottom: 20 + this.textHeight - this.scrollTop }) }; }
    scrollTo(top: number) { this.scrollTop = top; this.dispatchEvent(new Event('scroll')); }
  }
  const node = new Scroller();
  const onState = vi.fn();
  const action = follow(node as unknown as HTMLElement, { idleMs: Infinity, endSelector: '.bt', onState });
  return { node, action, onState, mutate: () => mutate(), resize: () => resize() };
}

afterEach(() => vi.unstubAllGlobals());

describe('answer text follow', () => {
  it('follows growing text but never scrolls farther for the footer', () => {
    const { node, action, mutate, onState } = scroller();
    expect(node.scrollTop).toBe(300);
    node.textHeight += 50;
    mutate();
    expect(node.scrollTop).toBe(350);
    node.footerHeight = 100;
    mutate();
    expect(node.scrollTop).toBe(350);
    expect(onState).toHaveBeenLastCalledWith({ overflow: true, atEnd: false });
    action.destroy();
  });

  it('preserves the first line while the reader is scrolled up, including on completion/resize', () => {
    const { node, action, mutate, resize } = scroller();
    node.scrollTo(0);
    node.textHeight += 50;
    mutate();
    node.footerHeight = 100;
    mutate();
    node.clientHeight += 26; // cancel button disappears on completion
    resize();
    expect(node.scrollTop).toBe(0);
    action.destroy();
  });

  it('lets the reader scroll or jump into the footer and keeps their place when Copy changes', () => {
    const { node, action, mutate } = scroller();
    node.footerHeight = 100;
    mutate();
    node.dispatchEvent(new Event('lowk-jump'));
    expect(node.scrollTop).toBe(400);
    mutate(); // Copy -> Copied
    expect(node.scrollTop).toBe(400);
    node.scrollTo(350);
    mutate();
    expect(node.scrollTop).toBe(350);
    action.destroy();
  });

  it('re-pins at the text end, and keeps the text end visible when the viewport grows', () => {
    const { node, action, mutate, resize } = scroller();
    node.scrollTo(0);
    node.scrollTo(300);
    node.textHeight += 50;
    mutate();
    expect(node.scrollTop).toBe(350);
    node.clientHeight += 26;
    node.footerHeight = 100;
    resize();
    expect(node.scrollTop).toBe(324);
    action.destroy();
  });
});

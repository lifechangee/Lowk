// Drag -> rectangle in window pixels. The final clamp and points -> pixels conversion are Rust (lowk_capture::area).
export interface Box { x: number; y: number; w: number; h: number }
export type Pt = [number, number];

/** A drag shorter than this on both axes is a click. */
export const CLICK_PX = 5;

export function normalize(a: Pt, b: Pt): Box {
  return { x: Math.min(a[0], b[0]), y: Math.min(a[1], b[1]), w: Math.abs(a[0] - b[0]), h: Math.abs(a[1] - b[1]) };
}

export function isClick(a: Pt, b: Pt): boolean {
  return Math.abs(a[0] - b[0]) < CLICK_PX && Math.abs(a[1] - b[1]) < CLICK_PX;
}

/** Keep the box inside a `w` x `h` window. */
export function clamp(r: Box, w: number, h: number): Box {
  const x0 = Math.max(0, r.x), y0 = Math.max(0, r.y);
  const x1 = Math.min(w, r.x + r.w), y1 = Math.min(h, r.y + r.h);
  return { x: x0, y: y0, w: Math.max(0, x1 - x0), h: Math.max(0, y1 - y0) };
}

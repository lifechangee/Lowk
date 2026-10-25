// Download progress: monotonic, ≤ 4 Hz, with speed and ETA; out-of-order events never move it backwards.
import { describe, expect, it } from 'vitest';
import { locale } from '../lib/i18n/locale.svelte';
import { DL_MIN_INTERVAL_MS, DlTracker } from './dl';
import { fmtEta, fmtGb, fmtSpeed } from './fmt';

const MB = 1048576;
const ev = (done: number, total = 1308, file = 'a.gguf', phase: 'downloading' | 'verifying' | 'ready' | 'failed' = 'downloading') => ({ file, phase, done_bytes: done * MB, total_bytes: total * MB, message: null });

describe('DlTracker', () => {
  it('never goes backwards when events arrive out of order (129 130 129 ... 137 139)', () => {
    const t = new DlTracker();
    let last = 0;
    let now = 0;
    for (const d of [129, 130, 129, 128, 131, 137, 136, 139, 138]) {
      now += 300;
      const v = t.feed(ev(d), now);
      if (v) { expect(v.doneBytes).toBeGreaterThanOrEqual(last); last = v.doneBytes; }
    }
    expect(last).toBe(139 * MB);
    expect(t.view.pct).toBeGreaterThanOrEqual(10);
  });

  it('repaints at most 4 times a second', () => {
    const t = new DlTracker();
    let paints = 0;
    for (let i = 0; i < 100; i++) if (t.feed(ev(i), i * 10)) paints++; // 100 events in 1 s
    expect(paints).toBeLessThanOrEqual(1000 / DL_MIN_INTERVAL_MS + 1);
    expect(paints).toBeGreaterThanOrEqual(3);
  });

  it('sums several files and keeps each file monotonic', () => {
    const t = new DlTracker();
    t.feed(ev(100, 600, 'a'), 0);
    t.feed(ev(50, 700, 'b'), 300);
    const v = t.feed(ev(90, 600, 'a'), 600)!; // stale event for a
    expect(v.doneBytes).toBe(150 * MB);
    expect(v.totalBytes).toBe(1300 * MB);
  });

  it('smooths a speed and an ETA from the byte rate', () => {
    const t = new DlTracker();
    t.feed(ev(0), 0);
    let v = t.feed(ev(10), 1000)!; // 10 MB/s
    v = t.feed(ev(20), 2000)!;
    expect(v.speed / MB).toBeGreaterThan(8);
    expect(v.speed / MB).toBeLessThan(12);
    expect(v.eta).not.toBeNull();
    expect(v.eta!).toBeGreaterThan(100);
    expect(v.eta!).toBeLessThan(160);
  });

  it('verifying and failed are shown at once; a failure keeps the message', () => {
    const t = new DlTracker();
    t.feed(ev(100), 0);
    expect(t.feed(ev(1308, 1308, 'a', 'verifying'), 10)!.phase).toBe('verifying');
    const f = t.feed({ file: '', phase: 'failed', done_bytes: 0, total_bytes: 0, message: 'checksum' }, 20)!;
    expect(f.phase).toBe('failed');
    expect(f.message).toBe('checksum');
  });
});

describe('number formatting follows the language', () => {
  it('RU: 15.923527 -> «16 ГБ», 134.99377 -> «135 ГБ»; one decimal under 10', () => {
    locale.setting = 'ru';
    expect(fmtGb(15.923527)).toMatch(/^16\s?ГБ$|^16 ГБ$/);
    expect(fmtGb(134.99377)).toBe('135 ГБ');
    expect(fmtGb(1.4)).toBe('1,4 ГБ');
    expect(fmtSpeed(3.2 * MB)).toBe('3,2 МБ/с');
    expect(fmtEta(40)).toBe('40 с');
    expect(fmtEta(200)).toBe('4 мин');
  });
  it('EN: «16 GB», «135 GB», «3.2 MB/s»', () => {
    locale.setting = 'en';
    expect(fmtGb(15.923527)).toBe('16 GB');
    expect(fmtGb(134.99377)).toBe('135 GB');
    expect(fmtSpeed(3.2 * MB)).toBe('3.2 MB/s');
  });
});

// Download progress: monotonic, ≤ 4 Hz, with speed and ETA; out-of-order events never move it backwards.
import { describe, expect, it } from 'vitest';
import { locale } from '../lib/i18n/locale.svelte';
import { DL_MIN_INTERVAL_MS, DlTracker } from './dl';
import { fmtEta, fmtGb, fmtSpeed } from './fmt';

const MB = 1048576;
const ev = (done: number, total = 1308, file = 'a.gguf', phase: 'downloading' | 'retrying' | 'verifying' | 'repairing' | 'ready' | 'failed' = 'downloading') => ({ file, phase, done_bytes: done * MB, total_bytes: total * MB, message: null });

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

  it('shows the one figure of the whole install as it is (no per-file summing)', () => {
    // the backend sends the same grand total with every file: 3 files must not triple it
    const t = new DlTracker();
    t.feed(ev(700, 1372, 'nemotron.gguf'), 0);
    t.feed(ev(1000, 1372, 'silero.gguf'), 300);
    const v = t.feed(ev(1100, 1372, 'whisper.gguf'), 600)!;
    expect(v.totalBytes).toBe(1372 * MB);
    expect(v.doneBytes).toBe(1100 * MB);
  });

  it('the total stays the same from the first event to the last', () => {
    const t = new DlTracker();
    const totals = new Set<number>();
    let now = 0;
    for (const d of [0, 10, 300, 700, 900, 1372]) {
      now += 300;
      const v = t.feed(ev(d, 1372), now);
      if (v) totals.add(v.totalBytes);
    }
    expect([...totals]).toEqual([1372 * MB]);
  });

  it('speed is an average over the last seconds and gives an ETA; a stall lowers it instead of jumping to 0 and back', () => {
    const t = new DlTracker();
    t.feed(ev(0), 0);
    let v = t.feed(ev(20), 2000)!; // 10 MB/s
    v = t.feed(ev(40), 4000)!;
    expect(v.speed / MB).toBeCloseTo(10, 0);
    expect(v.eta!).toBeGreaterThan(120);
    expect(v.eta!).toBeLessThan(140);
    v = t.feed(ev(40), 5000)!; // one silent second
    expect(v.speed / MB).toBeGreaterThan(5);
    expect(v.speed / MB).toBeLessThan(10);
    v = t.feed(ev(67), 7000)!; // the burst after the stall
    expect(v.speed / MB).toBeGreaterThan(8);
    expect(v.speed / MB).toBeLessThan(15);
  });

  it('retrying and repairing are shown at once, with the bar held and no speed', () => {
    const t = new DlTracker();
    t.feed(ev(500), 0);
    let v = t.feed(ev(500, 1308, 'a.gguf', 'retrying'), 10)!;
    expect(v.phase).toBe('retrying');
    expect(v.speed).toBe(0);
    expect(v.doneBytes).toBe(500 * MB);
    v = t.feed(ev(0, 1308, 'a.gguf', 'repairing'), 20)!;
    expect(v.phase).toBe('repairing');
    expect(v.doneBytes).toBe(500 * MB); // never back down
    v = t.feed(ev(520), 1000)!;
    expect(v.phase).toBe('downloading');
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

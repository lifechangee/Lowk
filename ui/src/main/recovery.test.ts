// Recovery notice: names the call, says it was recovered, shows how much was saved, links to the call.
import { beforeEach, describe, expect, it } from 'vitest';
import { locale } from '../lib/i18n/locale.svelte';
import { recoveryNotice } from './recovery';

const ID = '01JC8Z3K6Q9X2M4N7R5T0V0101';
beforeEach(() => { locale.setting = 'ru'; });

describe('recoveryNotice', () => {
  it('says nothing when nothing was recovered', () => {
    expect(recoveryNotice([])).toBeNull();
  });
  it('names the call, the saved length and opens the call', () => {
    const n = recoveryNotice([{ id: ID, title: 'Планерка', duration_s: 754 }])!;
    expect(n.text).toContain('«Планерка»');
    expect(n.text).toContain('восстановили');
    expect(n.text).toContain('13 мин');
    expect(n.href).toBe(`#/call/${ID}`);
  });
  it('a call saved for under a minute is shown in seconds, an untitled one gets a name', () => {
    const n = recoveryNotice([{ id: ID, title: '  ', duration_s: 20 }])!;
    expect(n.text).toContain('20 сек');
    expect(n.text).toContain('Без названия');
  });
  it('several calls: the count and the latest call', () => {
    const n = recoveryNotice([{ id: 'a', title: 'A', duration_s: 100 }, { id: ID, title: 'B', duration_s: 3700 }])!;
    expect(n.text).toContain('2');
    expect(n.text).toContain('«B»');
    expect(n.text).toContain('1 ч 02 мин');
    expect(n.href).toBe(`#/call/${ID}`);
  });
  it('English', () => {
    locale.setting = 'en';
    const n = recoveryNotice([{ id: ID, title: 'Sync', duration_s: 754 }])!;
    expect(n.text).toBe('The call “Sync” was interrupted by a crash and has been recovered. Saved: 13 min.');
  });
});

import { describe, expect, it } from 'vitest';
import { en, ru } from '../lib/i18n';
import { BUILTIN_TEMPLATES, bindSlot, defaultId, draftOf, duplicate, fillNames, move, newTemplate, restoreBuiltins, validate, visible } from './templates';

const base = () => [...BUILTIN_TEMPLATES, { id: 'obj', names: { ru: 'Возражение', en: 'Objection' }, instruction: 'Ответь', vision: false, reasoning: null }];
const ids = (l: { id: string }[]) => l.map((t) => t.id);

describe('templates page logic', () => {
  it('hides the summary template and falls back to brief for a missing default', () => {
    expect(ids(visible(base()))).toEqual(['brief', 'complement', 'obj']);
    expect(defaultId(base(), 'obj')).toBe('obj');
    expect(defaultId(base(), 'gone')).toBe('brief');
  });
  it('the default template owns the ask_brief bind; brief gets ask_template when not default', () => {
    expect(bindSlot('brief', 'brief')).toEqual({ action: 'ask_brief' });
    expect(bindSlot('obj', 'obj')).toEqual({ action: 'ask_brief' });
    expect(bindSlot('brief', 'obj')).toEqual({ action: 'ask_template', template: 'brief' });
    expect(bindSlot('complement', 'brief')).toEqual({ action: 'ask_complement' });
    expect(bindSlot('obj', 'brief')).toEqual({ action: 'ask_template', template: 'obj' });
  });
  it('validates: non-empty name and instruction, unique names per language', () => {
    const d = (o: object) => ({ ...draftOf(newTemplate(base(), 1)), ...o });
    expect(validate(d({}), base(), 'en')).toEqual(['name', 'instruction']);
    expect(validate(d({ en: 'New', instruction: 'x' }), base(), 'en')).toEqual([]);
    expect(validate(d({ en: ' brief ', instruction: 'x' }), base(), 'en')).toEqual(['nameTaken']);
    expect(validate(d({ ru: 'КРАТКО', instruction: 'x' }), base(), 'ru')).toEqual(['nameTaken']);
    const own = draftOf(base()[3]!);
    expect(validate(own, base(), 'en')).toEqual([]);
    expect(fillNames(d({ ru: 'Только' }), 'ru')).toMatchObject({ ru: 'Только', en: 'Только' });
  });
  it('duplicates after the original with a free name, moves among visible ones, restores built-ins', () => {
    const dup = duplicate(base(), 'obj', 'c1');
    expect(ids(dup)).toEqual(['brief', 'complement', 'summary', 'obj', 'c1']);
    expect(dup[4]!.names).toEqual({ ru: 'Возражение (копия)', en: 'Objection (copy)' });
    expect(duplicate(dup, 'obj', 'c2')[4]!.names.en).toBe('Objection (copy)+');
    expect(ids(move(base(), 'obj', -1))).toEqual(['brief', 'obj', 'summary', 'complement']);
    expect(ids(move(base(), 'brief', -1))).toEqual(ids(base()));
    const edited = base().map((t) => (t.id === 'brief' ? { ...t, instruction: 'x' } : t)).filter((t) => t.id !== 'complement');
    const r = restoreBuiltins(edited);
    expect(r.find((t) => t.id === 'brief')!.instruction).toBe(BUILTIN_TEMPLATES[0]!.instruction);
    expect(ids(r)).toContain('complement');
    expect(ids(r)).toContain('obj');
  });
  it('has RU and EN strings for every template key', () => {
    for (const k of Object.keys(ru).filter((x) => x.startsWith('m.tpl.'))) expect(en[k as keyof typeof en], k).toBeTruthy();
  });
});

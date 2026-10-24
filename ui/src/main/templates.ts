// Pure helpers for Settings → Templates. The list order is the overlay's cycle order; `summary` (call summary prompt)
// lives in the same list but is never shown here, in the overlay or as a bind.
import type { Template } from '../lib/ipc';
import { HIDDEN_TEMPLATES, TEMPLATE_ACTIONS, type Slot } from './binds';

export type Lang = 'ru' | 'en';
export const BUILTIN_IDS = ['brief', 'complement', 'summary'] as const;
/** Cannot be deleted (their text can be edited; «Reset» restores the shipped text). */
export const isBuiltin = (id: string): boolean => (BUILTIN_IDS as readonly string[]).includes(id);

/** Mirrors `default_templates()` in `lowk-types` (the Rust side validates and owns the real defaults). */
export const BUILTIN_TEMPLATES: Template[] = [
  { id: 'brief', names: { ru: 'Кратко', en: 'Brief' }, instruction: 'Кратко ответь на вопрос, заданный в системе/приложении.', needs_my_answer: false, vision: false, reasoning: null },
  { id: 'complement', names: { ru: 'Дополни мой ответ', en: 'Complement my answer' }, instruction: 'Я уже ответил. Добавь недостающие детали, не повторяя сказанное.', needs_my_answer: true, vision: false, reasoning: null },
  { id: 'summary', names: { ru: 'Резюме', en: 'Summary' }, instruction: 'Сделай краткое резюме созвона: ключевые темы, решения, договорённости и следующие шаги. Отвечай на языке разговора.', needs_my_answer: false, vision: false, reasoning: null },
];

/** Templates the page manages, in cycle order. */
export const visible = (all: Template[]): Template[] => all.filter((t) => !HIDDEN_TEMPLATES.has(t.id));

export const nameOf = (t: Template, lang: Lang): string => (lang === 'ru' ? t.names.ru : t.names.en) || t.names.ru || t.names.en || t.id;

/** The template ⌘↵ asks: the configured default if it still exists, else brief, else the first one. */
export function defaultId(all: Template[], want: string | undefined): string {
  const vis = visible(all);
  return [want ?? '', 'brief'].find((i) => vis.some((t) => t.id === i)) ?? vis[0]?.id ?? 'brief';
}

/** Bind slot of a template row: the default template owns ⌘↵ (`ask_brief`); every other one its dedicated action or `ask_template`. */
export function bindSlot(id: string, def: string): Slot {
  if (id === def) return { action: 'ask_brief' };
  const a = TEMPLATE_ACTIONS[id];
  return a && a !== 'ask_brief' ? { action: a } : { action: 'ask_template', template: id };
}

export type Draft = { id: string; ru: string; en: string; instruction: string; vision: boolean; reasoning: boolean | null };
export const draftOf = (t: Template): Draft => ({ id: t.id, ru: t.names.ru, en: t.names.en, instruction: t.instruction, vision: !!t.vision, reasoning: t.reasoning ?? null });
export const applyDraft = (t: Template, d: Draft): Template => ({ ...t, names: { ru: d.ru.trim(), en: d.en.trim() }, instruction: d.instruction.trim(), vision: d.vision, reasoning: d.reasoning });

/** Name in the other language defaults to the typed one, so one field is enough. */
export function fillNames(d: Draft, lang: Lang): Draft {
  const a = (lang === 'ru' ? d.ru : d.en).trim();
  const other = (lang === 'ru' ? d.en : d.ru).trim();
  return lang === 'ru' ? { ...d, ru: a, en: other || a } : { ...d, en: a, ru: other || a };
}

export type DraftError = 'name' | 'instruction' | 'nameTaken';
/** Non-empty name and instruction; names unique (case-insensitive) per language among the other templates. */
export function validate(d: Draft, all: Template[], lang: Lang): DraftError[] {
  const f = fillNames(d, lang);
  const out: DraftError[] = [];
  if (!f.ru || !f.en) out.push('name');
  else if (visible(all).some((t) => t.id !== d.id && (t.names.ru.trim().toLowerCase() === f.ru.toLowerCase() || t.names.en.trim().toLowerCase() === f.en.toLowerCase()))) out.push('nameTaken');
  if (!d.instruction.trim()) out.push('instruction');
  return out;
}

export function newTemplate(all: Template[], now = Date.now()): Template {
  let id = `t${now.toString(36)}`;
  while (all.some((t) => t.id === id)) id += 'x';
  return { id, names: { ru: '', en: '' }, instruction: '', needs_my_answer: false, vision: false, reasoning: null };
}

/** Copy right after the original with a free « (копия)» / « (copy)» name. */
export function duplicate(all: Template[], id: string, newId: string): Template[] {
  const i = all.findIndex((t) => t.id === id);
  if (i < 0) return all;
  let ru = `${all[i]!.names.ru} (копия)`;
  let en = `${all[i]!.names.en} (copy)`;
  while (all.some((t) => t.names.ru.toLowerCase() === ru.toLowerCase() || t.names.en.toLowerCase() === en.toLowerCase())) { ru += '+'; en += '+'; }
  return [...all.slice(0, i + 1), { ...all[i]!, id: newId, names: { ru, en } }, ...all.slice(i + 1)];
}

/** Move one visible template up (-1) or down (+1) among the visible ones; hidden ones keep their place. */
export function move(all: Template[], id: string, dir: -1 | 1): Template[] {
  const vis = visible(all);
  const i = vis.findIndex((t) => t.id === id);
  const j = i + dir;
  if (i < 0 || j < 0 || j >= vis.length) return all;
  const a = all.indexOf(vis[i]!);
  const b = all.indexOf(vis[j]!);
  const out = [...all];
  [out[a], out[b]] = [out[b]!, out[a]!];
  return out;
}

/** «Сбросить к стандартным»: built-ins get the shipped text back (missing ones return); user templates stay. */
export function restoreBuiltins(all: Template[]): Template[] {
  const out = all.map((t) => BUILTIN_TEMPLATES.find((b) => b.id === t.id) ?? t);
  for (const b of BUILTIN_TEMPLATES) if (!out.some((t) => t.id === b.id)) out.push(b);
  return out;
}

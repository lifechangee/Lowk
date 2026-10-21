// Pure helpers for the bind editor: formatting, recording, defaults, conflict warnings.
import type { ActionId, BindSpec, MouseBtn, Mods, Os, Trigger } from '../lib/ipc';

export const ACTIONS_PRIMARY: ActionId[] = ['ask_brief', 'ask_complement', 'screenshot', 'screenshot_area', 'click_hold', 'toggle_interactive', 'toggle_overlay', 'panic_hide', 'toggle_record', 'move_hold'];
/** G6: one bind per template; `ask` (the template picked in the overlay chip) stays assignable. */
export const ACTIONS_EXTRA: ActionId[] = ['ask', 'next_template', 'prev_template', 'cancel_answer', 'clear_shots', 'toggle_main'];
export const ALL_ACTIONS: ActionId[] = [...ACTIONS_PRIMARY, ...ACTIONS_EXTRA];

const combo = (key: string, mods: Mods): Trigger => ({ kind: 'combo', mods, key });

/** Defaults mirror `Settings::defaults_for(Os)` in `lowk-types` (round 3 decisions). */
export function defaultBinds(os: Os): BindSpec[] {
  const m: Mods = os === 'macos' ? { meta: true } : { ctrl: true };
  const ms: Mods = { ...m, shift: true };
  return [
    { action: 'ask_brief', trigger: combo('Enter', m) },
    { action: 'ask_complement', trigger: combo('Enter', ms) },
    { action: 'screenshot', trigger: combo('KeyS', ms) },
    { action: 'screenshot_area', trigger: combo('KeyA', ms) },
    { action: 'toggle_interactive', trigger: combo('KeyI', ms) },
    { action: 'toggle_overlay', trigger: combo('Backslash', m) },
    { action: 'toggle_record', trigger: combo('KeyR', ms) },
    { action: 'move_hold', trigger: { kind: 'modifier_only', mods: { alt: true } } },
    { action: 'click_hold', trigger: { kind: 'modifier_only', mods: { ctrl: true } } },
  ];
}

/** Template ids that have a dedicated action (the same mapping as `ActionId::template_id` in lowk-types). */
export const TEMPLATE_ACTIONS: Record<string, ActionId> = { brief: 'ask_brief', complement: 'ask_complement', say: 'ask_say', factcheck: 'ask_factcheck', followup: 'ask_followup', recap: 'ask_recap' };
/** Never offered as a bind: used by the call summary. */
export const HIDDEN_TEMPLATES = new Set(['summary']);

/** A bindable slot: an action, plus the template id for `ask_template`. */
export type Slot = { action: ActionId; template?: string | null };
export const slotKey = (s: Slot): string => (s.template ? `${s.action}:${s.template}` : s.action);
export const sameSlot = (a: Slot, b: Slot): boolean => a.action === b.action && (a.template ?? null) === (b.template ?? null);

/** The slot of a template: its dedicated action when it has one, else `ask_template` + its id. */
export function templateSlot(id: string): Slot {
  const a = TEMPLATE_ACTIONS[id];
  return a ? { action: a } : { action: 'ask_template', template: id };
}

export function bindFor(binds: BindSpec[] | undefined, action: ActionId, template?: string | null): BindSpec | undefined {
  return binds?.find((b) => sameSlot(b, { action, template }));
}

/** Replace (or remove, with `trigger = null`) the bind of one slot; returns a new list. */
export function withBind(binds: BindSpec[], action: ActionId, trigger: Trigger | null, template?: string | null): BindSpec[] {
  const slot: Slot = { action, template };
  const rest = binds.filter((b) => !sameSlot(b, slot));
  if (!trigger) return rest;
  const prev = binds.find((b) => sameSlot(b, slot));
  const swallow = trigger.kind === 'mouse' ? true : (prev?.swallow ?? false);
  return [...rest, template ? { action, trigger, swallow, template } : { action, trigger, swallow }];
}

/** Binds whose template no longer exists (a deleted custom template): shown so they can be cleared. */
export function orphanTemplateBinds(binds: BindSpec[], templateIds: string[]): BindSpec[] {
  return binds.filter((b) => b.action === 'ask_template' && b.template && !templateIds.includes(b.template));
}

// ---- formatting ---------------------------------------------------------------------------

const MAC_ORDER: [keyof Mods, string][] = [['ctrl', '⌃'], ['alt', '⌥'], ['shift', '⇧'], ['meta', '⌘']];
const WIN_ORDER: [keyof Mods, string][] = [['ctrl', 'Ctrl'], ['alt', 'Alt'], ['shift', 'Shift'], ['meta', 'Win']];

export function keyLabel(code: string, os: Os): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^Numpad\d$/.test(code)) return 'Num ' + code.slice(6);
  const map: Record<string, string> = {
    Enter: os === 'macos' ? '↵' : 'Enter', Space: 'Space', Backslash: '\\', Slash: '/', Comma: ',', Period: '.', Semicolon: ';', Quote: "'", Backquote: '`',
    BracketLeft: '[', BracketRight: ']', Minus: '-', Equal: '=', Escape: 'Esc', Tab: 'Tab', Backspace: '⌫', Delete: 'Del',
    ArrowUp: '↑', ArrowDown: '↓', ArrowLeft: '←', ArrowRight: '→', Home: 'Home', End: 'End', PageUp: 'PgUp', PageDown: 'PgDn',
  };
  return map[code] ?? code;
}

export function formatMods(mods: Mods, os: Os): string[] {
  return (os === 'macos' ? MAC_ORDER : WIN_ORDER).filter(([k]) => mods[k]).map(([, s]) => s);
}

export type MouseNames = { middle: string; back: string; forward: string; other: (n: number) => string };

export function mouseName(b: MouseBtn, names: MouseNames): string {
  return b.kind === 'middle' ? names.middle : b.kind === 'back' ? names.back : b.kind === 'forward' ? names.forward : names.other(b.n);
}

/** Display parts: each part renders as one keycap. */
export function formatTrigger(tr: Trigger, os: Os, names: MouseNames): string[] {
  if (tr.kind === 'combo') return [...formatMods(tr.mods, os), keyLabel(tr.key, os)];
  if (tr.kind === 'modifier_only') return formatMods(tr.mods, os);
  return [...formatMods(tr.mods, os), mouseName(tr.button, names)];
}

/** Single-string form for compact hints («⌘⇧R»). */
export function triggerText(tr: Trigger, os: Os, names: MouseNames): string {
  return formatTrigger(tr, os, names).join(os === 'macos' ? '' : '+');
}

// ---- recording ----------------------------------------------------------------------------

type KeyLike = { code: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean; shiftKey: boolean };

export const isModifierCode = (code: string): boolean => /^(Shift|Control|Alt|Meta|OS)(Left|Right)?$/.test(code);

export function modsOf(e: Pick<KeyLike, 'metaKey' | 'ctrlKey' | 'altKey' | 'shiftKey'>): Mods {
  const m: Mods = {};
  if (e.metaKey) m.meta = true;
  if (e.ctrlKey) m.ctrl = true;
  if (e.altKey) m.alt = true;
  if (e.shiftKey) m.shift = true;
  return m;
}

export const hasMods = (m: Mods): boolean => !!(m.meta || m.ctrl || m.alt || m.shift);

/** Combo from a keydown. `null` for a bare modifier (it may become a modifier-only bind on release). */
export function triggerFromKeyEvent(e: KeyLike): Trigger | null {
  if (isModifierCode(e.code) || e.code === '' || e.code === 'Unidentified') return null;
  return { kind: 'combo', mods: modsOf(e), key: e.code };
}

/** Mouse button → bind. Primary and secondary buttons are not bindable. */
export function triggerFromMouse(button: number, mods: Mods): Trigger | null {
  if (button === 0 || button === 2) return null;
  const b: MouseBtn = button === 1 ? { kind: 'middle' } : button === 3 ? { kind: 'back' } : button === 4 ? { kind: 'forward' } : { kind: 'other', n: button + 1 };
  return { kind: 'mouse', button: b, mods };
}

export function triggerEquals(a: Trigger, b: Trigger): boolean {
  return JSON.stringify(norm(a)) === JSON.stringify(norm(b));
}
function norm(t: Trigger): unknown {
  const mods = { meta: !!t.mods.meta, ctrl: !!t.mods.ctrl, alt: !!t.mods.alt, shift: !!t.mods.shift };
  if (t.kind === 'combo') return { k: 'c', key: t.key, mods };
  if (t.kind === 'modifier_only') return { k: 'm', mods };
  return { k: 'x', b: t.button, mods };
}

// ---- warnings -----------------------------------------------------------------------------

export type BindWarning = 'global_enter' | 'save_as' | 'os_shortcut' | 'duplicate' | 'needs_modifier';

const MAC_RESERVED = ['Digit3', 'Digit4', 'Digit5'];

export function bindWarnings(spec: BindSpec, all: BindSpec[], os: Os): BindWarning[] {
  const out: BindWarning[] = [];
  const t = spec.trigger;
  if (t.kind === 'combo') {
    const primary = os === 'macos' ? t.mods.meta : t.mods.ctrl;
    if (t.key === 'Enter' && primary && !t.mods.shift && !t.mods.alt) out.push('global_enter');
    if (t.key === 'KeyS' && primary && t.mods.shift) out.push('save_as');
    if (os === 'macos' && t.mods.meta && t.mods.shift && MAC_RESERVED.includes(t.key)) out.push('os_shortcut');
    if (os === 'windows' && t.mods.meta && ['KeyX', 'KeyL', 'KeyD', 'KeyE', 'KeyR'].includes(t.key)) out.push('os_shortcut');
    if (!hasMods(t.mods) && /^(Key|Digit|Space|Enter)/.test(t.key)) out.push('needs_modifier');
  }
  if (all.some((o) => !sameSlot(o, spec) && triggerEquals(o.trigger, t))) out.push('duplicate');
  return out;
}

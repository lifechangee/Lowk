// Overlay view state. Fed by an OverlaySource (mock demo in dev, Tauri events in the app).
// No business logic: it only folds events into what the components render.
import type { LlmEvent, OverlayMode, OverlayVisibility, PillEdge, SessionPhase, SttEvent, Template, Tone, ToneKind } from '../lib/ipc/bindings';
import type { Lang } from '../lib/i18n';
import { PRESETS, kFromTransparency, toneToVars, type ToneVars } from './tone';

export interface SubLine {
  stream: 'me' | 'system';
  utt: number;
  text: string;
  partial: boolean;
}

/** Which link of the Question -> AI hint -> My answer chain is active. */
export type ChainStage = 'question' | 'hint' | 'my';

export const DEFAULT_TEMPLATES: Template[] = [
  { id: 'brief', names: { ru: 'Кратко ответить', en: 'Brief answer' }, instruction: '' },
  { id: 'complement', names: { ru: 'Дополни мой ответ', en: 'Complement my answer' }, instruction: '', needs_my_answer: true },
  { id: 'say', names: { ru: 'Что сказать?', en: 'What to say?' }, instruction: '' },
  { id: 'factcheck', names: { ru: 'Фактчек', en: 'Fact-check' }, instruction: '' },
  { id: 'followup', names: { ru: 'Follow-up вопросы', en: 'Follow-up questions' }, instruction: '' },
  { id: 'recap', names: { ru: 'Рекап за 5 минут', en: '5-minute recap' }, instruction: '' },
];

export class OverlayStore {
  lang = $state<Lang>('ru');
  mode = $state<OverlayMode>('passive');
  phase = $state<SessionPhase>('idle');
  /** Elapsed recording time in ms (the source or a ticker updates it). */
  elapsedMs = $state(0);
  tone = $state<ToneVars>(PRESETS.dark);
  toneKind = $state<ToneKind>('dark');
  /** 0..90 transparency slider value (`settings.overlay.opacity`). */
  transparency = $state(60);
  fontPx = $state(13);
  visibility = $state<OverlayVisibility>('recording_only');
  pillEdge = $state<PillEdge>('right');
  isMac = $state(true);
  /** `true` in the pill window, `false` in the overlay window; `null` = decided by `visibility` (dev stage). */
  pillWindow = $state<boolean | null>(null);
  /** The backend owns the «Мой ответ» text (`qa://my-answer`); do not accumulate it locally. */
  serverMyAnswer = false;
  /** Self-test sentinel: the overlay paints a magenta block while the app grabs the screen. */
  sentinel = $state(false);

  templates = $state<Template[]>(DEFAULT_TEMPLATES);
  templateId = $state('brief');
  menuOpen = $state(false);

  sub = $state<SubLine | null>(null);
  answer = $state('');
  streaming = $state(false);
  answerError = $state(false);
  hasAnswered = $state(false);
  model = $state('DeepSeek V4.1 Flash');
  myAnswer = $state('');
  /** True from the end of an AI answer until the next Ask: Me finals are collected as "My answer". */
  armed = $state(false);
  stage = $state<ChainStage>('question');

  shots = $state(0);
  thumbs = $state<string[]>([]);
  toast = $state<string | null>(null);

  k = $derived(kFromTransparency(this.transparency));
  template = $derived(this.templates.find((x) => x.id === this.templateId) ?? this.templates[0]);
  recording = $derived(this.phase === 'recording');
  /** Idle + `pill` visibility = the unobtrusive edge strip instead of the full overlay. */
  showPill = $derived(this.pillWindow ?? (this.visibility === 'pill' && this.phase !== 'recording' && this.phase !== 'paused'));

  #toastTimer: ReturnType<typeof setTimeout> | undefined;

  // ---- event folding (same shapes as the IPC DTOs) ----
  applyStt(e: SttEvent): void {
    const stream = e.stream;
    const partial = e.kind === 'partial';
    this.sub = { stream, utt: e.utt, text: e.text, partial };
    if (!partial) {
      if (stream === 'system' && this.stage !== 'hint') this.stage = 'question';
      if (stream === 'me' && this.armed && !this.serverMyAnswer) {
        this.myAnswer = (this.myAnswer ? this.myAnswer + ' ' : '') + e.text.trim();
        this.stage = 'my';
      }
    }
  }

  applyLlm(e: LlmEvent): void {
    if (e.type === 'start') {
      this.answer = '';
      this.answerError = false;
      this.streaming = true;
      this.hasAnswered = true;
      this.armed = false;
      this.myAnswer = '';
      this.stage = 'hint';
    } else if (e.type === 'delta') {
      this.answer += e.text;
    } else if (e.type === 'done') {
      this.streaming = false;
      this.armed = true;
    } else {
      this.streaming = false;
      this.answerError = true;
    }
  }

  applyMyAnswer(text: string): void {
    this.myAnswer = text;
    if (text) this.stage = 'my';
  }

  applyTone(t: Tone): void {
    this.toneKind = t.tone;
    this.tone = toneToVars(t);
  }

  setMode(m: OverlayMode): void {
    this.mode = m;
    if (m !== 'interactive') this.menuOpen = false;
  }

  setQueue(shots: number, thumbs: string[] = []): void {
    this.shots = shots;
    this.thumbs = thumbs.slice(-3);
  }

  flash(msg: string): void {
    this.toast = msg;
    clearTimeout(this.#toastTimer);
    this.#toastTimer = setTimeout(() => (this.toast = null), 1600);
  }

  selectTemplate(id: string): void {
    this.templateId = id;
    this.menuOpen = false;
  }

  reset(): void {
    this.sub = null;
    this.answer = '';
    this.streaming = false;
    this.answerError = false;
    this.hasAnswered = false;
    this.myAnswer = '';
    this.armed = false;
    this.stage = 'question';
    this.shots = 0;
    this.thumbs = [];
  }
}

/** `H:MM:SS` / `HH:MM:SS`, as in the mockup timer. */
export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const p = (n: number) => String(n).padStart(2, '0');
  return `${p(h)}:${p(m)}:${p(s % 60)}`;
}

/** Tiny `**bold**` splitter so the answer never goes through {@html}. */
export function splitBold(s: string): { text: string; bold: boolean }[] {
  const out: { text: string; bold: boolean }[] = [];
  const re = /\*\*(.+?)\*\*/gs;
  let last = 0;
  for (const m of s.matchAll(re)) {
    if (m.index > last) out.push({ text: s.slice(last, m.index), bold: false });
    out.push({ text: m[1]!, bold: true });
    last = m.index + m[0].length;
  }
  if (last < s.length) out.push({ text: s.slice(last), bold: false });
  return out;
}

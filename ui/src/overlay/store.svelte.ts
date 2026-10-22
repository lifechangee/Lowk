// Overlay view state. Fed by an OverlaySource (mock demo in dev, Tauri events in the app).
// No business logic: it only folds events into what the components render.
import type { SourceRef, LlmEvent, OverlayMode, OverlayVisibility, PillEdge, SessionPhase, SttEvent, Template, ToneKind } from '../lib/ipc/bindings';
import type { Lang } from '../lib/i18n';
import { keyLabels, type KeyLabels } from './keys';
import { GRID_COLS, GRID_ROWS, PRESETS, kFromTransparency, parseGrid, toneToVars, type ToneEvent, type ToneVars } from './tone';

/** An LLM event as it arrives; `seq` is the per-request delta counter the backend adds (optional). */
export type LlmIn = LlmEvent & { seq?: number };

export interface SubLine {
  stream: 'me' | 'system';
  utt: number;
  rev: number;
  text: string;
  partial: boolean;
}

/** How many subtitle lines the overlay keeps (the scrollable history; older ones are dropped). */
export const MAX_SUB_LINES = 60;

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
  /** Per-region tones (4x3, row-major) from `overlay://tone`; `null` = only the global tone is known. */
  grid = $state<ToneVars[] | null>(null);
  /** 0..90 transparency slider value (`settings.overlay.opacity`). */
  transparency = $state(60);
  fontPx = $state(13);
  visibility = $state<OverlayVisibility>('recording_only');
  pillEdge = $state<PillEdge>('right');
  isMac = $state(true);
  /** Real bind labels from `settings.binds` (empty string = unbound); defaults until settings arrive. */
  keysOverride = $state<Partial<KeyLabels> | null>(null);
  keys = $derived<KeyLabels>({ ...keyLabels(this.isMac), ...this.keysOverride });
  /** `true` in the pill window, `false` in the overlay window; `null` = decided by `visibility` (dev stage). */
  pillWindow = $state<boolean | null>(null);
  /** The backend owns the «Мой ответ» text (`qa://my-answer`); do not accumulate it locally. */
  serverMyAnswer = false;
  /** Self-test sentinel: the overlay paints a magenta block while the app grabs the screen. */
  sentinel = $state(false);
  /** Hold / modifier-only binds are off because Input Monitoring is missing (hook could not start). */
  holdDegraded = $state(false);

  templates = $state<Template[]>(DEFAULT_TEMPLATES);
  templateId = $state('brief');
  menuOpen = $state(false);

  /** Subtitle history, oldest first: finals plus the line being spoken. The newest line is always the last. */
  lines = $state<SubLine[]>([]);
  sub = $derived<SubLine | null>(this.lines.length ? this.lines[this.lines.length - 1]! : null);
  /** Engine failure of a stream (`stt://error`); cleared by the next transcript event. */
  sttError = $state<{ recoverable: boolean } | null>(null);
  /** The local speech model is still loading (`stt://loading`); audio is buffered and transcribed once it is up. */
  sttLoading = $state(false);
  /** The captured app's audio is gone and could not be re-acquired (`audio://notice`); cleared when it returns. */
  audioLost = $state(false);
  answer = $state('');
  streaming = $state(false);
  answerError = $state(false);
  /** `error.code` of the failed ask (`api_key_missing`, `local_only_blocked`, ...); null = generic. */
  errorCode = $state<string | null>(null);
  /** What the answer is grounded on (`llm://sources`), shown as chips under the answer. */
  sources = $state<SourceRef[]>([]);
  withheld = $state(0);
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
  /** The scrim cells (12x8): the grid, or the global tone everywhere when the backend sent none. */
  cells = $derived(this.grid ?? Array.from({ length: GRID_COLS * GRID_ROWS }, () => this.tone));
  template = $derived(this.templates.find((x) => x.id === this.templateId) ?? this.templates[0]);
  recording = $derived(this.phase === 'recording');
  /** Idle + `pill` visibility = the unobtrusive edge strip instead of the full overlay. */
  showPill = $derived(this.pillWindow ?? (this.visibility === 'pill' && this.phase !== 'recording' && this.phase !== 'paused'));

  #toastTimer: ReturnType<typeof setTimeout> | undefined;
  /** Request being streamed + last delta seq applied: events can arrive twice (F10 defence in depth). */
  #req: string | null = null;
  #seq = -1;

  // ---- event folding (same shapes as the IPC DTOs) ----
  applyStt(e: SttEvent): void {
    const stream = e.stream;
    const partial = e.kind === 'partial';
    this.sttError = null;
    // A revision of a line we already show (partial growth, the final, a Whisper correction) updates it IN PLACE: a
    // correction of an older utterance never replaces the line being spoken now. An utterance id we have not seen
    // is always a NEW line, even if its number is lower than before (engine restart / switch, ids start over):
    // dropping it by id is what froze the subtitles for good.
    let i = -1;
    for (let k = this.lines.length - 1; k >= 0; k--) {
      const l = this.lines[k]!;
      if (l.stream === stream && l.utt === e.utt) {
        i = k;
        break;
      }
    }
    if (i >= 0) {
      const cur = this.lines[i]!;
      if (e.rev < cur.rev || (partial && !cur.partial)) return; // stale revision
      this.lines[i] = { stream, utt: e.utt, rev: e.rev, text: e.text, partial };
    } else {
      // A newer utterance of the same stream means the earlier unfinished one is over (its final may never come).
      for (const l of this.lines) if (l.stream === stream && l.partial) l.partial = false;
      this.lines.push({ stream, utt: e.utt, rev: e.rev, text: e.text, partial });
      if (this.lines.length > MAX_SUB_LINES) this.lines.splice(0, this.lines.length - MAX_SUB_LINES);
    }
    if (!partial) {
      if (stream === 'system' && this.stage !== 'hint') this.stage = 'question';
      if (stream === 'me' && this.armed && !this.serverMyAnswer) {
        this.myAnswer = (this.myAnswer ? this.myAnswer + ' ' : '') + e.text.trim();
        this.stage = 'my';
      }
    }
  }

  applyLlm(e: LlmIn): void {
    if (e.type === 'start') {
      // A repeated `start` for the request already streaming must not wipe the text.
      if (this.#req === e.req_id) return;
      this.#req = e.req_id;
      this.#seq = -1;
      this.answer = '';
      this.answerError = false;
      this.errorCode = null;
      this.sources = [];
      this.withheld = 0;
      this.streaming = true;
      this.hasAnswered = true;
      this.armed = false;
      this.myAnswer = '';
      this.stage = 'hint';
    } else if (e.type === 'sources') {
      if (this.#req !== e.req_id) return;
      this.sources = e.sources;
      this.withheld = e.withheld;
    } else if (e.type === 'delta') {
      // Deltas of a finished/foreign request and deltas whose seq was already applied are dropped.
      if (this.#req !== e.req_id || !this.streaming) return;
      if (typeof e.seq === 'number') {
        if (e.seq <= this.#seq) return;
        this.#seq = e.seq;
      }
      this.answer += e.text;
    } else if (e.type === 'done') {
      if (!this.streaming || this.#req !== e.req_id) return;
      this.streaming = false;
      this.armed = true;
    } else {
      if (this.#req !== null && this.#req !== e.req_id) return;
      this.streaming = false;
      // a cancelled ask is not an error; a refused one says why (no key, local-only folder)
      if (e.error.code === 'llm_cancelled') return;
      this.answerError = true;
      this.errorCode = e.error.code;
    }
  }

  applyMyAnswer(text: string): void {
    this.myAnswer = text;
    if (text) this.stage = 'my';
  }

  applyTone(t: ToneEvent): void {
    this.toneKind = t.tone;
    const global = toneToVars(t);
    const grid = parseGrid(t);
    this.grid = grid;
    // The scrim floor must hold for the worst region, not just the average one.
    this.tone = grid ? { ...global, smin: Math.max(global.smin, ...grid.map((c) => c.smin)) } : global;
  }

  setMode(m: OverlayMode): void {
    this.mode = m;
    if (m !== 'interactive' && m !== 'hover') this.menuOpen = false;
  }

  setQueue(shots: number, thumbs: string[] = []): void {
    this.shots = shots;
    this.thumbs = thumbs.slice(-3);
    if (shots === 0) this.thumbs = [];
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
    this.lines = [];
    this.sttError = null;
    this.answer = '';
    this.streaming = false;
    this.answerError = false;
    this.errorCode = null;
    this.sources = [];
    this.withheld = 0;
    this.hasAnswered = false;
    this.myAnswer = '';
    this.armed = false;
    this.#req = null;
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

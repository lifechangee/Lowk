// Real source: backend events -> store, user actions -> typed commands (generated bindings).
import type { UnlistenFn } from '@tauri-apps/api/event';
import { EVENTS, on, commands } from '../lib/ipc';
import type { SourceRef, BindReport, ActionId, LlmEvent, OverlayMode, QaUpdate, QueueState, SessionState, Settings, SttEvent } from '../lib/ipc/bindings';
import { bindFor, triggerText } from '../main/binds';
import { resolveOverlayLang } from './i18n';
import type { OverlayStore } from './store.svelte';
import type { OverlaySource } from './source';
import type { ToneEvent } from './tone';

/** Commands answer with `{status:'error'}` instead of throwing; the overlay only needs to not crash. */
const quiet = (p: Promise<unknown>) => void p.catch(() => {});

export class TauriSource implements OverlaySource {
  #un: UnlistenFn[] = [];
  #tick: ReturnType<typeof setInterval> | undefined;
  #t0 = 0;
  #store: OverlayStore | undefined;
  #locale = 'en';
  #gen = 0;

  start(store: OverlayStore): void {
    // Idempotent: a second start() (HMR, remount) must not stack a second set of listeners (F10).
    this.stop();
    const gen = ++this.#gen;
    this.#store = store;
    store.serverMyAnswer = true;
    store.pillWindow = location.hash === '#pill';
    // A listener whose registration resolves after stop()/restart is released at once instead of leaking.
    const add = (p: Promise<UnlistenFn>) =>
      void p.then((u) => {
        if (gen === this.#gen) this.#un.push(u);
        else u();
      });
    add(on<SttEvent>(EVENTS.sttSegment, (e) => store.applyStt(e)));
    add(on<{ loading: boolean }>(EVENTS.sttLoading, (e) => (store.sttLoading = e.loading)));
    add(on<{ kind: 'target_lost' | 'target_restored' }>(EVENTS.audioNotice, (e) => (store.audioLost = e.kind === 'target_lost')));
    add(on<{ recoverable: boolean }>(EVENTS.sttError, (e) => (store.sttError = { recoverable: e.recoverable })));
    for (const n of ['start', 'sources', 'delta', 'done', 'error'] as const) {
      add(on<Omit<LlmEvent, 'type'>>(`llm://${n}`, (e) => store.applyLlm({ ...e, type: n } as LlmEvent)));
    }
    add(on<{ mode: OverlayMode } | OverlayMode>(EVENTS.overlayMode, (e) => store.setMode(typeof e === 'string' ? e : e.mode)));
    add(on<ToneEvent>(EVENTS.overlayTone, (e) => store.applyTone(e)));
    add(on<QueueState>(EVENTS.queueChanged, (e) => store.setQueue(e.shots, e.thumbs)));
    add(on<QaUpdate>(EVENTS.myAnswer, (e) => {
      // The close update of the PREVIOUS link arrives right after the new Ask's `start`; it must not
      // bring the old text back under the new question.
      if (!e.closed) store.applyMyAnswer(e.text);
    }));
    const holdOff = (r: BindReport) => (store.holdDegraded = r.entries.some((e) => (e.action === 'move_hold' || e.action === 'click_hold') && e.status.state !== 'active'));
    add(on<BindReport>('binds://report', holdOff));
    void commands.bindsGet().then((b) => holdOff(b.report)).catch(() => {});
    add(on<{ perm: string }>('binds://failed', (e) => {
      const ru = store.lang === 'ru';
      const names: Record<string, [string, string]> = { screen_recording: ['Запись экрана', 'Screen Recording'], input_monitoring: ['Мониторинг ввода', 'Input Monitoring'], accessibility: ['Универсальный доступ', 'Accessibility'], microphone: ['Микрофон', 'Microphone'], system_audio: ['Системный звук', 'System Audio'] };
      const n = names[e.perm]?.[ru ? 0 : 1] ?? e.perm;
      store.flash(ru ? `Нужно разрешение «${n}» (Lowk → Разрешения)` : `Needs “${n}” permission (Lowk → Permissions)`);
    }));
    add(on<boolean>('overlay://sentinel', (on_) => (store.sentinel = on_)));
    add(on<Settings>(EVENTS.settingsChanged, (s) => this.#applySettings(s)));
    add(on<SessionState>(EVENTS.sessionState, (e) => this.#session(e)));
    this.#tick = setInterval(() => {
      if (store.phase === 'recording') store.elapsedMs = Date.now() - this.#t0;
    }, 1000);
    void commands.localeGet().then((l) => (this.#locale = l)).catch(() => {});
    void commands.settingsGet().then((s) => this.#applySettings(s)).catch(() => {});
    void commands.overlayTone().then((r) => r.status === 'ok' && store.applyTone(r.data)).catch(() => {});
    void commands.sessionState().then((s) => this.#session(s)).catch(() => {});
  }

  #session(e: SessionState): void {
    const store = this.#store!;
    if (e.phase === 'recording' && store.phase !== 'recording') {
      this.#t0 = Date.now();
      store.elapsedMs = 0;
      store.reset();
      quiet(commands.queueClear());
    }
    if (e.phase === 'idle') store.sttLoading = false;
    store.phase = e.phase;
    store.micOn = e.mic_on ?? true;
  }

  #applySettings(s: Settings): void {
    const store = this.#store!;
    store.transparency = s.overlay?.opacity ?? store.transparency;
    store.fontPx = s.overlay?.font_px ?? store.fontPx;
    store.visibility = s.overlay?.visibility ?? store.visibility;
    store.pillEdge = s.overlay?.pill_edge ?? store.pillEdge;
    if (s.binds) {
      const os = store.isMac ? 'macos' : 'windows';
      const names = { middle: 'M3', back: 'Back', forward: 'Fwd', other: (n: number) => `M${n}` };
      const lab = (a: ActionId) => {
        const b = bindFor(s.binds, a);
        return b ? triggerText(b.trigger, os, names) : '';
      };
      store.keysOverride = { ask: lab('ask_brief') || lab('ask'), complement: lab('ask_complement'), shot: lab('screenshot'), move: lab('move_hold'), click: lab('click_hold'), interactive: lab('toggle_interactive') };
    }
    if (s.templates?.length) store.templates = s.templates.filter((t) => t.id !== 'summary');
    store.lang = resolveOverlayLang(s.ui?.language ?? 'auto', this.#locale);
  }

  stop(): void {
    this.#gen++;
    this.#un.forEach((u) => u());
    this.#un = [];
    clearInterval(this.#tick);
  }

  ask = () => quiet(commands.llmAsk(null));
  screenshot = () => {
    // G7: the queue arrives through `queue://changed`; only a refusal (e.g. no Screen Recording) needs a word.
    void commands.captureScreenshot().then((r) => {
      if (r.status === 'error') this.#store?.flash(this.#store.lang === 'ru' ? 'Нет доступа к записи экрана' : 'No Screen Recording access');
      else this.#store?.flash(this.#store.lang === 'ru' ? 'Скриншот добавлен' : 'Screenshot added');
    }).catch(() => {});
  };
  openSource = (s: SourceRef) => quiet(commands.sourceOpen(s));
  openSettings = () => quiet(commands.settingsOpen('providers'));
  clearQueue = () => quiet(commands.queueClear());
  submitText = (text: string) => quiet(commands.overlayInputSubmit(text));
  setTemplate = (id: string) => quiet(commands.llmSetTemplate(id));
  toggleRecord = () => {
    const recording = this.#store?.phase === 'recording' || this.#store?.phase === 'paused';
    quiet(recording ? commands.sessionStop() : commands.sessionStart({ title: null, folder: null, system: null, mic: null, demo: false }));
  };
  stopRecording = () => quiet(commands.sessionStop());
  setMic = (on: boolean) => {
    // optimistic; the session event that follows is the truth
    if (this.#store) this.#store.micOn = on;
    void commands.sessionSetMic(on).then((r) => r.status === 'ok' && this.#store && (this.#store.micOn = r.data.mic_on ?? true)).catch(() => {});
  };
  escape = () => quiet(commands.overlayEscape());
  dragStart = () => quiet(commands.overlayDragStart());
  dragTo = (dx: number, dy: number) => quiet(commands.overlayDragTo(dx, dy));
  dragEnd = () => quiet(commands.overlayDragEnd());
  pillDragStart = () => quiet(commands.pillDragStart());
  pillDragTo = (dx: number, dy: number) => quiet(commands.pillDragTo(dx, dy));
  pillDragEnd = () => quiet(commands.pillDragEnd());
}

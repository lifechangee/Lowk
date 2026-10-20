// Real source: backend events -> store, user actions -> typed commands (generated bindings).
import type { UnlistenFn } from '@tauri-apps/api/event';
import { EVENTS, on, commands } from '../lib/ipc';
import type { LlmEvent, OverlayMode, QaUpdate, QueueState, SessionState, Settings, SttEvent, Tone } from '../lib/ipc/bindings';
import { resolveLang, setLang } from '../lib/i18n';
import type { OverlayStore } from './store.svelte';
import type { OverlaySource } from './source';

/** Commands answer with `{status:'error'}` instead of throwing; the overlay only needs to not crash. */
const quiet = (p: Promise<unknown>) => void p.catch(() => {});

export class TauriSource implements OverlaySource {
  #un: UnlistenFn[] = [];
  #tick: ReturnType<typeof setInterval> | undefined;
  #t0 = 0;
  #store: OverlayStore | undefined;
  #locale = 'en';

  start(store: OverlayStore): void {
    this.#store = store;
    store.serverMyAnswer = true;
    store.pillWindow = location.hash === '#pill';
    const add = (p: Promise<UnlistenFn>) => void p.then((u) => this.#un.push(u));
    add(on<SttEvent>(EVENTS.sttSegment, (e) => store.applyStt(e)));
    for (const n of ['start', 'delta', 'done', 'error'] as const) {
      add(on<Omit<LlmEvent, 'type'>>(`llm://${n}`, (e) => store.applyLlm({ ...e, type: n } as LlmEvent)));
    }
    add(on<{ mode: OverlayMode } | OverlayMode>(EVENTS.overlayMode, (e) => store.setMode(typeof e === 'string' ? e : e.mode)));
    add(on<Tone>(EVENTS.overlayTone, (e) => store.applyTone(e)));
    add(on<QueueState>(EVENTS.queueChanged, (e) => store.setQueue(e.shots, e.thumbs)));
    add(on<QaUpdate>(EVENTS.myAnswer, (e) => store.applyMyAnswer(e.text)));
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
    }
    store.phase = e.phase;
  }

  #applySettings(s: Settings): void {
    const store = this.#store!;
    store.transparency = s.overlay?.opacity ?? store.transparency;
    store.fontPx = s.overlay?.font_px ?? store.fontPx;
    store.visibility = s.overlay?.visibility ?? store.visibility;
    store.pillEdge = s.overlay?.pill_edge ?? store.pillEdge;
    if (s.templates?.length) store.templates = s.templates.filter((t) => t.id !== 'summary');
    store.lang = resolveLang(s.ui?.language ?? 'auto', this.#locale);
    setLang(store.lang);
  }

  stop(): void {
    this.#un.forEach((u) => u());
    clearInterval(this.#tick);
  }

  ask = () => quiet(commands.llmAsk(null));
  screenshot = () => quiet(commands.captureScreenshot());
  submitText = (text: string) => quiet(commands.overlayInputSubmit(text));
  setTemplate = (id: string) => quiet(commands.llmSetTemplate(id));
  toggleRecord = () => {
    const recording = this.#store?.phase === 'recording' || this.#store?.phase === 'paused';
    quiet(recording ? commands.sessionStop() : commands.sessionStart({ title: null, folder: null, system: null, mic: null, demo: true }));
  };
  escape = () => quiet(commands.overlayEscape());
  dragStart = () => quiet(commands.overlayDragStart());
  dragTo = (dx: number, dy: number) => quiet(commands.overlayDragTo(dx, dy));
  dragEnd = () => quiet(commands.overlayDragEnd());
}

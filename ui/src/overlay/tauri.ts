// Real source: backend events -> store. Commands that do not exist in the generated bindings yet
// (overlay_input_submit, llm_ask, llm_set_template, capture_screenshot) are invoked by name
// and are no-ops until WP6/WP7 land them (errors are swallowed on purpose).
import { invoke } from '@tauri-apps/api/core';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { EVENTS, on, commands } from '../lib/ipc';
import type { LlmEvent, OverlayMode, QueueState, SessionState, SttEvent, Tone } from '../lib/ipc/bindings';
import type { OverlayStore } from './store.svelte';
import type { OverlaySource } from './source';

const call = (cmd: string, args?: Record<string, unknown>) => void invoke(cmd, args).catch(() => {});

export class TauriSource implements OverlaySource {
  #un: UnlistenFn[] = [];
  #tick: ReturnType<typeof setInterval> | undefined;
  #t0 = 0;

  start(store: OverlayStore): void {
    const add = (p: Promise<UnlistenFn>) => void p.then((u) => this.#un.push(u));
    add(on<SttEvent>(EVENTS.sttSegment, (e) => store.applyStt(e)));
    for (const n of ['start', 'delta', 'done', 'error'] as const) {
      add(on<Omit<LlmEvent, 'type'>>(`llm://${n}`, (e) => store.applyLlm({ ...e, type: n } as LlmEvent)));
    }
    add(on<{ mode: OverlayMode } | OverlayMode>(EVENTS.overlayMode, (e) => store.setMode(typeof e === 'string' ? e : e.mode)));
    add(on<Tone>(EVENTS.overlayTone, (e) => store.applyTone(e)));
    add(on<QueueState>(EVENTS.queueChanged, (e) => store.setQueue(e.shots, e.thumbs)));
    add(
      on<SessionState>(EVENTS.sessionState, (e) => {
        if (e.phase === 'recording' && store.phase !== 'recording') {
          this.#t0 = Date.now();
          store.elapsedMs = 0;
          store.reset();
        }
        store.phase = e.phase;
      }),
    );
    this.#tick = setInterval(() => {
      if (store.phase === 'recording') store.elapsedMs = Date.now() - this.#t0;
    }, 1000);
    void commands.settingsGet().then((s) => {
      store.transparency = s.overlay?.opacity ?? store.transparency;
      store.fontPx = s.overlay?.font_px ?? store.fontPx;
      store.visibility = s.overlay?.visibility ?? store.visibility;
      store.pillEdge = s.overlay?.pill_edge ?? store.pillEdge;
      if (s.templates?.length) store.templates = s.templates;
    }).catch(() => {});
  }

  stop(): void {
    this.#un.forEach((u) => u());
    clearInterval(this.#tick);
  }

  ask = () => call('llm_ask');
  screenshot = () => call('capture_screenshot');
  submitText = (text: string) => call('overlay_input_submit', { text });
  setTemplate = (id: string) => call('llm_set_template', { id });
  toggleRecord = () => call('session_toggle_record');
}

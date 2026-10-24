// View state for the main window. No business logic: everything goes through `api`.
import { locale } from '../lib/i18n/locale.svelte';
import type { Key, LangSetting } from '../lib/i18n';
import type { CallMeta, FolderMeta, IndexStatus, Settings, SourceInfo, Target } from '../lib/ipc';
import { api, CommandError, folderKey, folderRef, hasTauri, SYS_TRASH, SYS_UNSORTED, type EngineNeeded, type FolderKey, type LiveLine } from './api';
import { EVENTS, on } from '../lib/ipc';
import type { SessionState } from '../lib/ipc';
import { mergePatch } from './api/mock';
import { hrefs, router } from './router.svelte';
import { tm } from './i18n';
import { ALL_LANGS, resolve as resolveLangs, supportedBy } from './langs';

class Library {
  folders = $state<FolderMeta[]>([]);
  calls = $state<CallMeta[]>([]);
  summarized = $state<string[]>([]);
  loaded = $state(false);

  async reload(): Promise<void> {
    const s = await api.libraryList();
    this.folders = s.folders;
    this.calls = s.calls;
    this.summarized = s.summarized;
    this.loaded = true;
  }
  live = $derived(this.calls.find((c) => c.ended === null && !c.deleted) ?? null);
  active = $derived(this.calls.filter((c) => !c.deleted));
  trashed = $derived(this.calls.filter((c) => c.deleted));
  recent = $derived([...this.active].sort((a, b) => b.started.localeCompare(a.started)));
  unsorted = $derived(this.active.filter((c) => c.folder.kind === 'unsorted'));
  tags = $derived.by(() => {
    const m = new Map<string, number>();
    for (const c of this.active) for (const t of c.tags) m.set(t, (m.get(t) ?? 0) + 1);
    return [...m.entries()].sort((a, b) => b[1] - a[1]);
  });
  callsIn(key: FolderKey): CallMeta[] {
    if (key === SYS_TRASH) return this.trashed;
    return this.active.filter((c) => folderKey(c.folder) === key).sort((a, b) => b.started.localeCompare(a.started));
  }
  call(id: string): CallMeta | undefined { return this.calls.find((c) => c.id === id); }
  folder(key: FolderKey): FolderMeta | undefined { return this.folders.find((f) => folderKey(f.id) === key); }
  lastActivity(key: FolderKey): string | null { return this.callsIn(key)[0]?.started ?? null; }

  /** F9: move a call into a folder (drag & drop and the «Переместить в…» menu both end here). */
  async move(id: string, to: FolderKey): Promise<boolean> {
    const c = this.call(id);
    if (!c || c.deleted || folderKey(c.folder) === to) return false;
    await api.callMove(id, to);
    await this.reload();
    ui.notify(tm('m.movedTo', { folder: to === SYS_UNSORTED ? tm('main.unsorted') : (this.folder(to)?.title ?? '') }));
    return true;
  }
}
export const library = new Library();

class Prefs {
  value = $state<Settings>({});
  loaded = $state(false);

  async load(): Promise<void> {
    this.value = await api.settingsGet();
    this.loaded = true;
    locale.setSetting((this.value.ui?.language ?? 'auto') as LangSetting);
  }
  /** Optimistic JSON-merge-patch update. */
  async patch(patch: unknown): Promise<void> {
    this.value = mergePatch(this.value, patch);
    if ((patch as Settings).ui?.language) locale.setSetting((patch as Settings).ui!.language as LangSetting);
    try { this.value = await api.settingsPatch(patch); } catch { /* keep optimistic value; backend reports via toast in WP6 */ }
  }
}
export const prefs = new Prefs();

/** Call languages: what the engine supports, the default set and the last pick (per-call selector). */
class CallLangs {
  supported = $state<string[]>([...ALL_LANGS]);
  /** Settings «Языки созвона». */
  default = $derived(resolveLangs(prefs.value.stt?.languages, ['ru', 'en'], this.supported));
  /** What the per-call selector starts with: the last pick, else the default. */
  initial = $derived(resolveLangs(prefs.value.stt?.last_languages?.length ? prefs.value.stt.last_languages : prefs.value.stt?.languages, ['ru', 'en'], this.supported));

  async load(): Promise<void> {
    try { this.supported = supportedBy(await api.engineStatus(), prefs.value.stt?.engine); } catch { /* keep the full list */ }
  }
}
export const callLangs = new CallLangs();

/** G3: running apps that produce audio + (lazily) the installed apps behind «Другое…». */
class Sources {
  running = $state<SourceInfo[]>([]);
  installed = $state<SourceInfo[] | null>(null);
  loaded = $state(false);
  /** Apps picked from the installed list, so their name/icon survive when they are not running. */
  picked = $state<Record<string, SourceInfo>>({});

  async refresh(): Promise<void> {
    try { this.running = await api.sourceList(); } catch { /* keep the previous list */ }
    this.loaded = true;
  }
  async loadInstalled(): Promise<void> {
    if (this.installed) return;
    try { this.installed = await api.sourceListInstalled(); } catch { this.installed = []; }
  }
  remember(s: SourceInfo): void { this.picked = { ...this.picked, [s.id]: s }; }
  /** Everything we can resolve an id against (running first: it has the live pid/icon). */
  pool = $derived([...this.running, ...Object.values(this.picked), ...(this.installed ?? [])]);
  find(id: string): SourceInfo | undefined { return this.pool.find((s) => s.id === id); }
}
export const sources = new Sources();

class Session {
  phase = $state<'idle' | 'recording'>('idle');
  startedAt = $state(0);
  now = $state(Date.now());
  folder = $state<FolderKey | null>(null);
  sourceLabel = $state('');
  callId = $state<string | null>(null);
  lines = $state<(LiveLine & { at_s: number })[]>([]);
  /** The local speech model is still loading (`stt://loading`). */
  sttLoading = $state(false);
  /** The microphone is recorded (false = muted for this call; the system audio keeps recording). */
  micOn = $state(true);
  elapsed = $derived(this.phase === 'recording' ? Math.max(0, Math.floor((this.now - this.startedAt) / 1000)) : 0);
  #tick: ReturnType<typeof setInterval> | null = null;
  #off: (() => void) | null = null;

  async start(folder: FolderKey | null, target: Target | null, sourceLabel: string, languages: string[] | null = null): Promise<void> {
    if (this.phase === 'recording') return;
    languages = languages ? [...languages] : null; // plain array, not a reactive proxy (it crosses IPC)
    let st: SessionState;
    try {
      st = await api.sessionStart({ title: null, folder: folder && folder !== SYS_UNSORTED ? folderRef(folder) : null, system: target, mic: null, demo: false, languages });
    } catch (e) {
      // No engine: the backend already raised the «download the model» prompt (engine://needed); nothing started.
      if (e instanceof CommandError && e.app.code === 'engine_not_ready') return;
      throw e;
    }
    // The backend remembers the pick as `stt.last_languages`; mirror it so the selector keeps it without a reload.
    if (languages?.length) prefs.value = mergePatch(prefs.value, { stt: { last_languages: languages } });
    this.micOn = st.mic_on ?? true;
    this.#begin(st.call, folder ?? SYS_UNSORTED, sourceLabel);
    await library.reload();
  }
  #begin(call: string | null, folder: FolderKey, sourceLabel: string): void {
    this.phase = 'recording';
    this.callId = call;
    this.folder = folder;
    this.sourceLabel = sourceLabel;
    this.startedAt = Date.now();
    this.now = this.startedAt;
    this.lines = [];
    this.#tick = setInterval(() => (this.now = Date.now()), 1000);
    this.#off = api.onLiveLine((l) => {
      // A background correction re-emits an already shown utterance: replace it in place.
      const i = l.utt === undefined ? -1 : this.lines.findIndex((x) => x.utt === l.utt && x.speaker === l.speaker);
      if (i >= 0) this.lines[i] = { ...this.lines[i]!, text: l.text };
      else this.lines.push({ ...l, at_s: this.elapsed });
    });
  }
  /** Follow a session started or stopped elsewhere (record bind, overlay, demo end). */
  sync(s: SessionState): void {
    this.micOn = s.mic_on ?? true;
    if (s.phase === 'recording' && this.phase === 'idle') this.#begin(s.call, SYS_UNSORTED, '');
    else if (s.phase === 'idle' && this.phase === 'recording') {
      const done = this.callId;
      this.#off?.();
      if (this.#tick) clearInterval(this.#tick);
      this.phase = 'idle';
      void this.#finished(done);
    }
  }
  /** F8: a recording ended (stop button, bind, overlay): library home, window in front, new call on top. */
  async #finished(call: string | null): Promise<void> {
    await library.reload().catch(() => undefined);
    ui.justFinished = call;
    router.go(hrefs.home);
    void focusMainWindow();
    setTimeout(() => { if (ui.justFinished === call) ui.justFinished = null; }, 6000);
  }
  /** Live mic switch (button in the recording bar); the backend writes the transcript marker. */
  async setMic(on: boolean): Promise<void> {
    if (this.phase !== 'recording') return;
    const prev = this.micOn;
    this.micOn = on;
    try { this.micOn = (await api.sessionSetMic(on)).mic_on ?? true; } catch (e) { this.micOn = prev; throw e; }
  }
  async stop(): Promise<void> {
    if (this.phase !== 'recording') return;
    this.#off?.();
    if (this.#tick) clearInterval(this.#tick);
    this.phase = 'idle';
    const done = this.callId;
    await api.sessionStop();
    await this.#finished(done);
  }
}
export const session = new Session();

class Ui {
  paletteOpen = $state(false);
  /** Briefly highlights the «Новый созвон» card after a jump from another screen. */
  flashNew = $state(false);
  /** The call that just stopped recording: highlighted at the top of Recent for a few seconds. */
  justFinished = $state<string | null>(null);
  /** Open «Переместить в…» menu (context menu or the ⋯ button). */
  moveMenu = $state<{ id: string; x: number; y: number } | null>(null);
  openMove(id: string, x: number, y: number): void { this.moveMenu = { id, x, y }; }
  callTab = $state<'transcript' | 'summary' | 'qa' | 'shots' | 'sources' | 'md'>('transcript');
  /** G5: the header «Новый созвон» popover (never shown on the library home). */
  newCallOpen = $state(false);
  /** Folder the popover starts with when opened from a folder's empty state («Новый созвон здесь»). */
  newCallFolder = $state<string | null>(null);
  /** The «Новая папка» dialog (sidebar «+», empty states). */
  newFolderOpen = $state(false);
  /** A real call cannot start: the prompt to download the model / pick a cloud engine (`engine://needed`). */
  engineNeeded = $state<EngineNeeded | null>(null);
  toast = $state<string | null>(null);
  /** An action shown inside the toast (e.g. «Отменить» after moving a document to the trash). */
  toastAction = $state<{ label: string; run: () => void } | null>(null);
  #timer: ReturnType<typeof setTimeout> | null = null;
  notify(msg: string, action: { label: string; run: () => void } | null = null): void {
    this.toast = msg;
    this.toastAction = action;
    if (this.#timer) clearTimeout(this.#timer);
    this.#timer = setTimeout(() => { this.toast = null; this.toastAction = null; }, action ? 6000 : 3200);
  }
}
export const ui = new Ui();

/** Search index status (`index://status`): drives the «keyword only» notice and the embedder card. */
class Index {
  status = $state<IndexStatus | null>(null);
  async refresh(): Promise<void> {
    try { this.status = await api.indexStatus(); } catch { this.status = null; }
  }
  /** The semantic half is not available: results are keyword-only until it is built. */
  keywordOnly = $derived(this.status !== null && this.status.semantic.state !== 'ready');
}
export const index = new Index();

/** Bring the main window to the front (the backend also does this; plain browsers have no window). */
export async function focusMainWindow(): Promise<void> {
  if (!hasTauri) return;
  try {
    const { getCurrentWindow } = await import('@tauri-apps/api/window');
    const w = getCurrentWindow();
    await w.show();
    await w.unminimize();
    await w.setFocus();
  } catch { /* no core:window permission for this webview: the backend raises the window itself */ }
}

/** Backend events that change main-window state (no-op in a plain browser). */
export function attachEvents(): void {
  if (!hasTauri) return;
  let t: ReturnType<typeof setTimeout> | undefined;
  void on(EVENTS.storeChanged, () => { clearTimeout(t); t = setTimeout(() => void library.reload(), 300); });
  void on<Settings>(EVENTS.settingsChanged, (s) => { prefs.value = s; locale.setSetting((s.ui?.language ?? 'auto') as LangSetting); });
  void on<SessionState>(EVENTS.sessionState, (s) => { if (s.phase === 'idle') session.sttLoading = false; session.sync(s); });
  api.onEngineNeeded((e) => (ui.engineNeeded = e));
  void on<{ perm: string }>('binds://failed', (e) => ui.notify(tm('m.bindFailedPerm', { perm: tm(`m.perm.${e.perm}` as Key) })));
  void on<{ loading: boolean }>(EVENTS.sttLoading, (e) => (session.sttLoading = e.loading));
  // go home even when the library reload fails (e.g. the vault is busy writing the just-ended call)
  void on<{ route?: string }>(EVENTS.sessionEnded, () => { void library.reload().catch(() => undefined).then(() => { router.go(hrefs.home); }); });
}

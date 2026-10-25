// In-memory backend used in a plain browser (vite dev, tests) and as the stand-in
// for every command the backend has not wired yet. Mutations are real within the page's life.
import type { AboutInfo, AppError, BindReport, CallMeta, ChunkKind, DiagnosticsSaved, DocPatch, Document, EmbedderInfo, EngineDescriptor, EngineInfo, EngineProgress, FolderMeta, HwInfo, ImportSummary, IndexStatus, IngestProgress, LlmProviderStatus, LocalServer, MicMode, MicPreset, NewFolder, Os, PermKind, PermState, PinnedUsage, ReindexScope, RetxEvent, RetxPreset, RetxStatus, SearchHit, SearchQuery, SearchResults, SelfTestReport, SemanticState, SessionState, Settings, SourceInfo, SourceRef, StartOpts, StealthStatus, SttDiagnostics, Summary, SummaryEvent, TagSuggestion, UpdateStatus } from '../../lib/ipc';
import { defaultBinds } from '../binds';
import { docMd, kindOfFile, LIVE_LINES, makeCalls, makeDetail, makeDocuments, makeFolderDetail, makeFolders, SUMMARY_MD, SUMMARY_MD_EN, summarizedIds, TAG_SUGGESTIONS } from './fixtures';
import { CommandError, folderKey, folderRef, SYS_TRASH, SYS_UNSORTED, type Backend, type CallDetail, type FolderDetail, type FolderKey, type LibrarySnapshot, type LiveLine, type PickedFile, type SecretProvider, type EngineNeeded } from './types';

const delay = (ms = 30) => new Promise<void>((r) => setTimeout(r, ms));
/** QA switches (browser mock only): `?summary=hold|nokey|offline`, `?embedder=missing|building|off|fail|hold`, `?docs=hold`, `?empty=1`, `?warnmb=1`. */
const qp = (name: string): string | null => (typeof location === 'undefined' ? null : new URLSearchParams(location.search).get(name));
const appErr = (code: AppError['code']): CommandError => new CommandError({ code, params: {}, detail: null });
type Searchable = { kind: ChunkKind; text: string; source: SourceRef; folder: FolderMeta | null; when: string; tags: string[]; id: string };
const docUlid = (n: number): string => `01JC8Z3K6Q9X2M4N7R5T0V${String(n).padStart(4, '0')}`;

/** RFC 7386 merge patch (objects merge, arrays and scalars replace, null deletes). */
export function mergePatch<T>(target: T, patch: unknown): T {
  if (patch === null || typeof patch !== 'object' || Array.isArray(patch)) return patch as T;
  const base: Record<string, unknown> = target && typeof target === 'object' && !Array.isArray(target) ? { ...(target as Record<string, unknown>) } : {};
  for (const [k, v] of Object.entries(patch as Record<string, unknown>)) {
    if (v === null) delete base[k];
    else base[k] = mergePatch(base[k], v);
  }
  return base as T;
}

export function detectOs(): Os {
  const ua = typeof navigator === 'undefined' ? '' : navigator.userAgent;
  return /Windows/i.test(ua) ? 'windows' : /Linux/i.test(ua) && !/Android/i.test(ua) ? 'linux' : 'macos';
}

export class MockBackend implements Backend {
  readonly isMock: boolean = true;
  /** QA switch: `?os=windows|macos` picks the platform variant (default: from the user agent). */
  #os: Os = ((): Os => { const o = qp('os'); return o === 'windows' || o === 'macos' || o === 'linux' ? o : detectOs(); })();
  #now = Date.now();
  #folders: FolderMeta[] = makeFolders();
  #calls: CallMeta[] = makeCalls(this.#now);
  #details = new Map<string, CallDetail>();
  #summaries = new Set<string>(summarizedIds(this.#calls));
  #instructions = new Map<string, string>();
  #secrets = new Set<SecretProvider>();
  #perms: Record<PermKind, PermState> = { microphone: 'not_determined', system_audio: 'unknown', screen_recording: 'denied', input_monitoring: 'not_determined', accessibility: 'denied' };
  #settings: Settings;
  #live = new Set<(l: LiveLine) => void>();
  #liveTimer: ReturnType<typeof setInterval> | null = null;
  #session: SessionState = { phase: 'idle', call: null, demo: false, mic_on: true, mic_mode: 'on', mic_auto_unsupported: false };
  // ---- documents, search, summary ----
  #documents: Document[] = makeDocuments(this.#now);
  #docMds = new Map<string, string>();
  #docSubs = new Set<(p: IngestProgress) => void>();
  #docTimers = new Map<string, ReturnType<typeof setInterval>>();
  #docProgress = new Map<string, { done: number; total: number; eta: number | null }>([[docUlid(408), { done: 120, total: 300, eta: 95 }]]);
  #idxSubs = new Set<(s: IndexStatus) => void>();
  #embInstalled = !['missing', 'fail', 'hold'].includes(qp('embedder') ?? '');
  #embInstalling = false;
  #embCancel = false;
  #summaryText = new Map<string, string>();
  #sumSubs = new Set<(e: SummaryEvent) => void>();
  #sumTimers = new Map<string, ReturnType<typeof setTimeout>>();
  #retxSubs = new Set<(e: RetxEvent) => void>();
  #retxJob: { call: string; done: number; total: number; paused: boolean; timer: ReturnType<typeof setTimeout> | undefined } | null = null;
  #retxRev = new Map<string, number>();
  #retxLive = new Map<string, string[]>();
  #docSeq = 900;

  constructor() {
    const fresh = typeof location !== 'undefined' && /[?&]fresh\b/.test(location.search);
    this.#settings = {
      schema_version: 1,
      ui: { language: 'auto', font_px: 14 },
      onboarding: { step: Number(qp('step') ?? 0), done: !fresh, recording_notice_ack: !fresh || qp('ack') !== null, tour_done: !fresh },
      vault: { path: '~/Documents/Lowk', unsorted_dir: 'Unsorted', trash_dir: 'Trash' },
      // ?src=system mirrors the real backend's fresh default (no default_source stored yet) for the QA suite.
      audio: { default_source: typeof location !== 'undefined' && /[?&]src=system\b/.test(location.search) ? null : { kind: 'app', id: 'us.zoom.xos' }, aec: 'auto', keep_audio: false },
      stt: { engine: 'whisper-large-v3-turbo', languages: ['ru', 'en'], cloud_vendor: 'soniox' },
      llm: { provider: 'openrouter', base_url: 'https://openrouter.ai/api/v1', model: 'deepseek/deepseek-v4.1-flash-1017', reasoning: false, data_collection_deny: true, default_template: 'brief', summary_model: 'deepseek/deepseek-v4.1-flash-1017', pinned_provider: 'DeepInfra', local_base_url: '', local_model: '', vision_model: 'anthropic/claude-haiku-5.5', local_vision_model: '', ask_context_tokens: 0 },
      embedder: { mode: qp('embedder') === 'off' ? 'off' : 'local', model: 'embeddinggemma-2-q8_0', cloud_model: 'openai/text-embedding-3-small' },
      docs: { pinned_budget_tokens: 6000, warn_slow_mb: Number(qp('warnmb') ?? 50), max_file_mb: 500, ocr: true, retrieved_budget_tokens: 3000 },
      overlay: { opacity: 60, font_px: 14, display: 'primary', visibility: 'recording_only', pill_edge: 'right', tone_mode: 'auto' },
      capture: { monitors: 'active' },
      binds: defaultBinds(this.#os),
      stealth: { hide_main: false, verify_interval_ms: 5000 },
      updates: { check_daily: true, last_checked: new Date(this.#now - 3 * 3600_000).toISOString(), skipped_version: null, auto_install: false },
      templates: [
        { id: 'brief', names: { ru: 'Кратко', en: 'Brief' }, instruction: 'Кратко ответь на вопрос, заданный в системе/приложении.', needs_my_answer: false, vision: false, reasoning: null },
        { id: 'complement', names: { ru: 'Дополни мой ответ', en: 'Complement my answer' }, instruction: 'Я уже ответил. Добавь недостающие детали, не повторяя сказанное.', needs_my_answer: true, vision: false, reasoning: null },
        { id: 'summary', names: { ru: 'Резюме', en: 'Summary' }, instruction: 'Сделай краткое резюме созвона: ключевые темы, решения, договорённости и следующие шаги. Отвечай на языке разговора.', needs_my_answer: false, vision: false, reasoning: null },
        { id: 'objection', names: { ru: 'Возражение клиента', en: 'Client objection' }, instruction: 'Предложи вежливый ответ на возражение клиента: признай сомнение, приведи один аргумент и задай уточняющий вопрос.', needs_my_answer: false, vision: false, reasoning: true },
      ],
    };
    if (qp('empty')) { this.#folders = []; this.#calls = []; this.#documents = []; }
    for (const f of this.#folders) this.#instructions.set(folderKey(f.id), makeFolderDetail(f).instructions);
  }

  platform(): Os { return this.#os; }
  async localeGet(): Promise<string> {
    const forced = typeof location !== 'undefined' ? new URLSearchParams(location.search).get('lang') : null;
    return forced ?? (typeof navigator !== 'undefined' ? navigator.language : 'en');
  }
  async settingsGet(): Promise<Settings> { await delay(); return structuredClone(this.#settings); }

  // Updater. `?update=` picks the scenario: (none) = this build has no signing key; idle = up to date; available;
  // offline = the check fails; badsig = the download fails the signature check; recording = a recording is running.
  #upd: UpdateStatus = (() => {
    const mode = qp('update');
    const base: UpdateStatus = { enabled: !!mode, current: '0.1.0', phase: 'idle', available: null, downloaded: 0, total: null, error: null, restart_pending: false, blocked_by_recording: false, last_checked: null };
    if (mode === 'available' || mode === 'badsig' || mode === 'recording') {
      base.phase = 'available';
      base.available = { version: '0.2.0', notes: 'Faster start of the speech engine.\nFixed the overlay position after sleep.', date: '2030-01-01T00:00:00Z' };
    }
    return base;
  })();
  async updateStatus(): Promise<UpdateStatus> { return structuredClone(this.#upd); }
  async updateCheck(_manual: boolean): Promise<UpdateStatus> {
    await delay(150);
    const u = this.#upd;
    if (!u.enabled) return structuredClone(u);
    if (qp('update') === 'recording') { u.blocked_by_recording = true; return structuredClone(u); }
    u.blocked_by_recording = false;
    u.last_checked = new Date().toISOString();
    if (qp('update') === 'offline') { u.phase = 'error'; u.error = 'network'; }
    return structuredClone(u);
  }
  async updateInstall(): Promise<UpdateStatus> {
    const u = this.#upd;
    if (!u.enabled || u.phase !== 'available') return structuredClone(u);
    u.phase = 'downloading'; u.downloaded = 0; u.total = 1000; u.error = null;
    const step = () => {
      if (u.phase !== 'downloading') return;
      u.downloaded += 250;
      if (u.downloaded < 1000) { setTimeout(step, 120); return; }
      if (qp('update') === 'badsig') { u.phase = 'error'; u.error = 'signature'; return; }
      u.phase = 'ready';
    };
    setTimeout(step, 120);
    return structuredClone(u);
  }
  async updateRestart(): Promise<UpdateStatus> {
    const u = this.#upd;
    if (u.phase !== 'ready') return structuredClone(u);
    if (qp('update') === 'recording') u.restart_pending = true;
    else Object.assign(u, { current: u.available?.version ?? u.current, phase: 'idle', available: null, restart_pending: false });
    return structuredClone(u);
  }
  async updateSkip(version: string): Promise<UpdateStatus> {
    const u = this.#upd;
    this.#settings = mergePatch(this.#settings, { updates: { skipped_version: version } });
    if (u.available?.version === version) Object.assign(u, { phase: 'idle', available: null, error: null });
    return structuredClone(u);
  }
  async settingsPatch(patch: unknown): Promise<Settings> {
    this.#settings = mergePatch(this.#settings, patch);
    return structuredClone(this.#settings);
  }

  #counts(): FolderMeta[] {
    return this.#folders.map((f) => ({ ...f, call_count: this.#calls.filter((c) => !c.deleted && folderKey(c.folder) === folderKey(f.id)).length }));
  }
  async libraryList(): Promise<LibrarySnapshot> {
    await delay();
    return { folders: this.#counts(), calls: structuredClone(this.#calls), summarized: [...this.#summaries] };
  }
  async folderGet(key: FolderKey): Promise<FolderDetail | null> {
    const meta = this.#counts().find((f) => folderKey(f.id) === key);
    if (!meta) return null;
    return { meta, instructions: this.#instructions.get(key) ?? '' };
  }
  async callGet(id: string): Promise<CallDetail | null> {
    const meta = this.#calls.find((c) => c.id === id);
    if (!meta) return null;
    let d = this.#details.get(id);
    if (!d) { d = makeDetail(meta); this.#details.set(id, d); }
    return { ...structuredClone(d), meta: structuredClone(meta), summary: this.#summaries.has(id) ? (this.#summaryText.get(id) ?? d.summary ?? this.#summaryFor(meta)) : null };
  }
  async folderCreate(input: NewFolder): Promise<FolderMeta> {
    const id = '01JC8Z3K6Q9X2M4N7R5T0V' + String(900 + this.#folders.length).padStart(4, '0');
    const meta: FolderMeta = { id: { kind: 'user', id }, title: input.title, tags: input.tags, allow_cloud: input.allow_cloud, default_template: null, mic: 'inherit', retx: 'inherit', path: input.title, call_count: 0 };
    this.#folders.push(meta);
    this.#instructions.set(id, input.instructions);
    return meta;
  }
  async folderUpdate(id: string, patch: { instructions?: string; tags?: string[]; allow_cloud?: boolean; mic?: MicPreset; retx?: RetxPreset }): Promise<void> {
    const f = this.#folders.find((x) => folderKey(x.id) === id);
    if (!f) return;
    if (patch.instructions !== undefined) this.#instructions.set(id, patch.instructions);
    if (patch.tags) f.tags = patch.tags;
    if (patch.allow_cloud !== undefined) f.allow_cloud = patch.allow_cloud;
    if (patch.mic !== undefined) f.mic = patch.mic;
    if (patch.retx !== undefined) f.retx = patch.retx;
  }
  #call(id: string): CallMeta | undefined { return this.#calls.find((c) => c.id === id); }
  async callTrash(id: string): Promise<void> { const c = this.#call(id); if (c) c.deleted = true; }
  async callRestore(id: string): Promise<void> { const c = this.#call(id); if (c) c.deleted = false; }
  async callMove(id: string, folder: FolderKey): Promise<void> { const c = this.#call(id); if (c) c.folder = folderRef(folder); }
  async callRename(id: string, title: string): Promise<void> { const c = this.#call(id); if (c) c.title = title; }
  async trashEmpty(): Promise<void> { this.#calls = this.#calls.filter((c) => !c.deleted); }
  async exportMarkdown(id: string): Promise<string> {
    const d = await this.callGet(id);
    if (!d) return '';
    const hms = (s: number) => [Math.floor(s / 3600), Math.floor(s / 60) % 60, s % 60].map((x) => String(x).padStart(2, '0')).join(':');
    const lines = ['---', 'lowk: 1', 'type: call', `id: ${d.meta.id}`, `title: ${d.meta.title}`, `started: ${d.meta.started}`, '---', '', `# ${d.meta.title}`, '', '## Summary', d.summary ?? '', '', '## Transcript', ''];
    for (const u of d.transcript) lines.push(`[${hms(u.at_s)}] **${u.speaker === 'me' ? 'Я' : 'Система'}:** ${u.text}`, '');
    for (const q of d.qa) lines.push(`> [!lowk-qa] id=${q.id} at=${hms(q.at_s)}`, `> **Вопрос:** ${q.question}`, `> **Подсказка ИИ:** ${q.answer}`, ...(q.my_answer ? [`> **Мой ответ:** ${q.my_answer}`] : []), '');
    return lines.join('\n');
  }

  async sourceList(): Promise<SourceInfo[]> {
    // ?apps=none mirrors a machine where nothing plays audio right now.
    if (typeof location !== 'undefined' && /[?&]apps=none\b/.test(location.search)) return [];
    const app = (id: string, name: string, producing: boolean, pid: number): SourceInfo => ({ id, name, bundle_id: id, pid, icon_png_b64: null, producing_audio: producing, kind: 'app' });
    return [app('us.zoom.xos', 'Zoom', true, 4121), app('com.spotify.client', 'Spotify', true, 3310)];
  }
  async sourceListInstalled(): Promise<SourceInfo[]> {
    await delay(60);
    const names: [string, string][] = [['com.apple.Safari', 'Safari'], ['com.google.Chrome', 'Google Chrome'], ['org.mozilla.firefox', 'Firefox'], ['com.apple.Music', 'Music'], ['notion.id', 'Notion'], ['us.zoom.xos', 'Zoom'], ['com.spotify.client', 'Spotify'], ['com.microsoft.teams2', 'Microsoft Teams'], ['com.tinyspeck.slackmacgap', 'Slack'], ['org.videolan.vlc', 'VLC'], ['com.apple.QuickTimePlayerX', 'QuickTime Player'], ['com.loom.desktop', 'Loom']];
    return names.map(([id, name]) => ({ id, name, bundle_id: id, pid: 0, icon_png_b64: null, producing_audio: false, kind: 'app' as const }));
  }
  async engines(): Promise<EngineDescriptor[]> {
    return [
      { id: 'whisper-large-v3-turbo', name: 'Whisper Large V3 Turbo', kind: 'local', languages: ['*'], streaming: 'pseudo', size_mb: 751, license: 'MIT' },
      { id: 'voxtral-mini', name: 'Voxtral Mini', kind: 'local', languages: ['*'], streaming: 'native', size_mb: 2400, license: 'Apache-2.0' },
      { id: 'soniox', name: 'Soniox', kind: 'cloud', languages: ['*'], streaming: 'native', size_mb: 0, license: 'cloud' },
      { id: 'deepgram', name: 'Deepgram', kind: 'cloud', languages: ['*'], streaming: 'native', size_mb: 0, license: 'cloud' },
    ];
  }
  #engineInstalled = new Set<string>();
  #engineSubs = new Set<(p: EngineProgress) => void>();
  #installCancel = false;
  /** QA hook: `?cpu=1` makes the mock worker report the CPU backend (GPU expected). */
  async sttDiagnostics(): Promise<SttDiagnostics | null> {
    const cpu = typeof location !== 'undefined' && /[?&]cpu=1\b/.test(location.search);
    const p = { samples: 0, p50_ms: 0, p95_ms: 0, max_ms: 0 };
    return { engine: 'whisper-large-v3-turbo', lookahead_ms: 0, streams: [], corrected: 0, correction_skipped: 0, correction_lag: p, backend: cpu ? 'cpu' : 'vulkan', cpu_fallback: cpu };
  }
  async engineStatus(): Promise<EngineInfo[]> {
    return (await this.engines()).map((d) => ({
      descriptor: d,
      fitness: { kind: 'recommended' as const },
      installed: d.kind === 'cloud' || this.#engineInstalled.has(d.id),
      recommended: d.id === 'whisper-large-v3-turbo',
      needs_key: d.kind === 'cloud',
      license_url: 'https://openmdw.ai/license/1-1/',
      extra_mb: d.id === 'whisper-large-v3-turbo' ? 620 : 0,
      runtime_missing: d.kind === 'local' && typeof location !== 'undefined' && /[?&]runtime=missing\b/.test(location.search),
    }));
  }
  async engineInstall(id: string): Promise<void> {
    this.#installCancel = false;
    const total = 751 * 1024 * 1024;
    const fail = typeof location !== 'undefined' && /[?&]install=fail\b/.test(location.search);
    const mode = qp('install');
    for (let i = 1; i <= 10; i++) {
      await delay(mode === 'slow' || mode === 'jitter' ? 320 : 60);
      if (this.#installCancel) throw new Error('cancelled');
      const phase = i === 10 ? 'verifying' : 'downloading';
      // QA: `?install=jitter` also emits stale (lower) counts after the real one, like the backend did
      if (mode === 'jitter' && phase === 'downloading') for (const f of this.#engineSubs) f({ engine: id, file: 'model.gguf', phase, done_bytes: (total / 10) * i - 3 * 1048576, total_bytes: total, message: null });
      for (const f of this.#engineSubs) f({ engine: id, file: 'model.gguf', phase, done_bytes: (total / 10) * i, total_bytes: total, message: null });
      if (mode === 'jitter' && phase === 'downloading') for (const f of this.#engineSubs) f({ engine: id, file: 'model.gguf', phase, done_bytes: (total / 10) * i - 8 * 1048576, total_bytes: total, message: null });
      if (fail && i === 6) {
        for (const f of this.#engineSubs) f({ engine: id, file: '', phase: 'failed', done_bytes: 0, total_bytes: 0, message: 'checksum' });
        throw new Error('download_failed');
      }
    }
    this.#engineInstalled.add(id);
    for (const f of this.#engineSubs) f({ engine: id, file: 'model.gguf', phase: 'ready', done_bytes: total, total_bytes: total, message: null });
  }
  async engineInstallCancel(): Promise<void> { this.#installCancel = true; }
  onEngineProgress(cb: (p: EngineProgress) => void): () => void {
    this.#engineSubs.add(cb);
    return () => this.#engineSubs.delete(cb);
  }
  async hwProbe(): Promise<HwInfo> {
    await delay(400);
    return this.#os === 'windows'
      ? { os: 'windows', os_version: 'Windows 11', arch: 'x86_64', chip: '12th Gen Intel(R) Core(TM) i5-12400F @ 2.50GHz', ram_gb: 15.923527, gpus: [{ vendor: 'NVIDIA', name: 'GeForce RTX 3060', vram_mb: 12288 }], cpu_features: ['avx2'], free_disk_gb: 134.99377, on_battery: false }
      : { os: 'macos', os_version: 'macOS 27.0.1', arch: 'aarch64', chip: 'Apple M4', ram_gb: 24, gpus: [], cpu_features: ['neon'], free_disk_gb: 310, on_battery: false };
  }
  async permStatus(): Promise<Record<PermKind, PermState>> {
    if (this.#os === 'windows') return { microphone: this.#perms.microphone, system_audio: 'not_applicable', screen_recording: 'not_applicable', input_monitoring: 'not_applicable', accessibility: 'not_applicable' };
    return { ...this.#perms };
  }
  async permRequest(kind: PermKind): Promise<PermState> { await delay(200); this.#perms[kind] = 'granted'; return 'granted'; }
  async permOpenSettings(kind: PermKind): Promise<void> { const g = globalThis as { __permOpened?: PermKind[] }; g.__permOpened = [...(g.__permOpened ?? []), kind]; await delay(); }
  /** QA switch: `?stealth=degraded|unavailable` (default verified); a re-check with `?stealth=flip` turns green after the first run. */
  #stealth: StealthStatus = ((): StealthStatus => {
    const q = qp('stealth');
    return q === 'degraded' || q === 'flip' ? { state: 'degraded', reason: 'sharing_type_not_none' } : q === 'unavailable' ? { state: 'unavailable', reason: 'no_window_backend' } : { state: 'verified' };
  })();
  #stealthSubs = new Set<(s: StealthStatus) => void>();
  async stealthStatus(): Promise<StealthStatus> { return structuredClone(this.#stealth); }
  async stealthSelftest(): Promise<SelfTestReport> {
    await delay(150);
    if (qp('stealth') === 'flip') { this.#stealth = { state: 'verified' }; for (const f of this.#stealthSubs) f(this.#stealth); }
    const hidden = this.#stealth.state === 'verified';
    return { overlay_hidden: hidden, main_visible: false, details: `overlay flag: ${this.#stealth.state}; pixels: overlay sentinel px=${hidden ? 0 : 9000}; ` };
  }
  onStealthStatus(cb: (s: StealthStatus) => void): () => void { this.#stealthSubs.add(cb); return () => this.#stealthSubs.delete(cb); }
  async appRelaunch(): Promise<void> { await delay(); }
  /** Backend asks for the «download the model / choose cloud» prompt (a real call cannot start). */
  onEngineNeeded(_cb: (e: EngineNeeded) => void): () => void { return () => undefined; }
  onBindReport(_cb: (r: BindReport) => void): () => void { return () => undefined; }
  async secretStatus(p: SecretProvider): Promise<boolean> { return this.#secrets.has(p); }
  async secretSet(p: SecretProvider, value: string): Promise<void> { if (value.trim()) this.#secrets.add(p); }
  async secretDelete(p: SecretProvider): Promise<void> { this.#secrets.delete(p); }
  async secretCheck(p: SecretProvider): Promise<{ ok: boolean; balance?: string; error?: AppError }> {
    if (p === 'openrouter') {
      try { await this.llmKeyTest(); return { ok: true, balance: '$4.20' }; } catch (e) { return { ok: false, error: e instanceof CommandError ? e.app : { code: 'unknown' } }; }
    }
    await delay(500);
    return this.#secrets.has(p) ? { ok: true } : { ok: false };
  }
  async pickVaultDir(): Promise<string | null> { return this.#os === 'windows' ? 'D:\\Notes\\Lowk' : '~/Obsidian/Lowk'; }
  async revealPath(_path: string): Promise<void> { await delay(); }
  async logsReveal(): Promise<void> { await delay(); }
  async appAbout(): Promise<AboutInfo> {
    return { version: '0.0.1', commit: 'a1b2c3d4e5f6', build_date: '2026-10-25', license: 'AGPL-3.0-or-later', source_url: 'https://github.com/lowk-app/lowk' };
  }
  async licensesText(): Promise<string> {
    return '# Third-party components\n\n## Downloadable models\n\n| Model | Licence | Source |\n|---|---|---|\n| Nemotron 3.5 ASR Streaming 0.6B | OpenMDW-1.1 | <https://huggingface.co/handy-computer/nemotron-3.5-asr-streaming-0.6b-gguf/resolve/main/nemotron-3.5-asr-streaming-0.6b-Q8_0.gguf> |\n' + Array.from({ length: 80 }, (_, i) => `| crate-${i} | 1.${i}.0 | MIT OR Apache-2.0 | <https://github.com/example/crate-${i}> |`).join('\n');
  }
  async diagnosticsSave(dest: string): Promise<DiagnosticsSaved> { await delay(); return { path: dest, bytes: 48_213 }; }
  async bindReport(): Promise<BindReport> {
    return {
      entries: (this.#settings.binds ?? []).map((b) => ({
        action: b.action,
        status: b.action === 'move_hold' && this.#perms.input_monitoring !== 'granted' && this.#os === 'macos' ? { state: 'degraded' as const, reason: 'input_monitoring' as const } : { state: 'active' as const },
      })),
    };
  }

  // ================= documents =================
  #doc(id: string): Document {
    const d = this.#documents.find((x) => x.id === id);
    if (!d) throw appErr('not_found');
    return d;
  }
  #emitDoc(d: Document, done = 0, total = 0, eta: number | null = null): void {
    const p: IngestProgress = { doc: d.id, folder: d.folder, status: d.status, done, total, eta_s: eta, error: d.error };
    if (d.status === 'converting' || d.status === 'queued') this.#docProgress.set(d.id, { done, total, eta });
    else this.#docProgress.delete(d.id);
    for (const f of this.#docSubs) f(structuredClone(p));
    this.#emitIndex();
  }
  async docList(folder: FolderKey): Promise<Document[]> {
    await delay();
    return structuredClone(this.#documents.filter((d) => !d.deleted && folderKey(d.folder) === folder).sort((a, b) => b.added.localeCompare(a.added)));
  }
  async docGet(id: string): Promise<Document | null> {
    const d = this.#documents.find((x) => x.id === id);
    return d ? structuredClone(d) : null;
  }
  async pickDocuments(): Promise<PickedFile[] | null> {
    return new Promise((resolve) => {
      const input = document.createElement('input');
      input.type = 'file';
      input.multiple = true;
      input.onchange = () => resolve(input.files?.length ? [...input.files].map((f) => ({ name: f.name, path: f.name, size_bytes: f.size })) : null);
      input.oncancel = () => resolve(null);
      input.click();
    });
  }
  onFileDrop(_cb: (files: PickedFile[]) => void): () => void { return () => undefined; }
  /** Mock identity of a file: name + size (the real backend compares SHA-256 of the content). */
  #same(folder: FolderKey, f: PickedFile): Document[] {
    return f.size_bytes > 0 ? this.#documents.filter((d) => !d.deleted && folderKey(d.folder) === folder && d.file_name === f.name && d.size_bytes === f.size_bytes) : [];
  }
  async docFindDuplicates(folder: FolderKey, files: PickedFile[]): Promise<Array<{ index: number; existing: Document }>> {
    await delay();
    return files.flatMap((f, index) => {
      const e = this.#same(folder, f)[0];
      return e ? [{ index, existing: structuredClone(e) }] : [];
    });
  }
  async docImport(folder: FolderKey, files: PickedFile[], opts?: { pin?: boolean; onDuplicate?: 'copy' | 'replace' }): Promise<Document[]> {
    const out: Document[] = [];
    const warnMb = this.#settings.docs?.warn_slow_mb ?? 50;
    for (const f of files) {
      const same = this.#same(folder, f);
      if (same.length && !opts?.onDuplicate) throw new CommandError({ code: 'doc_duplicate', params: { id: same[0]!.id, title: same[0]!.title }, detail: null });
      if (same.length && opts?.onDuplicate === 'replace') for (const d of same) d.deleted = true;
      const kind = kindOfFile(f.name);
      const stem = f.name.replace(/\.[^.]+$/, '');
      const doc: Document = {
        id: '01JC8Z3K6Q9X2M4N7R5D0C' + String(this.#docSeq++).padStart(4, '0'), folder: folderRef(folder), title: stem, kind: kind ?? 'txt', file_name: f.name,
        md_path: `Docs/${stem}.md`, original_path: `Docs/${f.name}`, size_bytes: f.size_bytes, pages: null, status: 'queued', error: null,
        warnings: f.size_bytes > warnMb * 1048576 ? [{ code: 'large_file', page: null, detail: null }] : [], pinned: !!opts?.pin, tokens: 0, tags: [],
        added: new Date().toISOString(), source_sha256: 'beef'.repeat(16), converter: '', deleted: false,
      };
      this.#documents.push(doc);
      if (!kind) {
        doc.status = 'failed';
        doc.error = { code: 'doc_unsupported', params: { ext: (f.name.split('.').pop() ?? '').toLowerCase() }, detail: null };
        queueMicrotask(() => this.#emitDoc(doc));
      } else this.#run(doc.id);
      out.push(structuredClone(doc));
    }
    return out;
  }
  /** Fake conversion: queued -> converting (page by page) -> indexing -> ready, or a failure keyed on the file name. */
  #run(id: string): void {
    const first = this.#doc(id);
    const sizes: Partial<Record<Document['kind'], number>> = { pdf: 12, pptx: 10, keynote: 10, xlsx: 3 };
    const total = first.pages ?? sizes[first.kind] ?? 1;
    const hold = qp('docs') === 'hold';
    const failCode: AppError['code'] | null = /encrypted|защищ/i.test(first.file_name) ? 'doc_encrypted' : /empty|пуст/i.test(first.file_name) ? 'doc_empty' : /ocr-fail/i.test(first.file_name) ? 'ocr_failed' : null;
    let done = 0;
    first.status = 'queued';
    first.error = null;
    this.#emitDoc(first, 0, total);
    clearInterval(this.#docTimers.get(id));
    const t = setInterval(() => {
      const d = this.#documents.find((x) => x.id === id);
      if (!d || d.status === 'cancelled') { clearInterval(t); return; }
      const stop = () => { clearInterval(t); this.#docTimers.delete(id); };
      if (d.status === 'queued') { d.status = 'converting'; this.#emitDoc(d, 0, total); return; }
      if (d.status === 'converting') {
        done = Math.min(total, done + Math.max(1, Math.ceil(total / 6)));
        if (failCode && done >= total * 0.4) { d.status = 'failed'; d.error = { code: failCode, params: {}, detail: null }; stop(); this.#emitDoc(d, done, total); return; }
        if (hold && done >= total * 0.4) { stop(); this.#emitDoc(d, done, total, 40); return; }
        if (done >= total) { d.status = 'indexing'; this.#emitDoc(d, total, total); return; }
        this.#emitDoc(d, done, total, Math.round((total - done) * 0.8));
        return;
      }
      if (d.status === 'indexing') {
        d.status = 'ready';
        d.pages = d.kind === 'txt' || d.kind === 'md' || d.kind === 'image' ? null : total;
        d.tokens = Math.max(200, total * 180);
        d.converter = d.kind === 'pdf' ? 'pdf-text' : d.kind;
        stop();
        this.#emitDoc(d, total, total);
      }
    }, 220);
    this.#docTimers.set(id, t);
  }
  async docGetMd(id: string): Promise<string> { return this.#docMds.get(id) ?? docMd(this.#doc(id)); }
  async docSaveMd(id: string, md: string): Promise<Document> {
    const d = this.#doc(id);
    this.#docMds.set(id, md);
    d.status = 'indexing';
    d.tokens = Math.ceil(md.length / 4);
    this.#emitDoc(d);
    setTimeout(() => { d.status = 'ready'; this.#emitDoc(d); }, 400);
    return structuredClone(d);
  }
  async docPatch(id: string, patch: DocPatch): Promise<Document> {
    const d = this.#doc(id);
    if (patch.title != null) d.title = patch.title;
    if (patch.tags != null) d.tags = patch.tags;
    if (patch.pinned != null) d.pinned = patch.pinned;
    if (patch.folder != null) d.folder = patch.folder;
    this.#emitIndex();
    return structuredClone(d);
  }
  async docDelete(id: string): Promise<void> { this.#doc(id).deleted = true; this.#emitIndex(); }
  async docRestore(id: string): Promise<Document> { const d = this.#doc(id); d.deleted = false; this.#emitIndex(); return structuredClone(d); }
  async docRetry(id: string): Promise<Document> { const d = this.#doc(id); this.#run(id); return structuredClone(d); }
  async docCancel(id: string): Promise<void> {
    const d = this.#doc(id);
    clearInterval(this.#docTimers.get(id));
    d.status = 'cancelled';
    this.#emitDoc(d);
  }
  async docsPinnedUsage(folder: FolderKey): Promise<PinnedUsage> {
    const pinned = this.#documents.filter((d) => !d.deleted && d.pinned && d.status === 'ready' && folderKey(d.folder) === folder);
    const tokens = pinned.reduce((n, d) => n + d.tokens, 0);
    const budget = this.#settings.docs?.pinned_budget_tokens ?? 6000;
    return { count: pinned.length, tokens, budget_tokens: budget, over_budget: tokens > budget };
  }
  onDocProgress(cb: (p: IngestProgress) => void): () => void {
    this.#docSubs.add(cb);
    // Late subscribers learn where the in-flight conversions stand (the real backend repeats `doc://progress` often enough).
    queueMicrotask(() => {
      for (const [id, p] of this.#docProgress) {
        const d = this.#documents.find((x) => x.id === id);
        if (d && (d.status === 'converting' || d.status === 'queued')) cb({ doc: id, folder: d.folder, status: d.status, done: p.done, total: p.total, eta_s: p.eta, error: null });
      }
    });
    return () => this.#docSubs.delete(cb);
  }

  // ================= search + embedder =================
  #semantic(): SemanticState {
    if (this.#settings.embedder?.mode === 'off') return { state: 'off' };
    if (!this.#embInstalled) return { state: 'model_missing' };
    if (this.#building) return { state: 'building', done: 1240, total: 1840 };
    if (qp('embedder') === 'failed') return { state: 'failed', error: { code: 'embed_failed', params: {}, detail: null } };
    return { state: 'ready' };
  }
  #building = qp('embedder') === 'building';
  #status(): IndexStatus {
    const calls = this.#calls.filter((c) => !c.deleted).length;
    const docs = this.#documents.filter((d) => !d.deleted && d.status === 'ready').length;
    return {
      files: calls + docs, chunks: 1840, pending: this.#documents.filter((d) => ['queued', 'converting', 'indexing'].includes(d.status)).length,
      semantic: this.#semantic(), embedder_model: this.#settings.embedder?.mode === 'off' ? null : (this.#settings.embedder?.model ?? null), last_error: null,
    };
  }
  #emitIndex(): void { for (const f of this.#idxSubs) f(this.#status()); }
  async indexStatus(): Promise<IndexStatus> { return this.#status(); }
  async indexRebuild(_scope: ReindexScope): Promise<void> {
    this.#building = true;
    this.#emitIndex();
    await delay(900);
    this.#building = false;
    this.#emitIndex();
  }
  onIndexStatus(cb: (s: IndexStatus) => void): () => void { this.#idxSubs.add(cb); return () => this.#idxSubs.delete(cb); }

  #searchable(includeTrash: boolean): Searchable[] {
    const out: Searchable[] = [];
    const meta = (ref: SourceRef['folder']) => this.#folders.find((f) => folderKey(f.id) === folderKey(ref)) ?? null;
    for (const c of this.#calls.filter((x) => includeTrash || !x.deleted)) {
      const d = makeDetail(c);
      const base = { folder: meta(c.folder), when: c.started, tags: c.tags };
      const src = (at: number | null, kind: SourceRef['kind'] = 'call'): SourceRef => ({ kind, id: c.id, folder: c.folder, title: c.title, at_s: at, page: null, heading_path: [], chunk_id: null, pinned: false });
      d.transcript.forEach((u, i) => out.push({ ...base, id: `${c.id}:s${i}`, kind: 'speech', text: u.text, source: src(u.at_s) }));
      d.qa.forEach((q) => out.push({ ...base, id: `${c.id}:q${q.id}`, kind: 'qa', text: `${q.question} ${q.answer.replace(/\*\*/g, '')}`, source: src(q.at_s) }));
      d.shots.forEach((s) => out.push({ ...base, id: `${c.id}:i${s.id}`, kind: 'image', text: `${s.title}. ${s.caption}`, source: src(s.at_s, 'image') }));
    }
    for (const d of this.#documents.filter((x) => (includeTrash || !x.deleted) && x.status === 'ready')) {
      const pages = (this.#docMds.get(d.id) ?? docMd(d)).split(/<!-- p:(\d+) -->/);
      for (let i = 1; i < pages.length; i += 2) {
        const page = Number(pages[i]);
        out.push({ id: `${d.id}:p${page}`, kind: 'doc', text: (pages[i + 1] ?? '').replace(/^#+\s.*$/gm, '').replace(/\s+/g, ' ').trim(), source: { kind: 'doc', id: d.id, folder: d.folder, title: d.title, at_s: null, page, heading_path: [], chunk_id: null, pinned: d.pinned }, folder: meta(d.folder), when: d.added, tags: d.tags });
      }
    }
    return out;
  }
  async searchQuery(q: SearchQuery): Promise<SearchResults> {
    await delay(40);
    const terms = q.text.toLowerCase().split(/\s+/).filter(Boolean);
    const f = q.filters;
    const folders = f.folders?.length ? new Set(f.folders.map(folderKey)) : null;
    const kinds = f.kinds?.length ? new Set(f.kinds) : null;
    const perDoc = new Map<string, number>();
    const scored: { score: number; item: Searchable }[] = [];
    for (const item of terms.length ? this.#searchable(f.include_trash ?? false) : []) {
      const low = item.text.toLowerCase();
      if (!terms.every((t) => low.includes(t))) continue;
      if (folders && !folders.has(folderKey(item.source.folder))) continue;
      if (kinds && !kinds.has(item.kind)) continue;
      if (f.tags?.length && !f.tags.every((t) => item.tags.includes(t))) continue;
      if (f.from && item.when.slice(0, 10) < f.from.slice(0, 10)) continue;
      if (f.to && item.when.slice(0, 10) > f.to.slice(0, 10)) continue;
      if (item.kind === 'doc') {
        const n = perDoc.get(item.source.id) ?? 0;
        if (n >= 2) continue;
        perDoc.set(item.source.id, n + 1);
      }
      const count = terms.reduce((n, t) => n + low.split(t).length - 1, 0);
      scored.push({ score: count + (item.kind === 'doc' ? 0.5 : 0) + (item.kind === 'qa' ? -0.25 : 0), item });
    }
    scored.sort((a, b) => b.score - a.score || b.item.when.localeCompare(a.item.when) || a.item.id.localeCompare(b.item.id));
    const facets = new Map<string, { folder: SourceRef['folder']; count: number }>();
    for (const { item } of scored) { const k = folderKey(item.source.folder); const e = facets.get(k) ?? { folder: item.source.folder, count: 0 }; e.count++; facets.set(k, e); }
    const hits: SearchHit[] = scored.slice(q.offset, q.offset + q.limit).map(({ score, item }) => {
      const low = item.text.toLowerCase();
      const at = Math.min(...terms.map((t) => low.indexOf(t)).filter((i) => i >= 0));
      const from = Math.max(0, at - 48);
      const to = Math.min(item.text.length, at + 120);
      const lead = from > 0 ? '… ' : '';
      const snippet = lead + item.text.slice(from, to) + (to < item.text.length ? ' …' : '');
      const sl = snippet.toLowerCase();
      const highlights: [number, number][] = [];
      for (const t of terms) for (let i = sl.indexOf(t); i >= 0; i = sl.indexOf(t, i + t.length)) highlights.push([i, i + t.length]);
      highlights.sort((a, b) => a[0] - b[0]);
      return { chunk_id: item.id, kind: item.kind, source: item.source, folder_title: item.folder?.title ?? '', snippet, highlights, tags: item.tags, when: item.when, score };
    });
    const ready = this.#semantic().state === 'ready';
    return { hits, total: scored.length, took_ms: 12, mode_used: ready && q.mode !== 'keyword' ? q.mode : 'keyword', facets: [...facets.values()] };
  }
  async embedderInfo(): Promise<EmbedderInfo> {
    return { id: 'embeddinggemma-2-q8_0', name: 'EmbeddingGemma 2 (Q8_0)', dim: 768, size_mb: 310, installed: this.#embInstalled, installing: this.#embInstalling, license: 'Gemma Terms of Use', license_url: 'https://ai.google.dev/gemma/terms' };
  }
  async embedderInstall(): Promise<void> {
    this.#embCancel = false;
    this.#embInstalling = true;
    const total = 310 * 1024 * 1024;
    const mode = qp('embedder');
    for (let i = 1; i <= 10; i++) {
      await delay(60);
      if (mode === 'hold' && i >= 4) { while (!this.#embCancel) await delay(60); }
      if (this.#embCancel) { this.#embInstalling = false; throw appErr('ingest_cancelled'); }
      for (const f of this.#engineSubs) f({ engine: 'embedder', file: 'embeddinggemma.gguf', phase: i === 10 ? 'verifying' : 'downloading', done_bytes: (total / 10) * i, total_bytes: total, message: null });
      if (mode === 'fail' && i === 6) {
        this.#embInstalling = false;
        for (const f of this.#engineSubs) f({ engine: 'embedder', file: '', phase: 'failed', done_bytes: 0, total_bytes: 0, message: 'checksum' });
        throw appErr('download_failed');
      }
    }
    this.#embInstalled = true;
    this.#embInstalling = false;
    this.#building = true;
    for (const f of this.#engineSubs) f({ engine: 'embedder', file: 'embeddinggemma.gguf', phase: 'ready', done_bytes: total, total_bytes: total, message: null });
    this.#emitIndex();
    setTimeout(() => { this.#building = false; this.#emitIndex(); }, 1500);
  }
  async embedderInstallCancel(): Promise<void> { this.#embCancel = true; }

  // ================= M3: re-transcription =================
  // QA switches: `?retx=none` (no saved audio), `?retx=hold` (progress never finishes), `?retx=pause` (a recording interrupts it),
  // `?retx=localonly` (the cloud engine is refused), `?retx=nomodel`.
  #retxEmit(e: RetxEvent): void { for (const f of this.#retxSubs) f(e); }
  onRetxEvent(cb: (e: RetxEvent) => void): () => void { this.#retxSubs.add(cb); return () => this.#retxSubs.delete(cb); }
  async retxStatus(call: string): Promise<RetxStatus> {
    const has = qp('retx') !== 'none' && this.#calls.some((c) => c.id === call && !c.deleted && c.id.endsWith('0101'));
    const rev = this.#retxRev.get(call) ?? 1;
    const j = this.#retxJob?.call === call ? this.#retxJob : null;
    return { has_audio: has, running: !!j, paused: !!j?.paused, done_s: j?.done ?? 0, total_s: j?.total ?? 0, rev, revisions: rev > 1 ? [{ rev: 1, kind: 'live', at: new Date().toISOString() }] : [] };
  }
  async retxStart(call: string): Promise<void> {
    if (!(await this.retxStatus(call)).has_audio) throw appErr('not_found');
    if (qp('retx') === 'localonly') throw appErr('local_only_blocked');
    if (qp('retx') === 'nomodel') throw appErr('engine_not_ready');
    if (this.#retxJob) throw appErr('session_busy');
    const total = 90 * 60;
    const job = { call, done: 0, total, paused: false, timer: undefined as ReturnType<typeof setTimeout> | undefined };
    this.#retxJob = job;
    this.#retxEmit({ type: 'start', call, total_s: total, backend: 'local' });
    let ticks = 0;
    const tick = () => {
      if (this.#retxJob !== job) return;
      ticks++;
      if (qp('retx') === 'pause' && ticks === 3) { job.paused = true; this.#retxEmit({ type: 'paused', call }); job.timer = setTimeout(() => { job.paused = false; this.#retxEmit({ type: 'resumed', call }); tick(); }, 1500); return; }
      if (job.paused) return;
      job.done = Math.min(total, job.done + 900);
      this.#retxEmit({ type: 'progress', call, done_s: job.done, total_s: total });
      if (job.done >= total) {
        if (qp('retx') === 'hold') return;
        this.#retxJob = null;
        const d = this.#details.get(call);
        if (d) {
          this.#retxLive.set(call, d.transcript.map((u) => u.text));
          d.transcript = d.transcript.map((u, i) => (i === 0 ? { ...u, text: u.text.replace(/\.?$/, '.') + ' (re-transcribed)' } : u));
        }
        this.#retxRev.set(call, (this.#retxRev.get(call) ?? 1) + 1);
        this.#retxEmit({ type: 'done', call, rev: this.#retxRev.get(call)!, chunks: 42 });
        return;
      }
      job.timer = setTimeout(tick, 120);
    };
    job.timer = setTimeout(tick, 150);
  }
  async retxCancel(call: string): Promise<void> {
    const j = this.#retxJob;
    if (!j || j.call !== call) return;
    clearTimeout(j.timer);
    this.#retxJob = null;
    this.#retxEmit({ type: 'cancelled', call });
  }
  async retxRestore(call: string, rev: number): Promise<void> {
    const live = this.#retxLive.get(call);
    const d = this.#details.get(call);
    if (!live || !d || rev !== 1) throw appErr('not_found');
    d.transcript = d.transcript.map((u, i) => ({ ...u, text: live[i] ?? u.text }));
    this.#retxRev.set(call, (this.#retxRev.get(call) ?? 1) + 1);
  }

  // ================= summary, tags, provider =================
  #summaryFor(meta: CallMeta): string { return meta.languages[0] === 'en' ? SUMMARY_MD_EN : SUMMARY_MD; }
  #sumEmit(e: SummaryEvent): void { for (const f of this.#sumSubs) f(e); }
  onSummaryEvent(cb: (e: SummaryEvent) => void): () => void { this.#sumSubs.add(cb); return () => this.#sumSubs.delete(cb); }
  async llmSummarize(call: string): Promise<void> {
    const meta = this.#call(call);
    if (!meta) throw appErr('not_found');
    const mode = qp('summary');
    if (mode === 'nokey' && !this.#secrets.has('openrouter')) {
      this.#sumEmit({ type: 'start', call });
      await delay(200);
      this.#sumEmit({ type: 'error', call, error: { code: 'api_key_missing', params: {}, detail: null } });
      return;
    }
    const failure = ({ offline: 'network', provider: 'llm_provider', bad: 'llm_bad_response' } as Record<string, AppError['code']>)[mode ?? ''];
    this.#sumEmit({ type: 'start', call });
    if (failure) {
      await delay(250);
      this.#sumEmit({ type: 'error', call, error: { code: failure, params: {}, detail: null } });
      return;
    }
    const md = this.#summaryFor(meta);
    const lines = md.split('\n');
    let i = 0;
    const step = () => {
      if (mode === 'hold' && i >= 4) return;
      if (i < lines.length) {
        this.#sumEmit({ type: 'delta', call, text: lines[i]! + (i < lines.length - 1 ? '\n' : '') });
        i++;
        this.#sumTimers.set(call, setTimeout(step, 60));
        return;
      }
      this.#summaries.add(call);
      this.#summaryText.set(call, md);
      const summary: Summary = { call, key_points: [], decisions: [], tasks: [], questions: [], md, model: this.#settings.llm?.summary_model ?? '', generated_at: new Date().toISOString() };
      this.#sumEmit({ type: 'done', call, summary });
    };
    this.#sumTimers.set(call, setTimeout(step, 200));
  }
  async llmSummarizeCancel(call: string): Promise<void> {
    clearTimeout(this.#sumTimers.get(call));
    this.#sumEmit({ type: 'error', call, error: { code: 'llm_cancelled', params: {}, detail: null } });
  }
  async llmSuggestTags(call: string): Promise<TagSuggestion[]> {
    await delay(300);
    if (qp('tags') === 'fail') throw appErr('network');
    const have = new Set(this.#call(call)?.tags ?? []);
    return TAG_SUGGESTIONS.filter((t) => !have.has(t.tag)).map((t) => ({ tag: t.tag, confidence: t.confidence, existing: t.existing, reason: t.reason }));
  }
  async callSetTags(id: string, tags: string[]): Promise<void> {
    const c = this.#call(id);
    if (c) c.tags = [...new Set(tags.map((t) => t.trim()).filter(Boolean))];
  }
  async llmProviderStatus(): Promise<LlmProviderStatus> {
    return { provider: 'openrouter', model: this.#settings.llm?.summary_model ?? '', pinned_provider: this.#settings.llm?.pinned_provider ?? null, key_present: this.#secrets.has('openrouter') };
  }
  async llmDetectLocal(): Promise<LocalServer[]> {
    await delay(150);
    const mode = qp('local');
    if (mode === 'none') return [];
    const o: LocalServer = { kind: 'ollama', base_url: 'http://127.0.0.1:11434/v1', models: [{ name: 'qwen3:8b', vision: false }, { name: 'gemma3:4b', vision: true }] };
    if (mode === 'one') return [o];
    return [o, { kind: 'lmstudio', base_url: 'http://127.0.0.1:1234/v1', models: [{ name: 'llama-3.2-3b', vision: false }] }];
  }
  async llmKeyTest(): Promise<void> {
    await delay(400);
    if (!this.#secrets.has('openrouter')) throw appErr('api_key_missing');
    if (qp('key') === 'bad') throw appErr('llm_provider');
  }

  // ================= export / import =================
  async pickSavePath(defaultName: string): Promise<string | null> { return `~/Downloads/${defaultName}`; }
  async pickOpenPath(extension: string): Promise<string | null> { return `~/Downloads/lowk-export.${extension}`; }
  async folderExport(_folder: string, _dest: string): Promise<number> { await delay(400); return 14; }
  async folderImport(_src: string): Promise<ImportSummary> {
    await delay(400);
    if (qp('import') === 'fail') throw appErr('archive_invalid');
    return { files_copied: 14, skipped: 1, ids_reassigned: 2, folders_merged: 0 };
  }
  async exportVault(_dest: string): Promise<number> { await delay(500); return 212; }
  async importVault(_src: string): Promise<ImportSummary> {
    await delay(500);
    if (qp('import') === 'fail') throw appErr('archive_invalid');
    return { files_copied: 212, skipped: 0, ids_reassigned: 5, folders_merged: 3 };
  }

  async sessionStart(opts: StartOpts): Promise<SessionState> {
    const id = '01JC8Z3K6Q9X2M4N7R5T0V' + String(800 + this.#calls.length).padStart(4, '0');
    this.#calls.unshift({
      id, title: opts.title ?? 'Новый созвон', folder: opts.folder ?? { kind: 'unsorted' }, started: new Date().toISOString(), ended: null, duration_s: null,
      languages: opts.languages?.length ? opts.languages : ['ru'], tags: [], sources: { me: 'Microphone', system: null }, stt_engine: 'whisper-large-v3-turbo', path: '', deleted: false,
    });
    // QA hook: what the window asked for (languages of the call), like the real backend's frontmatter.
    (globalThis as { __lowkStartOpts?: StartOpts }).__lowkStartOpts = opts;
    if (opts.languages?.length) this.#settings = mergePatch(this.#settings, { stt: { last_languages: opts.languages } });
    // like the backend: the folder preset (inherit = settings) decides if the mic starts on
    const preset = opts.folder?.kind === 'user' ? (this.#folders.find((f) => f.id.kind === 'user' && f.id.id === (opts.folder as { id: string }).id)?.mic ?? 'inherit') : 'inherit';
    const g = this.#settings.audio?.record_mic as unknown;
    const globalMode: MicMode = g === false || g === 'off' ? 'off' : g === 'auto' ? 'auto' : 'on';
    this.#session = this.#micState(preset === 'inherit' ? globalMode : preset, (globalThis as { __lowkMockAutoApp?: boolean }).__lowkMockAutoApp ?? opts.system?.kind === 'app', { phase: 'recording', call: id, demo: true });
    let i = 0;
    this.#liveTimer = setInterval(() => { const l = LIVE_LINES[i++ % LIVE_LINES.length]!; this.#live.forEach((cb) => cb(l)); }, 2600);
    return this.#session;
  }
  async sessionStop(): Promise<void> {
    if (this.#liveTimer) clearInterval(this.#liveTimer);
    this.#liveTimer = null;
    const c = this.#session.call ? this.#call(this.#session.call) : undefined;
    if (c) { c.ended = new Date().toISOString(); c.duration_s = Math.max(1, Math.round((Date.now() - Date.parse(c.started)) / 1000)); c.title = c.title || 'Новый созвон'; }
    this.#session = { phase: 'idle', call: null, demo: false, mic_on: true, mic_mode: 'on', mic_auto_unsupported: false };
  }
  #autoApp = false;
  /** Mic fields for a mode like the backend: auto starts closed, or records like «on» when it cannot be detected. */
  #micState(mode: MicMode, canDetect: boolean, base: Pick<SessionState, 'phase' | 'call' | 'demo'>): SessionState {
    this.#autoApp = canDetect;
    const unsupported = mode === 'auto' && !canDetect;
    return { ...base, mic_mode: mode, mic_auto_unsupported: unsupported, mic_on: mode === 'on' || unsupported };
  }
  async sessionSetMicMode(mode: MicMode): Promise<SessionState> {
    if (this.#session.phase !== 'recording') throw appErr('session_idle');
    const { phase, call, demo } = this.#session;
    const prev = this.#session.mic_on;
    this.#session = this.#micState(mode, this.#autoApp, { phase, call, demo });
    if (mode === 'auto' && !this.#session.mic_auto_unsupported) this.#session.mic_on = prev; // keeps the state until the app lets go
    return this.#session;
  }
  async sessionSetMic(on: boolean): Promise<SessionState> {
    if (this.#session.phase !== 'recording') throw appErr('session_idle');
    this.#session = { ...this.#session, mic_on: on, mic_mode: on ? 'on' : 'off', mic_auto_unsupported: false };
    return this.#session;
  }
  onLiveLine(cb: (l: LiveLine) => void): () => void { this.#live.add(cb); return () => this.#live.delete(cb); }
}

export { SYS_TRASH, SYS_UNSORTED };

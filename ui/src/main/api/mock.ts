// In-memory backend used in a plain browser (vite dev, tests) and as the stand-in
// for every command WP6 has not wired yet. Mutations are real within the page's life.
import type { BindReport, CallMeta, EngineDescriptor, EngineInfo, EngineProgress, FolderMeta, HwInfo, NewFolder, Os, PermKind, PermState, SessionState, Settings, SourceInfo, StartOpts } from '../../lib/ipc';
import { defaultBinds } from '../binds';
import { LIVE_LINES, makeCalls, makeDetail, makeFolderDetail, makeFolders, summarizedIds } from './fixtures';
import { folderKey, folderRef, SYS_TRASH, SYS_UNSORTED, type Backend, type CallDetail, type DocItem, type FolderDetail, type FolderKey, type LibrarySnapshot, type LiveLine, type SearchHit, type SecretProvider, type EngineNeeded } from './types';

const delay = (ms = 30) => new Promise<void>((r) => setTimeout(r, ms));

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
  #os: Os = detectOs();
  #now = Date.now();
  #folders: FolderMeta[] = makeFolders();
  #calls: CallMeta[] = makeCalls(this.#now);
  #details = new Map<string, CallDetail>();
  #summaries = new Set<string>(summarizedIds(this.#calls));
  #instructions = new Map<string, string>();
  #docs = new Map<string, DocItem[]>();
  #secrets = new Set<SecretProvider>();
  #perms: Record<PermKind, PermState> = { microphone: 'not_determined', system_audio: 'unknown', screen_recording: 'denied', input_monitoring: 'not_determined', accessibility: 'denied' };
  #settings: Settings;
  #live = new Set<(l: LiveLine) => void>();
  #liveTimer: ReturnType<typeof setInterval> | null = null;
  #session: SessionState = { phase: 'idle', call: null, demo: false };

  constructor() {
    const fresh = typeof location !== 'undefined' && /[?&]fresh\b/.test(location.search);
    this.#settings = {
      schema_version: 1,
      ui: { language: 'auto', font_px: 14 },
      onboarding: { step: 0, done: !fresh, recording_notice_ack: !fresh },
      vault: { path: '~/Documents/Lowk', unsorted_dir: 'Unsorted', trash_dir: 'Trash' },
      // ?src=system mirrors the real backend's fresh default (no default_source stored yet) for the QA suite.
      audio: { default_source: typeof location !== 'undefined' && /[?&]src=system\b/.test(location.search) ? null : { kind: 'app', id: 'us.zoom.xos' }, aec: 'auto', keep_audio: false },
      stt: { engine: 'whisper-large-v3-turbo', languages: ['ru', 'en'], cloud_vendor: 'soniox' },
      llm: { provider: 'openrouter', base_url: 'https://openrouter.ai/api/v1', model: 'deepseek/deepseek-v4.1-flash', reasoning: false, data_collection_deny: true, default_template: 'brief' },
      overlay: { opacity: 60, font_px: 14, display: 'primary', visibility: 'recording_only', pill_edge: 'right', tone_mode: 'auto' },
      capture: { monitors: 'all' },
      binds: defaultBinds(this.#os),
      stealth: { hide_main: false, verify_interval_ms: 5000 },
      updates: { check_daily: true, last_checked: new Date(this.#now - 3 * 3600_000).toISOString() },
      templates: [
        { id: 'brief', names: { ru: 'Кратко', en: 'Brief' }, instruction: 'Briefly answer the question.' },
        { id: 'complement', names: { ru: 'Дополни мой ответ', en: 'Complement my answer' }, instruction: 'Complement my answer.', needs_my_answer: true },
        { id: 'summary', names: { ru: 'Резюме', en: 'Summary' }, instruction: 'Summarise the call.' },
        { id: 'objection', names: { ru: 'Возражение клиента', en: 'Client objection' }, instruction: 'Suggest a reply to the objection.' },
      ],
    };
    for (const f of this.#folders) {
      const d = makeFolderDetail(f);
      const id = folderKey(f.id);
      this.#instructions.set(id, d.instructions);
      this.#docs.set(id, [...d.docs]);
    }
  }

  platform(): Os { return this.#os; }
  async localeGet(): Promise<string> {
    const forced = typeof location !== 'undefined' ? new URLSearchParams(location.search).get('lang') : null;
    return forced ?? (typeof navigator !== 'undefined' ? navigator.language : 'en');
  }
  async settingsGet(): Promise<Settings> { await delay(); return structuredClone(this.#settings); }
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
    return { meta, instructions: this.#instructions.get(key) ?? '', docs: [...(this.#docs.get(key) ?? [])] };
  }
  async callGet(id: string): Promise<CallDetail | null> {
    const meta = this.#calls.find((c) => c.id === id);
    if (!meta) return null;
    let d = this.#details.get(id);
    if (!d) { d = makeDetail(meta); this.#details.set(id, d); }
    return { ...structuredClone(d), meta: structuredClone(meta), summary: this.#summaries.has(id) ? (d.summary ?? makeDetail(meta).summary ?? SUMMARY_TEXT) : null };
  }
  async folderCreate(input: NewFolder): Promise<FolderMeta> {
    const id = '01JC8Z3K6Q9X2M4N7R5T0V' + String(900 + this.#folders.length).padStart(4, '0');
    const meta: FolderMeta = { id: { kind: 'user', id }, title: input.title, tags: input.tags, allow_cloud: input.allow_cloud, default_template: null, path: input.title, call_count: 0 };
    this.#folders.push(meta);
    this.#instructions.set(id, input.instructions);
    this.#docs.set(id, []);
    return meta;
  }
  async folderUpdate(id: string, patch: { instructions?: string; tags?: string[]; allow_cloud?: boolean }): Promise<void> {
    const f = this.#folders.find((x) => folderKey(x.id) === id);
    if (!f) return;
    if (patch.instructions !== undefined) this.#instructions.set(id, patch.instructions);
    if (patch.tags) f.tags = patch.tags;
    if (patch.allow_cloud !== undefined) f.allow_cloud = patch.allow_cloud;
  }
  async docAdd(folderId: string, fileName: string): Promise<DocItem> {
    const ext = (fileName.split('.').pop() ?? 'txt').toLowerCase();
    const doc: DocItem = { id: crypto.randomUUID(), name: fileName, ext: (['pdf', 'docx', 'md', 'txt'].includes(ext) ? ext : 'txt') as DocItem['ext'], pages: null, state: 'indexing' };
    this.#docs.set(folderId, [...(this.#docs.get(folderId) ?? []), doc]);
    return doc;
  }
  #call(id: string): CallMeta | undefined { return this.#calls.find((c) => c.id === id); }
  async callTrash(id: string): Promise<void> { const c = this.#call(id); if (c) c.deleted = true; }
  async callRestore(id: string): Promise<void> { const c = this.#call(id); if (c) c.deleted = false; }
  async callMove(id: string, folder: FolderKey): Promise<void> { const c = this.#call(id); if (c) c.folder = folderRef(folder); }
  async callRename(id: string, title: string): Promise<void> { const c = this.#call(id); if (c) c.title = title; }
  async trashEmpty(): Promise<void> { this.#calls = this.#calls.filter((c) => !c.deleted); }
  async summaryGenerate(id: string): Promise<string> {
    await delay(1300);
    this.#summaries.add(id);
    return SUMMARY_TEXT;
  }
  async exportMarkdown(id: string): Promise<string> {
    const d = await this.callGet(id);
    if (!d) return '';
    const hms = (s: number) => [Math.floor(s / 3600), Math.floor(s / 60) % 60, s % 60].map((x) => String(x).padStart(2, '0')).join(':');
    const lines = ['---', 'lowk: 1', 'type: call', `id: ${d.meta.id}`, `title: ${d.meta.title}`, `started: ${d.meta.started}`, '---', '', `# ${d.meta.title}`, '', '## Summary', d.summary ?? '', '', '## Transcript', ''];
    for (const u of d.transcript) lines.push(`[${hms(u.at_s)}] **${u.speaker === 'me' ? 'Я' : 'Система'}:** ${u.text}`, '');
    for (const q of d.qa) lines.push(`> [!lowk-qa] id=${q.id} at=${hms(q.at_s)}`, `> **Вопрос:** ${q.question}`, `> **Подсказка ИИ:** ${q.answer}`, ...(q.my_answer ? [`> **Мой ответ:** ${q.my_answer}`] : []), '');
    return lines.join('\n');
  }
  async search(query: string): Promise<SearchHit[]> {
    const q = query.trim().toLowerCase();
    const hits: SearchHit[] = [];
    for (const c of this.#calls.filter((x) => !x.deleted)) {
      const d = makeDetail(c);
      const line = d.transcript.find((u) => u.text.toLowerCase().includes(q));
      if (!q || c.title.toLowerCase().includes(q) || line) hits.push({ group: 'calls', title: c.title, snippet: line && q ? line.text : c.tags.map((t) => '#' + t).join(' '), folder: folderKey(c.folder), call: c.id });
    }
    for (const [fid, docs] of this.#docs) for (const d of docs) if (!q || d.name.toLowerCase().includes(q)) hits.push({ group: 'docs', title: d.name, snippet: d.pages ? `p. ${d.pages}` : '', folder: fid, call: null });
    return hits.slice(0, 12);
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
    for (let i = 1; i <= 10; i++) {
      await delay(60);
      if (this.#installCancel) throw new Error('cancelled');
      const phase = i === 10 ? 'verifying' : 'downloading';
      for (const f of this.#engineSubs) f({ engine: id, file: 'model.gguf', phase, done_bytes: (total / 10) * i, total_bytes: total, message: null });
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
      ? { os: 'windows', os_version: 'Windows 11', arch: 'x86_64', chip: 'Intel Core i5-10400F', ram_gb: 16, gpus: [{ vendor: 'NVIDIA', name: 'RTX 3060', vram_mb: 12288 }], cpu_features: ['avx2'], free_disk_gb: 310, on_battery: false }
      : { os: 'macos', os_version: 'macOS 27.0.1', arch: 'aarch64', chip: 'Apple M4', ram_gb: 24, gpus: [], cpu_features: ['neon'], free_disk_gb: 310, on_battery: false };
  }
  async permStatus(): Promise<Record<PermKind, PermState>> {
    if (this.#os === 'windows') return { microphone: this.#perms.microphone, system_audio: 'not_applicable', screen_recording: 'not_applicable', input_monitoring: 'not_applicable', accessibility: 'not_applicable' };
    return { ...this.#perms };
  }
  async permRequest(kind: PermKind): Promise<PermState> { await delay(200); this.#perms[kind] = 'granted'; return 'granted'; }
  async permOpenSettings(_kind: PermKind): Promise<void> { await delay(); }
  async appRelaunch(): Promise<void> { await delay(); }
  /** Backend asks for the «download the model / choose cloud» prompt (a real call cannot start). */
  onEngineNeeded(_cb: (e: EngineNeeded) => void): () => void { return () => undefined; }
  onBindReport(_cb: (r: BindReport) => void): () => void { return () => undefined; }
  async secretStatus(p: SecretProvider): Promise<boolean> { return this.#secrets.has(p); }
  async secretSet(p: SecretProvider, value: string): Promise<void> { if (value.trim()) this.#secrets.add(p); }
  async secretDelete(p: SecretProvider): Promise<void> { this.#secrets.delete(p); }
  async secretCheck(p: SecretProvider): Promise<{ ok: boolean; balance?: string }> { await delay(500); return this.#secrets.has(p) ? { ok: true, balance: '$4.20' } : { ok: false }; }
  async pickVaultDir(): Promise<string | null> { return this.#os === 'windows' ? 'D:\\Notes\\Lowk' : '~/Obsidian/Lowk'; }
  async revealPath(_path: string): Promise<void> { await delay(); }
  async bindReport(): Promise<BindReport> {
    return {
      entries: (this.#settings.binds ?? []).map((b) => ({
        action: b.action,
        status: b.action === 'move_hold' && this.#perms.input_monitoring !== 'granted' && this.#os === 'macos' ? { state: 'degraded' as const, reason: 'input_monitoring' as const } : { state: 'active' as const },
      })),
    };
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
    this.#session = { phase: 'recording', call: id, demo: true };
    let i = 0;
    this.#liveTimer = setInterval(() => { const l = LIVE_LINES[i++ % LIVE_LINES.length]!; this.#live.forEach((cb) => cb(l)); }, 2600);
    return this.#session;
  }
  async sessionStop(): Promise<void> {
    if (this.#liveTimer) clearInterval(this.#liveTimer);
    this.#liveTimer = null;
    const c = this.#session.call ? this.#call(this.#session.call) : undefined;
    if (c) { c.ended = new Date().toISOString(); c.duration_s = Math.max(1, Math.round((Date.now() - Date.parse(c.started)) / 1000)); c.title = c.title || 'Новый созвон'; }
    this.#session = { phase: 'idle', call: null, demo: false };
  }
  onLiveLine(cb: (l: LiveLine) => void): () => void { this.#live.add(cb); return () => this.#live.delete(cb); }
}

const SUMMARY_TEXT = '- Цель Q4: рост выручки выше 30% без роста CAC; ставка на partner-канал (дешевле на 20–25%).\n- EBITDA margin B2B за Q3: 23,4% (около 25% без one-off миграции CRM).\n- Следующий шаг: обсудить бюджет на партнёров.';
export { SYS_TRASH, SYS_UNSORTED };

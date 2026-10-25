// Real backend: every method that has a command in the generated bindings calls it. No silent fallbacks:
// Commands that the backend has not implemented yet fail with their `not_implemented` AppError, which the UI shows.
// File dialogs go through tauri-plugin-dialog (invoked directly, capability dialog:allow-open / allow-save).
import { invoke } from '@tauri-apps/api/core';
import { commands, EVENTS, on } from '../../lib/ipc';
import type { AboutInfo, AppError, BindReport, DiagnosticsSaved, DocPatch, Document, EmbedderInfo, EngineDescriptor, EngineInfo, EngineProgress, FolderMeta, HwInfo, ImportSummary, IndexStatus, IngestProgress, LlmProviderStatus, LocalServer, MicMode, MicPreset, NewFolder, PermKind, PermState, PinnedUsage, ReindexScope, RetxEvent, RetxPreset, RetxStatus, SearchQuery, SearchResults, SelfTestReport, SessionState, Settings, SourceInfo, StartOpts, StealthStatus, SttDiagnostics, SttEvent, SummaryEvent, TagSuggestion, UpdateStatus } from '../../lib/ipc';
import { MockBackend } from './mock';
import { CommandError, folderKey, folderRef, type EngineNeeded, type CallDetail, type FolderDetail, type FolderKey, type LibrarySnapshot, type LiveLine, type PickedFile, type SecretProvider } from './types';

export { CommandError };

type Res<T> = { status: 'ok'; data: T } | { status: 'error'; error: AppError };
function ok<T>(r: Res<T>): T {
  if (r.status === 'error') {
    // Tauri itself rejects with a plain string (command not allowed, bad arguments): keep it visible, never «unknown error»
    if (typeof r.error === 'string') {
      console.error('[lowk] command rejected:', r.error);
      throw new CommandError({ code: 'unknown', params: {}, detail: r.error } as unknown as AppError);
    }
    throw new CommandError(r.error);
  }
  return r.data;
}

const PERMS: PermKind[] = ['microphone', 'system_audio', 'screen_recording', 'input_monitoring', 'accessibility'];

export class TauriBackend extends MockBackend {
  override readonly isMock: boolean = false;
  #vaultOpen = false;

  override async localeGet(): Promise<string> { return commands.localeGet(); }
  override async settingsGet(): Promise<Settings> { return commands.settingsGet(); }
  override async settingsPatch(patch: unknown): Promise<Settings> { return ok(await commands.settingsPatch(patch)); }

  override async updateStatus(): Promise<UpdateStatus> { return commands.updateStatus(); }
  override async updateCheck(manual: boolean): Promise<UpdateStatus> { return ok(await commands.updateCheck(manual)); }
  override async updateInstall(): Promise<UpdateStatus> { return commands.updateInstall(); }
  override async updateRestart(): Promise<UpdateStatus> { return commands.updateRestart(); }
  override async updateSkip(version: string): Promise<UpdateStatus> { return ok(await commands.updateSkip(version)); }

  async #ensureVault(): Promise<void> {
    if (this.#vaultOpen) return;
    ok(await commands.vaultOpen(null));
    this.#vaultOpen = true;
  }

  override async libraryList(): Promise<LibrarySnapshot> {
    await this.#ensureVault();
    const [folders, calls, summarized] = await Promise.all([commands.folderList(), commands.callList(null, true), commands.callSummarized()]);
    return { folders: ok(folders).filter((f) => f.id.kind === 'user'), calls: ok(calls), summarized: ok(summarized) };
  }
  override async folderGet(key: FolderKey): Promise<FolderDetail | null> {
    const ref = folderRef(key);
    if (ref.kind !== 'user') return null;
    try {
      const d = ok(await commands.folderGet(ref));
      return { meta: d.meta, instructions: d.instructions };
    } catch { return null; }
  }
  override async callGet(id: string): Promise<CallDetail | null> {
    try { return ok(await commands.callGet(id)) as CallDetail; } catch { return null; }
  }
  override async folderCreate(input: NewFolder): Promise<FolderMeta> { return ok(await commands.folderCreate(input)); }
  override async folderUpdate(id: string, patch: { instructions?: string; tags?: string[]; allow_cloud?: boolean; mic?: MicPreset; retx?: RetxPreset }): Promise<void> {
    ok(await commands.folderUpdate(id, { title: null, tags: patch.tags ?? null, allow_cloud: patch.allow_cloud ?? null, default_template: null, mic: patch.mic ?? null, retx: patch.retx ?? null, instructions: patch.instructions ?? null }));
  }
  override async callTrash(id: string): Promise<void> { ok(await commands.callDelete(id)); }
  override async callRestore(id: string): Promise<void> { ok(await commands.callRestore(id)); }
  override async callMove(id: string, folder: FolderKey): Promise<void> { ok(await commands.callMove(id, folderRef(folder))); }
  override async callRename(id: string, title: string): Promise<void> { ok(await commands.callRename(id, title)); }
  override async trashEmpty(): Promise<void> { ok(await commands.trashEmpty()); }
  override async exportMarkdown(id: string): Promise<string> { return ok(await commands.callGetMd(id)); }
  override async revealPath(path: string): Promise<void> { ok(await commands.pathReveal(path)); }
  override async logsReveal(): Promise<void> { ok(await commands.logsReveal()); }
  override async appAbout(): Promise<AboutInfo> { return commands.appAbout(); }
  override async licensesText(): Promise<string> { return commands.licensesText(); }
  override async diagnosticsSave(dest: string): Promise<DiagnosticsSaved> { return ok(await commands.diagnosticsSave(dest)); }
  override async sourceList(): Promise<SourceInfo[]> { return ok(await commands.sourceList()); }
  override async sourceListInstalled(): Promise<SourceInfo[]> { return ok(await commands.sourceListInstalled()); }
  override async engines(): Promise<EngineDescriptor[]> { return commands.engineList(); }
  override async engineStatus(): Promise<EngineInfo[]> { return commands.engineStatus(); }
  override async sttDiagnostics(): Promise<SttDiagnostics | null> { return commands.sttDiagnostics(); }
  override async engineInstall(id: string): Promise<void> { ok(await commands.engineInstall(id)); }
  override async engineInstallCancel(): Promise<void> { await commands.engineInstallCancel(); }
  override onEngineProgress(cb: (p: EngineProgress) => void): () => void {
    let off: (() => void) | undefined;
    let dead = false;
    void on<EngineProgress>(EVENTS.engineProgress, cb).then((u) => (dead ? u() : (off = u)));
    return () => { dead = true; off?.(); };
  }
  override async hwProbe(): Promise<HwInfo> { return ok(await commands.hwProbe()).report.info; }
  override async permStatus(): Promise<Record<PermKind, PermState>> {
    const s = await commands.permStatus();
    return Object.fromEntries(PERMS.map((k) => [k, s[k] ?? 'unknown'])) as Record<PermKind, PermState>;
  }
  override async stealthStatus(): Promise<StealthStatus> { return commands.stealthStatus(); }
  override async stealthSelftest(): Promise<SelfTestReport> { return ok(await commands.stealthSelftest()); }
  override onStealthStatus(cb: (s: StealthStatus) => void): () => void { return TauriBackend.#listen(EVENTS.stealthStatus, cb); }
  override async permRequest(kind: PermKind): Promise<PermState> { return ok(await commands.permRequest(kind)); }
  override async permOpenSettings(kind: PermKind): Promise<void> { await commands.permOpenSettings(kind); }
  override async appRelaunch(): Promise<void> { await commands.appRelaunch(); }
  override onEngineNeeded(cb: (e: EngineNeeded) => void): () => void {
    let off: (() => void) | undefined;
    let dead = false;
    void on<EngineNeeded>('engine://needed', cb).then((u) => (dead ? u() : (off = u)));
    return () => { dead = true; off?.(); };
  }
  override onBindReport(cb: (r: BindReport) => void): () => void {
    let off: (() => void) | undefined;
    let dead = false;
    void on<BindReport>('binds://report', cb).then((u) => (dead ? u() : (off = u)));
    return () => { dead = true; off?.(); };
  }
  override async secretStatus(p: SecretProvider): Promise<boolean> { return ok(await commands.secretStatus(p)); }
  override async secretSet(p: SecretProvider, value: string): Promise<void> { ok(await commands.secretSet(p, value)); }
  override async secretDelete(p: SecretProvider): Promise<void> { ok(await commands.secretDelete(p)); }
  override async secretCheck(p: SecretProvider): Promise<{ ok: boolean; balance?: string; error?: AppError }> {
    if (p !== 'openrouter') return { ok: await this.secretStatus(p) };
    try { await this.llmKeyTest(); return { ok: true }; } catch (e) { return { ok: false, error: e instanceof CommandError ? e.app : { code: 'unknown' } }; }
  }
  override async pickVaultDir(): Promise<string | null> {
    const r = await invoke<string | string[] | null>('plugin:dialog|open', { options: { multiple: false, directory: true } });
    return Array.isArray(r) ? (r[0] ?? null) : r;
  }
  override async bindReport(): Promise<BindReport> { return (await commands.bindsGet()).report; }

  override async sessionStart(opts: StartOpts): Promise<SessionState> { return ok(await commands.sessionStart(opts)); }
  override async sessionStop(): Promise<void> { ok(await commands.sessionStop()); }
  override async sessionSetMic(on: boolean): Promise<SessionState> { return ok(await commands.sessionSetMic(on)); }
  override async sessionSetMicMode(mode: MicMode): Promise<SessionState> { return ok(await commands.sessionSetMicMode(mode)); }
  override onLiveLine(cb: (line: LiveLine) => void): () => void {
    let off: (() => void) | undefined;
    let dead = false;
    void on<SttEvent>(EVENTS.sttSegment, (e) => { if (e.kind === 'final' && e.text.trim()) cb({ speaker: e.stream, text: e.text.trim(), utt: e.utt }); }).then((u) => (dead ? u() : (off = u)));
    return () => { dead = true; off?.(); };
  }

  // ---- documents, search, summary ----
  static #listen<T>(name: string, cb: (p: T) => void): () => void {
    let off: (() => void) | undefined;
    let dead = false;
    void on<T>(name, cb).then((u) => (dead ? u() : (off = u)));
    return () => { dead = true; off?.(); };
  }
  override async docList(folder: FolderKey): Promise<Document[]> { return ok(await commands.docList(folderRef(folder))); }
  override async docGet(id: string): Promise<Document | null> { try { return ok(await commands.docGet(id)); } catch { return null; } }
  override async pickDocuments(): Promise<PickedFile[] | null> {
    const r = await invoke<string[] | string | null>('plugin:dialog|open', { options: { multiple: true, directory: false, filters: [{ name: 'Documents', extensions: ['pdf', 'docx', 'pptx', 'key', 'xlsx', 'csv', 'tsv', 'txt', 'md', 'png', 'jpg', 'jpeg', 'heic', 'webp', 'tif', 'tiff'] }] } });
    if (!r) return null;
    return (Array.isArray(r) ? r : [r]).map((path) => ({ path, name: path.split(/[\\/]/).pop() ?? path, size_bytes: 0 }));
  }
  override onFileDrop(cb: (files: PickedFile[]) => void): () => void {
    let off: (() => void) | undefined;
    let dead = false;
    void import('@tauri-apps/api/webview').then(({ getCurrentWebview }) => getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === 'drop') cb(e.payload.paths.map((path) => ({ path, name: path.split(/[\\/]/).pop() ?? path, size_bytes: 0 })));
    })).then((u) => (dead ? u() : (off = u))).catch(() => undefined);
    return () => { dead = true; off?.(); };
  }
  override async docImport(folder: FolderKey, files: PickedFile[], opts?: { pin?: boolean; onDuplicate?: 'copy' | 'replace' }): Promise<Document[]> {
    return ok(await commands.docImport(files.map((f) => ({ folder: folderRef(folder), source_path: f.path, title: null, pin: !!opts?.pin, tags: [], on_duplicate: opts?.onDuplicate ?? 'reject' }))));
  }
  override async docFindDuplicates(folder: FolderKey, files: PickedFile[]): Promise<Array<{ index: number; existing: Document }>> {
    return ok(await commands.docFindDuplicates(files.map((f) => ({ folder: folderRef(folder), source_path: f.path, title: null, pin: false, tags: [], on_duplicate: 'reject' as const }))));
  }
  override async docGetMd(id: string): Promise<string> { return ok(await commands.docGetMd(id)); }
  override async docSaveMd(id: string, md: string): Promise<Document> { return ok(await commands.docSaveMd(id, md)); }
  override async docPatch(id: string, patch: DocPatch): Promise<Document> { return ok(await commands.docPatch(id, patch)); }
  override async docDelete(id: string): Promise<void> { ok(await commands.docDelete(id)); }
  override async docRestore(id: string): Promise<Document> { return ok(await commands.docRestore(id)); }
  override async docRetry(id: string): Promise<Document> { return ok(await commands.docRetry(id)); }
  override async docCancel(id: string): Promise<void> { ok(await commands.docCancel(id)); }
  override async docsPinnedUsage(folder: FolderKey): Promise<PinnedUsage> { return ok(await commands.docsPinnedUsage(folderRef(folder))); }
  override onDocProgress(cb: (p: IngestProgress) => void): () => void { return TauriBackend.#listen('doc://progress', cb); }

  override async searchQuery(q: SearchQuery): Promise<SearchResults> { return ok(await commands.searchQuery(q)); }
  override async indexStatus(): Promise<IndexStatus> { return ok(await commands.indexStatus()); }
  override async indexRebuild(scope: ReindexScope): Promise<void> { ok(await commands.indexRebuild(scope)); }
  override onIndexStatus(cb: (s: IndexStatus) => void): () => void { return TauriBackend.#listen('index://status', cb); }
  override async embedderInfo(): Promise<EmbedderInfo> { return ok(await commands.embedderInfo()); }
  override async embedderInstall(): Promise<void> { ok(await commands.embedderInstall()); }
  override async embedderInstallCancel(): Promise<void> { ok(await commands.embedderInstallCancel()); }

  override async llmSummarize(call: string): Promise<void> { ok(await commands.llmSummarize(call)); }
  override async llmSummarizeCancel(call: string): Promise<void> { ok(await commands.llmSummarizeCancel(call)); }
  override onSummaryEvent(cb: (e: SummaryEvent) => void): () => void { return TauriBackend.#listen('summary://event', cb); }
  override async llmSuggestTags(call: string): Promise<TagSuggestion[]> { return ok(await commands.llmSuggestTags(call)); }
  override async callSetTags(id: string, tags: string[]): Promise<void> { ok(await commands.callSetTags(id, tags)); }
  override async llmProviderStatus(): Promise<LlmProviderStatus> { return ok(await commands.llmProviderStatus()); }
  override async llmKeyTest(): Promise<void> { ok(await commands.llmKeyTest()); }
  override async llmDetectLocal(): Promise<LocalServer[]> { return ok(await commands.llmDetectLocal()); }

  override async retxStatus(call: string): Promise<RetxStatus> { return ok(await commands.retxStatus(call)); }
  override async retxStart(call: string): Promise<void> { ok(await commands.retxStart(call)); }
  override async retxCancel(call: string): Promise<void> { ok(await commands.retxCancel(call)); }
  override async retxRestore(call: string, rev: number): Promise<void> { ok(await commands.retxRestore(call, rev)); }
  override onRetxEvent(cb: (e: RetxEvent) => void): () => void { return TauriBackend.#listen('retx://event', cb); }

  override async pickSavePath(defaultName: string): Promise<string | null> {
    return invoke<string | null>('plugin:dialog|save', { options: { defaultPath: defaultName, filters: [{ name: 'Zip', extensions: ['zip'] }] } });
  }
  override async pickOpenPath(extension: string): Promise<string | null> {
    const r = await invoke<string | string[] | null>('plugin:dialog|open', { options: { multiple: false, directory: false, filters: [{ name: extension, extensions: [extension] }] } });
    return Array.isArray(r) ? (r[0] ?? null) : r;
  }
  override async folderExport(folder: string, dest: string): Promise<number> { return ok(await commands.folderExport(folder, dest)); }
  override async folderImport(src: string): Promise<ImportSummary> { return ok(await commands.folderImport(src)); }
  override async exportVault(dest: string): Promise<number> { return ok(await commands.exportVault(dest, false)); }
  override async importVault(src: string): Promise<ImportSummary> { return ok(await commands.importVault(src)); }
}

// Real backend: every method that has a command in the generated bindings calls it. Only what the
// roadmap defers (documents/search index M2, LLM summary M3, native folder picker) keeps the mock.
import { commands, EVENTS, on } from '../../lib/ipc';
import type { AppError, BindReport, EngineDescriptor, EngineInfo, EngineProgress, FolderMeta, HwInfo, NewFolder, PermKind, PermState, SessionState, Settings, SourceInfo, StartOpts, SttEvent } from '../../lib/ipc';
import { MockBackend } from './mock';
import { folderKey, folderRef, type EngineNeeded, type CallDetail, type FolderDetail, type FolderKey, type LibrarySnapshot, type LiveLine, type SearchHit, type SecretProvider } from './types';

export class CommandError extends Error {
  constructor(readonly app: AppError) {
    super(app.code);
  }
}

type Res<T> = { status: 'ok'; data: T } | { status: 'error'; error: AppError };
function ok<T>(r: Res<T>): T {
  if (r.status === 'error') throw new CommandError(r.error);
  return r.data;
}

const PERMS: PermKind[] = ['microphone', 'system_audio', 'screen_recording', 'input_monitoring', 'accessibility'];

export class TauriBackend extends MockBackend {
  override readonly isMock: boolean = false;
  #vaultOpen = false;

  override async localeGet(): Promise<string> { return commands.localeGet(); }
  override async settingsGet(): Promise<Settings> { return commands.settingsGet(); }
  override async settingsPatch(patch: unknown): Promise<Settings> { return ok(await commands.settingsPatch(patch)); }

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
      return { meta: d.meta, instructions: d.instructions, docs: [] };
    } catch { return null; }
  }
  override async callGet(id: string): Promise<CallDetail | null> {
    try { return ok(await commands.callGet(id)) as CallDetail; } catch { return null; }
  }
  override async folderCreate(input: NewFolder): Promise<FolderMeta> { return ok(await commands.folderCreate(input)); }
  override async folderUpdate(id: string, patch: { instructions?: string; tags?: string[]; allow_cloud?: boolean }): Promise<void> {
    ok(await commands.folderUpdate(id, { title: null, tags: patch.tags ?? null, allow_cloud: patch.allow_cloud ?? null, default_template: null, instructions: patch.instructions ?? null }));
  }
  override async callTrash(id: string): Promise<void> { ok(await commands.callDelete(id)); }
  override async callRestore(id: string): Promise<void> { ok(await commands.callRestore(id)); }
  override async callMove(id: string, folder: FolderKey): Promise<void> { ok(await commands.callMove(id, folderRef(folder))); }
  override async callRename(id: string, title: string): Promise<void> { ok(await commands.callRename(id, title)); }
  override async trashEmpty(): Promise<void> { ok(await commands.trashEmpty()); }
  override async exportMarkdown(id: string): Promise<string> { return ok(await commands.callGetMd(id)); }
  override async search(query: string): Promise<SearchHit[]> {
    const q = query.trim().toLowerCase();
    const calls = ok(await commands.callList(null, false));
    return calls
      .filter((c) => !q || c.title.toLowerCase().includes(q) || c.tags.some((t) => t.toLowerCase().includes(q)))
      .slice(0, 12)
      .map((c) => ({ group: 'calls' as const, title: c.title, snippet: c.tags.map((t) => '#' + t).join(' '), folder: folderKey(c.folder), call: c.id }));
  }

  override async sourceList(): Promise<SourceInfo[]> { return ok(await commands.sourceList()); }
  override async sourceListInstalled(): Promise<SourceInfo[]> { return ok(await commands.sourceListInstalled()); }
  override async engines(): Promise<EngineDescriptor[]> { return commands.engineList(); }
  override async engineStatus(): Promise<EngineInfo[]> { return commands.engineStatus(); }
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
  override async secretCheck(p: SecretProvider): Promise<{ ok: boolean; balance?: string }> { return { ok: await this.secretStatus(p) }; }
  override async pickVaultDir(): Promise<string | null> { return null; }
  override async bindReport(): Promise<BindReport> { return (await commands.bindsGet()).report; }

  override async sessionStart(opts: StartOpts): Promise<SessionState> { return ok(await commands.sessionStart(opts)); }
  override async sessionStop(): Promise<void> { ok(await commands.sessionStop()); }
  override onLiveLine(cb: (line: LiveLine) => void): () => void {
    let off: (() => void) | undefined;
    let dead = false;
    void on<SttEvent>(EVENTS.sttSegment, (e) => { if (e.kind === 'final' && e.text.trim()) cb({ speaker: e.stream, text: e.text.trim(), utt: e.utt }); }).then((u) => (dead ? u() : (off = u)));
    return () => { dead = true; off?.(); };
  }
}

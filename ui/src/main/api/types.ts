// View-model types for the main window. DTOs that already exist in the generated
// bindings are reused as-is; the rest are the shapes WP6 will need to provide.
import type {
  BindReport,
  CallMeta,
  EngineDescriptor,
  EngineInfo,
  EngineProgress,
  FolderMeta,
  FolderRef,
  HwInfo,
  NewFolder,
  Os,
  PermKind,
  PermState,
  SessionState,
  Settings,
  SourceInfo,
  StartOpts,
  Stream,
} from '../../lib/ipc';

export const SYS_UNSORTED = 'sys:unsorted';
export const SYS_TRASH = 'sys:trash';

/** Router/UI key of a folder: ULID for user folders, reserved ids for the system ones. */
export type FolderKey = string;

export function folderKey(ref: FolderRef): FolderKey {
  return ref.kind === 'user' ? ref.id : ref.kind === 'unsorted' ? SYS_UNSORTED : SYS_TRASH;
}
export function folderRef(key: FolderKey): FolderRef {
  return key === SYS_UNSORTED ? { kind: 'unsorted' } : key === SYS_TRASH ? { kind: 'trash' } : { kind: 'user', id: key };
}
export const isSystemKey = (key: FolderKey): boolean => key === SYS_UNSORTED || key === SYS_TRASH;

export type Utterance = { at_s: number; speaker: Stream; text: string };

/** One «Вопрос → Подсказка ИИ → Мой ответ» chain (`lowk-qa` callout). */
export type QaItem = {
  id: string;
  at_s: number;
  question: string;
  question_stream: Stream;
  answer: string;
  /** Display name of the prompt template that produced the answer. */
  template: string;
  my_answer: string | null;
  my_answer_at_s: number | null;
};

export type ShotItem = {
  id: string;
  at_s: number;
  /** Relative path inside the call's `.assets/`; the mock renders `title` as a placeholder. */
  rel_path: string;
  title: string;
  caption: string;
  source: 'manual' | 'auto';
};

export type CallDetail = {
  meta: CallMeta;
  summary: string | null;
  transcript: Utterance[];
  qa: QaItem[];
  shots: ShotItem[];
  keep_audio: boolean;
  /** Answering model shown in the properties pane. */
  model: string;
};

export type DocItem = { id: string; name: string; ext: 'pdf' | 'docx' | 'md' | 'txt'; pages: number | null; state: 'ready' | 'converted' | 'indexing' };

export type FolderDetail = { meta: FolderMeta; instructions: string; docs: DocItem[] };

/** `summarized`: ids of calls that already have a `## Summary` (will become a `CallMeta` field). */
export type LibrarySnapshot = { folders: FolderMeta[]; calls: CallMeta[]; summarized: string[] };

export type SearchHit = { group: 'calls' | 'docs'; title: string; snippet: string; folder: FolderKey; call: string | null };

export type SecretProvider = 'openrouter' | 'stt-cloud';

/** `utt` identifies the utterance within its stream: a corrected re-emit replaces the shown line. */
export type LiveLine = { speaker: Stream; text: string; utt?: number };

export interface Backend {
  readonly isMock: boolean;
  platform(): Os;
  localeGet(): Promise<string>;
  settingsGet(): Promise<Settings>;
  settingsPatch(patch: unknown): Promise<Settings>;

  libraryList(): Promise<LibrarySnapshot>;
  folderGet(key: FolderKey): Promise<FolderDetail | null>;
  callGet(id: string): Promise<CallDetail | null>;
  folderCreate(input: NewFolder): Promise<FolderMeta>;
  folderUpdate(id: string, patch: { instructions?: string; tags?: string[]; allow_cloud?: boolean }): Promise<void>;
  docAdd(folderId: string, fileName: string): Promise<DocItem>;
  callTrash(id: string): Promise<void>;
  callRestore(id: string): Promise<void>;
  callMove(id: string, folder: FolderKey): Promise<void>;
  callRename(id: string, title: string): Promise<void>;
  trashEmpty(): Promise<void>;
  summaryGenerate(id: string): Promise<string>;
  exportMarkdown(id: string): Promise<string>;
  search(query: string): Promise<SearchHit[]>;

  /** Running apps that produce audio right now (G3). */
  sourceList(): Promise<SourceInfo[]>;
  /** Installed apps for “Other…” (G3). */
  sourceListInstalled(): Promise<SourceInfo[]>;
  engines(): Promise<EngineDescriptor[]>;
  /** Fitness / installed / licence per engine (model download card). */
  engineStatus(): Promise<EngineInfo[]>;
  /** Downloads the engine's models (resumable, SHA-256 checked); resolves when ready, rejects when failed or cancelled. */
  engineInstall(id: string): Promise<void>;
  engineInstallCancel(): Promise<void>;
  onEngineProgress(cb: (p: EngineProgress) => void): () => void;
  hwProbe(): Promise<HwInfo>;
  permStatus(): Promise<Record<PermKind, PermState>>;
  permRequest(kind: PermKind): Promise<PermState>;
  permOpenSettings(kind: PermKind): Promise<void>;
  /** Quit and start Lowk again (macOS applies some grants only to a fresh process). */
  appRelaunch(): Promise<void>;
  onEngineNeeded(cb: (e: EngineNeeded) => void): () => void;
  /** `binds://report`: live status of every bind (degraded = a permission is missing). */
  onBindReport(cb: (r: BindReport) => void): () => void;
  secretStatus(provider: SecretProvider): Promise<boolean>;
  secretSet(provider: SecretProvider, value: string): Promise<void>;
  secretDelete(provider: SecretProvider): Promise<void>;
  secretCheck(provider: SecretProvider): Promise<{ ok: boolean; balance?: string }>;
  pickVaultDir(): Promise<string | null>;
  revealPath(path: string): Promise<void>;
  bindReport(): Promise<BindReport>;

  sessionStart(opts: StartOpts): Promise<SessionState>;
  sessionStop(): Promise<void>;
  /** Live transcript feed for the call page while recording. Returns an unsubscribe fn. */
  onLiveLine(cb: (line: LiveLine) => void): () => void;
}

/** `engine://needed`: a real call cannot start; what the prompt should offer. */
export type EngineNeeded = { engine: string; kind: 'local' | 'cloud' | 'none'; size_mb: number };

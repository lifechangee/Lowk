// View-model types for the main window. DTOs that already exist in the generated
// bindings are reused as-is; the rest are the shapes the backend provides.
import type {
  AppError,
  BindReport,
  CallMeta,
  DocPatch,
  Document,
  EmbedderInfo,
  EngineDescriptor,
  EngineInfo,
  EngineProgress,
  FolderMeta,
  FolderRef,
  HwInfo,
  ImportSummary,
  IndexStatus,
  IngestProgress,
  LlmProviderStatus,
  LocalServer,
  MicMode,
  MicPreset,
  NewFolder,
  Os,
  PermKind,
  PermState,
  PinnedUsage,
  ReindexScope,
  RetxEvent,
  RetxPreset,
  RetxStatus,
  SearchQuery,
  SearchResults,
  SessionState,
  SttDiagnostics,
  Settings,
  SourceInfo,
  SourceRef,
  StartOpts,
  Stream,
  SummaryEvent,
  TagSuggestion,
} from '../../lib/ipc';

export type { RetxEvent, RetxPreset, RetxStatus, RetxRevision } from '../../lib/ipc';
export type { SearchResults, SearchHit, Document, IngestProgress, PinnedUsage, IndexStatus, EmbedderInfo, TagSuggestion, SummaryEvent, ImportSummary, SourceRef, LlmProviderStatus, LocalServer, LocalModel } from '../../lib/ipc';

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
  /** What the answer was grounded on (`llm://sources`): documents, previous calls, instructions. */
  sources?: SourceRef[];
  /** Chunks withheld because the folder is local-only (`allow_cloud = false`). */
  withheld?: number;
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
  /** Union of the sources of every AI answer in the call (Sources tab). */
  sources?: SourceRef[];
};

export type FolderDetail = { meta: FolderMeta; instructions: string };

/** `summarized`: ids of calls that already have a `## Summary` (will become a `CallMeta` field). */
export type LibrarySnapshot = { folders: FolderMeta[]; calls: CallMeta[]; summarized: string[] };

/** A file chosen in the file dialog or dropped on the window (`NewDocument.source_path`). */
export type PickedFile = { name: string; path: string; /** 0 = unknown (native drop: the backend warns with `large_file`). */ size_bytes: number };

/** `AppError` thrown by a failed command (tauri.ts) or a mock failure. */
export class CommandError extends Error {
  constructor(readonly app: AppError) {
    super(app.code);
  }
}

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
  folderUpdate(id: string, patch: { instructions?: string; tags?: string[]; allow_cloud?: boolean; mic?: MicPreset; retx?: RetxPreset }): Promise<void>;
  callTrash(id: string): Promise<void>;
  callRestore(id: string): Promise<void>;
  callMove(id: string, folder: FolderKey): Promise<void>;
  callRename(id: string, title: string): Promise<void>;
  trashEmpty(): Promise<void>;
  exportMarkdown(id: string): Promise<string>;

  // ---- documents ----
  docList(folder: FolderKey): Promise<Document[]>;
  docGet(id: string): Promise<Document | null>;
  /** The file dialog. `null` = cancelled. Rejects with `not_implemented` where the shell has no dialog yet. */
  pickDocuments(): Promise<PickedFile[] | null>;
  /** Files dropped on the window (native drop gives paths; a plain browser uses the HTML5 drop in the component). */
  onFileDrop(cb: (files: PickedFile[]) => void): () => void;
  /** `onDuplicate` applies to files whose content is already in the folder; without it such a file is refused (`doc_duplicate`). */
  docImport(folder: FolderKey, files: PickedFile[], opts?: { pin?: boolean; onDuplicate?: 'copy' | 'replace' }): Promise<Document[]>;
  /** Files of the batch whose content is already in the folder, with the existing document. */
  docFindDuplicates(folder: FolderKey, files: PickedFile[]): Promise<Array<{ index: number; existing: Document }>>;
  docGetMd(id: string): Promise<string>;
  docSaveMd(id: string, md: string): Promise<Document>;
  docPatch(id: string, patch: DocPatch): Promise<Document>;
  docDelete(id: string): Promise<void>;
  docRestore(id: string): Promise<Document>;
  docRetry(id: string): Promise<Document>;
  docCancel(id: string): Promise<void>;
  docsPinnedUsage(folder: FolderKey): Promise<PinnedUsage>;
  /** `doc://progress` */
  onDocProgress(cb: (p: IngestProgress) => void): () => void;

  // ---- search and embedder ----
  searchQuery(q: SearchQuery): Promise<SearchResults>;
  indexStatus(): Promise<IndexStatus>;
  indexRebuild(scope: ReindexScope): Promise<void>;
  /** `index://status` */
  onIndexStatus(cb: (s: IndexStatus) => void): () => void;
  embedderInfo(): Promise<EmbedderInfo>;
  /** Resolves when installed; progress arrives through `onEngineProgress` with `engine === 'embedder'`. */
  embedderInstall(): Promise<void>;
  embedderInstallCancel(): Promise<void>;

  // ---- summary, tags, provider ----
  /** Starts the stream; the events arrive through `onSummaryEvent`. */
  llmSummarize(call: string): Promise<void>;
  llmSummarizeCancel(call: string): Promise<void>;
  /** `summary://event` */
  onSummaryEvent(cb: (e: SummaryEvent) => void): () => void;
  llmSuggestTags(call: string): Promise<TagSuggestion[]>;
  callSetTags(id: string, tags: string[]): Promise<void>;
  llmProviderStatus(): Promise<LlmProviderStatus>;
  /** Probes 127.0.0.1:11434 / :1234 / :8080 (<= 300 ms, no prompts); empty = nothing running. */
  llmDetectLocal(): Promise<LocalServer[]>;
  /** Rejects with the backend error (`api_key_missing`, `network`, `llm_provider`, ...). */
  llmKeyTest(): Promise<void>;

  // ---- M3: re-transcription of a saved recording ----
  /** Saved audio present?, a pass running / paused, progress, transcript revisions. */
  retxStatus(call: string): Promise<RetxStatus>;
  /** Starts the background pass; progress arrives through `onRetxEvent`. Rejects with `not_found` (no audio), `local_only_blocked`, `engine_not_ready`, `api_key_missing`, `session_busy`. */
  retxStart(call: string): Promise<void>;
  retxCancel(call: string): Promise<void>;
  /** Sets an older revision (the live transcript) back as the call text. */
  retxRestore(call: string, rev: number): Promise<void>;
  /** `retx://event` */
  onRetxEvent(cb: (e: RetxEvent) => void): () => void;

  // ---- export / import ----
  pickSavePath(defaultName: string): Promise<string | null>;
  pickOpenPath(extension: string): Promise<string | null>;
  folderExport(folder: string, dest: string): Promise<number>;
  folderImport(src: string): Promise<ImportSummary>;
  exportVault(dest: string): Promise<number>;
  importVault(src: string): Promise<ImportSummary>;

  /** Running apps that produce audio right now (G3). */
  sourceList(): Promise<SourceInfo[]>;
  /** Installed apps for “Other…” (G3). */
  sourceListInstalled(): Promise<SourceInfo[]>;
  engines(): Promise<EngineDescriptor[]>;
  /** Fitness / installed / licence per engine (model download card). */
  engineStatus(): Promise<EngineInfo[]>;
  /** STT health incl. the compute backend of the local worker (`cpu_fallback` = running on the CPU). */
  sttDiagnostics(): Promise<SttDiagnostics | null>;
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
  secretCheck(provider: SecretProvider): Promise<{ ok: boolean; balance?: string; error?: AppError }>;
  pickVaultDir(): Promise<string | null>;
  revealPath(path: string): Promise<void>;
  /** Opens the folder with lowk.log (support). */
  logsReveal(): Promise<void>;
  bindReport(): Promise<BindReport>;

  sessionStart(opts: StartOpts): Promise<SessionState>;
  sessionStop(): Promise<void>;
  /** Microphone on/off during a recording (the recording and the system audio keep going). */
  sessionSetMic(on: boolean): Promise<SessionState>;
  /** Microphone mode during a recording: on, off or auto (only while the call app holds the microphone). */
  sessionSetMicMode(mode: MicMode): Promise<SessionState>;
  /** Live transcript feed for the call page while recording. Returns an unsubscribe fn. */
  onLiveLine(cb: (line: LiveLine) => void): () => void;
}

/** `engine://needed`: a real call cannot start; what the prompt should offer. */
export type EngineNeeded = { engine: string; kind: 'local' | 'cloud' | 'none'; size_mb: number };

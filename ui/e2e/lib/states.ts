// State matrix for the visual + layout suite. Ids are stable (baseline file names).
export const LANGS = ['ru', 'en'] as const;
export const SIZES = [
  { id: 'L', w: 1280, h: 800 },
  { id: 'S', w: 960, h: 640 },
] as const;
export type Lang = (typeof LANGS)[number];

const ACME = '01JC8Z3K6Q9X2M4N7R5T0V0001';
const CALL1 = '01JC8Z3K6Q9X2M4N7R5T0V0101';
const CALL11 = '01JC8Z3K6Q9X2M4N7R5T0V0111';
const CALL3 = '01JC8Z3K6Q9X2M4N7R5T0V0103';
const NOVA = '01JC8Z3K6Q9X2M4N7R5T0V0002';
const HIRE = '01JC8Z3K6Q9X2M4N7R5T0V0003';
const EMPTY = '01JC8Z3K6Q9X2M4N7R5T0V0005';
const DOC_PDF = '01JC8Z3K6Q9X2M4N7R5T0V0401';
const DOC_BUSY = '01JC8Z3K6Q9X2M4N7R5T0V0408';
const DOC_FAIL = '01JC8Z3K6Q9X2M4N7R5T0V0409';

export interface MainState {
  id: string;
  title: string;
  hash: string;
  query?: string;
  /** After load: interactions */
  /** Extra query string (mock switches), e.g. `&summary=hold`. */
  q?: string;
  act?: string;
  mockup: 'mn' | 'ob';
}

export const MAIN_STATES: MainState[] = [
  { id: 'home', title: 'Library home', hash: '#/', mockup: 'mn' },
  { id: 'home-recording', title: 'Home while recording (demo)', hash: '#/', act: 'recording', mockup: 'mn' },
  { id: 'home-drag', title: 'Home: dragging a call over a folder (drop highlight)', hash: '#/', act: 'drag', mockup: 'mn' },
  { id: 'folder', title: 'Folder workspace', hash: `#/folder/${ACME}`, mockup: 'mn' },
  { id: 'unsorted', title: 'Unsorted', hash: '#/folder/sys:unsorted', mockup: 'mn' },
  { id: 'unsorted-menu', title: 'Unsorted: «Move to…» menu', hash: '#/folder/sys:unsorted', act: 'move-menu', mockup: 'mn' },
  { id: 'trash', title: 'Trash', hash: '#/folder/sys:trash', mockup: 'mn' },
  { id: 'call-transcript', title: 'Call: transcript tab', hash: `#/call/${CALL1}`, mockup: 'mn' },
  { id: 'call-summary', title: 'Call: summary tab', hash: `#/call/${CALL1}`, act: 'tab-summary', mockup: 'mn' },
  { id: 'call-qa', title: 'Call: AI questions tab', hash: `#/call/${CALL1}`, act: 'tab-qa', mockup: 'mn' },
  { id: 'call-shots', title: 'Call: screenshots tab', hash: `#/call/${CALL1}`, act: 'tab-shots', mockup: 'mn' },
  { id: 'call-longtitle', title: 'Call with a very long title', hash: `#/call/${CALL11}`, mockup: 'mn' },
  ...(['general', 'hotkeys', 'templates', 'appearance', 'providers', 'storage', 'permissions'] as const).map((p) => ({ id: `settings-${p}`, title: `Settings: ${p}`, hash: `#/settings/${p}`, mockup: 'mn' as const })),
  { id: 'settings-templates-edit', title: 'Templates: editor of the complement template', hash: '#/settings/templates', act: 'tpl-edit', mockup: 'mn' },
  { id: 'settings-templates-new', title: 'Templates: new template with validation', hash: '#/settings/templates', act: 'tpl-new', mockup: 'mn' },
  { id: 'settings-templates-delete', title: 'Templates: delete confirmation', hash: '#/settings/templates', act: 'tpl-delete', mockup: 'mn' },
  { id: 'newcall-popover', title: 'G5: header «New call» popover (folder + source + Start)', hash: '#/settings/general', act: 'new-call-popover', mockup: 'mn' },
  { id: 'source-picker', title: 'G3: source picker open (running apps, presets, Other…)', hash: '#/', act: 'source-picker', mockup: 'mn' },
  { id: 'source-other', title: 'G3: «Other…» installed-apps list', hash: '#/', act: 'source-other', mockup: 'mn' },
  { id: 'search', title: 'Search palette (Cmd+K), empty: recent calls', hash: '#/', act: 'search', mockup: 'mn' },
  // ---- documents, search, summary ----
  { id: 'home-empty', title: 'home, empty library', hash: '#/', q: '&empty=1', mockup: 'mn' },
  { id: 'unsorted-empty', title: 'unsorted, empty', hash: '#/folder/sys:unsorted', q: '&empty=1', mockup: 'mn' },
  { id: 'trash-empty', title: 'trash, empty', hash: '#/folder/sys:trash', q: '&empty=1', mockup: 'mn' },
  { id: 'folder-calls-empty', title: 'folder without calls', hash: `#/folder/${EMPTY}`, mockup: 'mn' },
  { id: 'folder-docs', title: 'documents tab: statuses, warnings, error, pinned meter', hash: `#/folder/${ACME}/documents`, mockup: 'mn' },
  { id: 'folder-docs-overflow', title: 'documents tab: pinned budget exceeded', hash: `#/folder/${HIRE}/documents`, mockup: 'mn' },
  { id: 'folder-docs-empty', title: 'documents tab: empty', hash: `#/folder/${EMPTY}/documents`, mockup: 'mn' },
  { id: 'folder-docs-slow', title: 'slow-file warning', hash: `#/folder/${EMPTY}/documents`, q: '&warnmb=1', act: 'slow-warning', mockup: 'mn' },
  { id: 'folder-docs-menu', title: 'document row menu', hash: `#/folder/${NOVA}/documents`, act: 'doc-menu', mockup: 'mn' },
  { id: 'folder-instructions', title: 'instructions tab', hash: `#/folder/${ACME}/instructions`, mockup: 'mn' },
  { id: 'folder-instructions-empty', title: 'instructions tab: empty with example', hash: `#/folder/${EMPTY}/instructions`, mockup: 'mn' },
  { id: 'doc-view', title: 'document viewer at page 3', hash: `#/doc/${DOC_PDF}?p=3`, mockup: 'mn' },
  { id: 'doc-edit', title: 'document editor', hash: `#/doc/${DOC_PDF}`, act: 'doc-edit', mockup: 'mn' },
  { id: 'doc-converting', title: 'document still converting', hash: `#/doc/${DOC_BUSY}`, mockup: 'mn' },
  { id: 'doc-failed', title: 'document failed (encrypted)', hash: `#/doc/${DOC_FAIL}`, mockup: 'mn' },
  { id: 'call-sources', title: 'call: sources tab', hash: `#/call/${CALL1}`, act: 'tab-sources', mockup: 'mn' },
  { id: 'call-sources-empty', title: 'call: sources tab empty', hash: `#/call/${CALL3}`, act: 'tab-sources', mockup: 'mn' },
  { id: 'call-at-minute', title: 'call opened at a minute (search hit)', hash: `#/call/${CALL1}?t=1424`, mockup: 'mn' },
  { id: 'call-menu', title: 'call header menu', hash: `#/call/${CALL1}`, act: 'call-menu', mockup: 'mn' },
  { id: 'call-props', title: 'call properties panel', hash: `#/call/${CALL1}`, act: 'call-props', mockup: 'mn' },
  { id: 'summary-empty', title: 'summary: not created yet', hash: `#/call/${CALL1}`, act: 'tab-summary', mockup: 'mn' },
  { id: 'summary-streaming', title: 'summary: streaming', hash: `#/call/${CALL1}`, q: '&summary=hold', act: 'summary-stream', mockup: 'mn' },
  { id: 'summary-done', title: 'summary: done', hash: `#/call/${CALL1}`, act: 'summary-done', mockup: 'mn' },
  { id: 'summary-nokey', title: 'summary: no OpenRouter key', hash: `#/call/${CALL1}`, q: '&summary=nokey', act: 'summary-error', mockup: 'mn' },
  { id: 'summary-offline', title: 'summary: offline error', hash: `#/call/${CALL1}`, q: '&summary=offline', act: 'summary-error', mockup: 'mn' },
  { id: 'palette-results', title: 'palette with results', hash: '#/', act: 'palette-q:EBITDA', mockup: 'mn' },
  { id: 'palette-none', title: 'palette: nothing found', hash: '#/', act: 'palette-q:zzzzzz', mockup: 'mn' },
  { id: 'palette-keyword', title: 'palette: keyword-only notice (embedder missing)', hash: '#/', q: '&embedder=missing', act: 'palette-q:EBITDA', mockup: 'mn' },
  { id: 'search-page', title: 'search results page', hash: '#/search?q=EBITDA', mockup: 'mn' },
  { id: 'search-page-filtered', title: 'search page with filters', hash: `#/search?q=бюджет&folder=${ACME}&kind=doc`, mockup: 'mn' },
  { id: 'search-page-empty', title: 'search page, no query', hash: '#/search', mockup: 'mn' },
  { id: 'search-page-none', title: 'search page, nothing found', hash: '#/search?q=zzzzzz', mockup: 'mn' },
  { id: 'search-page-keyword', title: 'search page, keyword-only (semantic index building)', hash: '#/search?q=EBITDA', q: '&embedder=building', mockup: 'mn' },
  { id: 'settings-knowledge', title: 'settings: documents & search (embedder ready)', hash: '#/settings/knowledge', mockup: 'mn' },
  { id: 'settings-knowledge-missing', title: 'embedder not installed (licence first)', hash: '#/settings/knowledge', q: '&embedder=missing', mockup: 'mn' },
  { id: 'settings-knowledge-installing', title: 'embedder downloading', hash: '#/settings/knowledge', q: '&embedder=hold', act: 'embedder-install', mockup: 'mn' },
  { id: 'settings-knowledge-failed', title: 'embedder download failed', hash: '#/settings/knowledge', q: '&embedder=fail', act: 'embedder-fail', mockup: 'mn' },
  { id: 'settings-knowledge-building', title: 'semantic index building', hash: '#/settings/knowledge', q: '&embedder=building', mockup: 'mn' },
  { id: 'settings-knowledge-off', title: 'embedder off', hash: '#/settings/knowledge', q: '&embedder=off', mockup: 'mn' },
  { id: 'settings-storage-export', title: 'vault export result', hash: '#/settings/storage', act: 'export-vault', mockup: 'mn' },
  { id: 'settings-storage-import', title: 'vault import result', hash: '#/settings/storage', act: 'import-vault', mockup: 'mn' },
  { id: 'settings-storage-import-fail', title: 'import failed (invalid archive)', hash: '#/settings/storage', q: '&import=fail', act: 'import-vault', mockup: 'mn' },
  { id: 'settings-providers-key', title: 'LLM provider card: key test fails', hash: '#/settings/providers', q: '&key=bad', act: 'key-test', mockup: 'mn' },
  { id: 'tour-offer', title: 'tour offered after onboarding', hash: '#/onboarding/8', q: '&fresh&ack', act: 'tour-offer', mockup: 'mn' },
  ...Array.from({ length: 6 }, (_, i) => ({ id: `tour-${i + 1}`, title: `tour step ${i + 1} / 6`, hash: '#/', act: `tour-${i}`, mockup: 'mn' as const })),
  { id: 'tour-3-try', title: 'tour step 3: demo answer streamed', hash: '#/', act: 'tour-try-ask', mockup: 'mn' },
  { id: 'tour-4-try', title: 'tour step 4: overlay follows the cursor', hash: '#/', act: 'tour-try-hold', mockup: 'mn' },
  { id: 'tour-5-try', title: 'tour step 5: screenshot counter', hash: '#/', act: 'tour-try-shot', mockup: 'mn' },
  ...Array.from({ length: 8 }, (_, i) => ({ id: `onboarding-${i}`, title: `Onboarding step ${i + 1}`, hash: `#/onboarding/${i}`, mockup: 'ob' as const })),
];

export type Bg = 'dark' | 'light' | 'color' | 'split';
export interface OverlayState {
  id: string;
  title: string;
  q: Record<string, string>;
  /** pill window viewport */
  pill?: boolean;
  /** keyboard presses after load */
  keys?: string[];
  full: boolean; // full bg x opacity grid
  /** Only these backdrops (default: dark, light, color). */
  bgs?: Bg[];
}

export const BGS: Bg[] = ['dark', 'light', 'color'];
export const OPACITIES = [0, 60, 90] as const;

export const OVERLAY_STATES: OverlayState[] = [
  { id: 'passive', title: 'Passive (answer + «Мой ответ»)', q: { still: '1' }, full: true },
  { id: 'interactive-menu', title: 'Interactive, template menu open', q: { still: '1', mode: 'interactive', menu: '1' }, full: true },
  { id: 'interactive', title: 'Interactive (input row)', q: { still: '1', mode: 'interactive' }, full: false },
  { id: 'hover', title: 'G6: hold-Ctrl clickable mode', q: { still: '1', mode: 'hover' }, full: false },
  { id: 'moving', title: 'Moving (hold-to-move)', q: { still: '1', mode: 'moving' }, full: false },
  { id: 'pill', title: 'Pill', q: { still: '1', view: 'pill' }, pill: true, full: false },
  { id: 'adaptive', title: 'Adaptive tone grid (dark left, light right, bright bottom)', q: { still: '1' }, full: false, bgs: ['split'] },
  { id: 'adaptive-moving', title: 'Adaptive grid + moving badge', q: { still: '1', mode: 'moving' }, full: false, bgs: ['split'] },
  { id: 'toast', title: 'Toast (screenshot added)', q: { still: '1' }, keys: ['s'], full: false },
];

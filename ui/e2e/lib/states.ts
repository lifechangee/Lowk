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

export interface MainState {
  id: string;
  title: string;
  hash: string;
  query?: string;
  /** After load: interactions */
  act?: 'tab-summary' | 'tab-qa' | 'tab-shots' | 'search' | 'recording' | 'move-menu' | 'drag' | 'new-call-popover' | 'source-picker' | 'source-other';
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
  ...(['general', 'hotkeys', 'appearance', 'providers', 'storage', 'permissions'] as const).map((p) => ({ id: `settings-${p}`, title: `Settings: ${p}`, hash: `#/settings/${p}`, mockup: 'mn' as const })),
  { id: 'newcall-popover', title: 'G5: header «New call» popover (folder + source + Start)', hash: '#/settings/general', act: 'new-call-popover', mockup: 'mn' },
  { id: 'source-picker', title: 'G3: source picker open (running apps, presets, Other…)', hash: '#/', act: 'source-picker', mockup: 'mn' },
  { id: 'source-other', title: 'G3: «Other…» installed-apps list', hash: '#/', act: 'source-other', mockup: 'mn' },
  { id: 'search', title: 'Search palette (Cmd+K)', hash: '#/', act: 'search', mockup: 'mn' },
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

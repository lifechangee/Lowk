// Capture-source helpers shared by the home screen, the header popover, onboarding and settings
// (F12, G3). A source value is `app:<id>` or `system`; ids are bundle ids (macOS) / exe names (Windows)
// and are never typed by the user: they come from the running-apps list, a preset or the installed list.
import type { CaptureSource, Os, SourceInfo, Target } from '../lib/ipc';

export type SourceValue = string;

/** Presets people actually record in. `mac` = bundle id, `win` = exe name, `match` finds the app in a list by name. */
export interface Preset {
  key: string;
  name: string;
  mac: string;
  win: string;
  match: RegExp;
}

export const PRESETS: Preset[] = [
  { key: 'zoom', name: 'Zoom', mac: 'us.zoom.xos', win: 'Zoom.exe', match: /^zoom(\.us)?$/i },
  { key: 'meet', name: 'Google Meet (Chrome)', mac: 'com.google.Chrome', win: 'chrome.exe', match: /^(google )?chrome$/i },
  { key: 'teams', name: 'Microsoft Teams', mac: 'com.microsoft.teams2', win: 'ms-teams.exe', match: /teams/i },
  { key: 'discord', name: 'Discord', mac: 'com.hnc.Discord', win: 'Discord.exe', match: /^discord$/i },
  { key: 'telegram', name: 'Telegram', mac: 'ru.keepcoder.Telegram', win: 'Telegram.exe', match: /^telegram/i },
  { key: 'slack', name: 'Slack', mac: 'com.tinyspeck.slackmacgap', win: 'slack.exe', match: /^slack$/i },
  { key: 'facetime', name: 'FaceTime', mac: 'com.apple.FaceTime', win: '', match: /^facetime$/i },
];

/** Presets that exist on this OS (FaceTime is macOS only). */
export const presetsFor = (os: Os): Preset[] => PRESETS.filter((p) => (os === 'windows' ? !!p.win : !!p.mac));

const same = (s: SourceInfo, p: Preset, os: Os): boolean =>
  s.id === (os === 'windows' ? p.win : p.mac) || s.bundle_id === p.mac || (os === 'windows' && s.id.toLowerCase().endsWith(p.win.toLowerCase())) || p.match.test(s.name);

/** A preset as a `SourceInfo`: the real entry when the app is running / installed, else a static one. */
export function presetInfo(p: Preset, os: Os, pool: SourceInfo[]): SourceInfo {
  const hit = pool.find((s) => s.kind === 'app' && same(s, p, os));
  if (hit) return hit.name === p.name || p.key !== 'meet' ? hit : { ...hit, name: p.name };
  const id = os === 'windows' ? p.win : p.mac;
  return { id, name: p.name, bundle_id: os === 'windows' ? null : p.mac, pid: null, icon_png_b64: null, producing_audio: false, kind: 'app' };
}

export const toValue = (s: SourceInfo): SourceValue => (s.kind === 'system' ? 'system' : `app:${s.id}`);

export const SYSTEM_SOURCE: SourceInfo = { id: 'system', name: '', bundle_id: null, pid: null, icon_png_b64: null, producing_audio: false, kind: 'system' };

/** The default `app:<id>` / `system` for a recording. A stored app wins (it need not be running: it is a
 *  choice, not a probe); `capture.default_source = whole_system` wins; otherwise the first app that is
 *  producing audio right now; nothing producing audio = the whole system. */
export function resolveSource(stored: Target | null | undefined, running: SourceInfo[], mode?: CaptureSource): SourceValue {
  if (mode === 'whole_system' || stored?.kind === 'whole_system') return 'system';
  if (stored?.kind === 'app') return `app:${stored.id}`;
  const first = running.find((a) => a.kind === 'app' && a.producing_audio);
  return first ? `app:${first.id}` : 'system';
}

/** Tolerant mapper: the backend may still answer with the older `AudioApp` shape (`pids`). */
export function toSourceInfo(x: Partial<SourceInfo> & { id: string; name: string; pids?: number[] }): SourceInfo {
  return {
    id: x.id,
    name: x.name,
    bundle_id: x.bundle_id ?? null,
    pid: x.pid ?? x.pids?.[0] ?? null,
    icon_png_b64: x.icon_png_b64 ?? null,
    producing_audio: !!x.producing_audio,
    kind: x.kind ?? 'app',
  };
}

/** Last-resort display name for an id that is in no list (`us.zoom.xos` -> `xos`; exe names lose `.exe`). */
export function nameFromId(id: string): string {
  const tail = id.split(/[\\/]/).pop() ?? id;
  return tail.replace(/\.exe$/i, '');
}

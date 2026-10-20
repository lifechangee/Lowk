// Capture-source default shared by the home screen, onboarding and settings (F12).
import type { AudioApp, CaptureSource, Target } from '../lib/ipc';

/** `app:<id>` or `system`. A stored app wins; `capture.default_source` = whole_system wins; otherwise
 *  (default `one_app`) the recommended single app (first in the list); no apps = whole system. */
export function resolveSource(stored: Target | null | undefined, apps: AudioApp[], mode?: CaptureSource): string {
  if (mode !== 'whole_system' && stored?.kind === 'app' && apps.some((a) => a.id === stored.id)) return `app:${stored.id}`;
  if (mode === 'whole_system' || stored?.kind === 'whole_system' || apps.length === 0) return 'system';
  return `app:${apps[0]!.id}`;
}

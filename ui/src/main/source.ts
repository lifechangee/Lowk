// Capture-source default shared by the home screen, onboarding and settings (F12).
import type { AudioApp, Target } from '../lib/ipc';

/** `app:<id>` or `system`. A stored app wins, a stored «whole system» wins; with nothing stored the
 *  recommended single app (first in the list) is the default, as onboarding promises. */
export function resolveSource(stored: Target | null | undefined, apps: AudioApp[]): string {
  if (stored?.kind === 'app' && apps.some((a) => a.id === stored.id)) return `app:${stored.id}`;
  if (stored?.kind === 'whole_system' || apps.length === 0) return 'system';
  return `app:${apps[0]!.id}`;
}

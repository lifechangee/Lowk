import { tm } from './i18n';
import type { MouseNames } from './binds';

/** Localized mouse button names for bind formatting (call inside a reactive context). */
export function mouseNames(): MouseNames {
  return {
    middle: tm('m.mouse.middle'),
    back: tm('m.mouse.back'),
    forward: tm('m.mouse.forward'),
    other: (n) => tm('m.mouse.other', { n }),
  };
}

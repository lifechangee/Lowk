import { common } from './common';
import { errors } from './errors';
import { main } from './main';
import { onboarding } from './onboarding';
import { overlay } from './overlay';
import { settings } from './settings';

// Source of truth for keys (en/index.ts must have the same set; see i18n.test.ts).
// One file per feature namespace so two features rarely touch the same file.
export const dict = { ...common, ...errors, ...main, ...overlay, ...settings, ...onboarding } as const;

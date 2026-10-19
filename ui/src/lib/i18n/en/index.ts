import { common } from './common';
import { errors } from './errors';
import { main } from './main';
import { onboarding } from './onboarding';
import { overlay } from './overlay';
import { settings } from './settings';

// Must mirror ru/index.ts key-for-key (enforced by i18n.test.ts).
export const dict = { ...common, ...errors, ...main, ...overlay, ...settings, ...onboarding } as const;

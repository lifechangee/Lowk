import { about } from './about';
import { area } from './area';
import { common } from './common';
import { docs } from './docs';
import { errors } from './errors';
import { m } from './m';
import { main } from './main';
import { onboarding } from './onboarding';
import { overlay } from './overlay';
import { search } from './search';
import { settings } from './settings';
import { summary } from './summary';
import { tour } from './tour';

// Must mirror ru/index.ts key-for-key (enforced by i18n.test.ts).
export const dict = { ...common, ...errors, ...main, ...m, ...overlay, ...area, ...settings, ...onboarding, ...docs, ...search, ...summary, ...tour, ...about } as const;

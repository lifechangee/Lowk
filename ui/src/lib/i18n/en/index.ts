import { common } from './common';
import { errors } from './errors';
import { main } from './main';
import { overlay } from './overlay';

// One file per feature namespace so two features rarely touch the same file.
export const dict = { ...common, ...errors, ...main, ...overlay } as const;

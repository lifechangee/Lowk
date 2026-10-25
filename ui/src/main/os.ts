// OS-specific wording: one place for the strings that name a platform feature (key store, file manager, shortcuts).
import { api } from './api';
import { tm } from './i18n';
import type { MKey } from './i18n';

type Variant = 'macos' | 'windows' | 'linux';
const variant = (): Variant => api.platform();

export const keyStoredText = (): string => tm(`m.keyStored.${variant()}` as MKey);
export const keyStorageHint = (): string => tm(`m.keyStorage.${variant()}` as MKey);
export const revealText = (): string => tm(`m.reveal.${variant()}` as MKey);
export const defaultAskNote = (): string => tm(variant() === 'macos' ? 'm.tpl.defaultNote.macos' : 'm.tpl.defaultNote.win');

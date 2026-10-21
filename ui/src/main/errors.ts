// AppError -> text. Every backend failure reaches the user through here (no silent fallbacks).
import type { AppError } from '../lib/ipc';
import { dicts, type Key } from '../lib/i18n';
import { CommandError } from './api';
import { tm } from './i18n';

/** The translated message of an `AppError`; `{param}` placeholders come from `error.params`. */
export function appErrorText(err: AppError): string {
  const key = `error.${err.code}` as Key;
  return key in dicts.en ? tm(key, err.params ?? {}) : tm('error.unknown');
}

/** The translated message of anything thrown by an `api` call. */
export function errText(e: unknown): string {
  return e instanceof CommandError ? appErrorText(e.app) : tm('error.unknown');
}

export const errCode = (e: unknown): AppError['code'] | null => (e instanceof CommandError ? e.app.code : null);

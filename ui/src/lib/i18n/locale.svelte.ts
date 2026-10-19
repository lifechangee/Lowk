// Reactive locale. Components read `t(...)` / `tPlural(...)` from here so that
// changing the setting re-renders every visible string without a reload.
import { commands } from '../ipc';
import {
  formatNumber,
  pluralCategory,
  resolveLang,
  setLang,
  t as translate,
  tPlural as translatePlural,
  type Key,
  type Lang,
  type LangSetting,
  type Params,
  type PluralBase,
} from './index';

class Locale {
  /** `auto` | `ru` | `en`, from settings.ui.language. */
  setting = $state<LangSetting>('auto');
  /** Resolved system locale (from Rust `locale_get`); `en` until loaded. */
  systemLocale = $state('en');
  /** The language actually used for display. */
  lang: Lang = $derived(resolveLang(this.setting, this.systemLocale));

  /** Sets both values and syncs `<html lang>`. Call once at window start. */
  init(systemLocale: string, setting: LangSetting = 'auto'): void {
    this.systemLocale = systemLocale;
    this.setting = setting;
    this.#syncDocument();
  }

  /** Live switch from the settings screen. */
  setSetting(setting: LangSetting): void {
    this.setting = setting;
    this.#syncDocument();
  }

  setSystemLocale(systemLocale: string): void {
    this.systemLocale = systemLocale;
    this.#syncDocument();
  }

  t = (key: Key, params?: Params): string => translate(key, params, this.lang);

  tPlural = (base: PluralBase, n: number, params?: Params): string =>
    translatePlural(base, n, params, this.lang);

  plural = (n: number) => pluralCategory(n, this.lang);

  number = (n: number, options?: Intl.NumberFormatOptions): string =>
    formatNumber(n, this.lang, options);

  #syncDocument(): void {
    setLang(this.lang);
    if (typeof document !== 'undefined') document.documentElement.lang = this.lang;
  }
}

export const locale = new Locale();

/** Reactive `t`: `t('main.title')` inside a template re-renders on language change. */
export const t = locale.t;
/** Reactive plural helper: `tPlural('main.calls', n)`. */
export const tPlural = locale.tPlural;

/**
 * Reads the OS locale from Rust (`locale_get`). Falls back to `en` when the
 * command is unavailable (plain browser, tests). `navigator.language` is not
 * used: the WebView can differ from the OS.
 */
export async function loadSystemLocale(): Promise<string> {
  try {
    return await commands.localeGet();
  } catch {
    return 'en';
  }
}

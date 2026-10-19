// Keys are `error.<ErrorCode>` (snake_case codes from lowk-types).
export const errors = {
  'error.unknown': 'Неизвестная ошибка',
  'error.not_implemented': 'Пока не реализовано',
  'error.settings_invalid': 'Некорректные настройки',
  'error.vault_not_open': 'Хранилище не открыто',
} as const;

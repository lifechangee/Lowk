// Overlay-local strings. These mirror i18n-keys.todo.json (WP10 merges them into lib/i18n);
// keys that already exist in lib/i18n are resolved there.
import { t as libT, type Key, type Lang } from '../lib/i18n';

const ru = {
  'overlay.chain.question': 'Вопрос',
  'overlay.chain.hint': 'Подсказка ИИ',
  'overlay.chain.my': 'Мой ответ',
  'overlay.my.title': 'Мой ответ',
  'overlay.my.empty': 'Скажите ответ вслух…',
  'overlay.shots': 'Скриншоты: {n}',
  'overlay.input.placeholder': 'Спросить Lowk о звонке…',
  'overlay.hint.interactive': 'интерактивный режим',
  'overlay.hint.ask': 'ответ',
  'overlay.hint.shot': 'скриншот',
  'overlay.hint.exit': 'вернуть сквозной режим',
  'overlay.hint.send': 'отправить',
  'overlay.hint.move': 'Удерживайте {key} / боковую кнопку мыши, чтобы перетащить',
  'overlay.move.badge': 'Перемещение · отпустите {key}, и снова сквозной режим',
  'overlay.toast.shot': 'Скриншот добавлен',
  'overlay.waiting': 'Слушаю…',
  'overlay.answer.empty': 'Нажмите {key}, чтобы спросить ИИ',
  'overlay.answer.error': 'Не удалось получить ответ',
  'overlay.pill.start': 'Начать запись',
  'overlay.template': 'Шаблон',
  'overlay.new': 'новое',
  'overlay.state.idle': 'Не записывается',
  'overlay.state.paused': 'Пауза',
} as const;

const en: Record<keyof typeof ru, string> = {
  'overlay.chain.question': 'Question',
  'overlay.chain.hint': 'AI hint',
  'overlay.chain.my': 'My answer',
  'overlay.my.title': 'My answer',
  'overlay.my.empty': 'Say your answer aloud…',
  'overlay.shots': 'Screenshots: {n}',
  'overlay.input.placeholder': 'Ask Lowk about the call…',
  'overlay.hint.interactive': 'interactive mode',
  'overlay.hint.ask': 'answer',
  'overlay.hint.shot': 'screenshot',
  'overlay.hint.exit': 'back to click-through',
  'overlay.hint.send': 'send',
  'overlay.hint.move': 'Hold {key} / a mouse side button to drag',
  'overlay.move.badge': 'Moving · release {key} to return to click-through',
  'overlay.toast.shot': 'Screenshot added',
  'overlay.waiting': 'Listening…',
  'overlay.answer.empty': 'Press {key} to ask the AI',
  'overlay.answer.error': 'Could not get an answer',
  'overlay.pill.start': 'Start recording',
  'overlay.template': 'Template',
  'overlay.new': 'new',
  'overlay.state.idle': 'Not recording',
  'overlay.state.paused': 'Paused',
};

export type OKey = keyof typeof ru;
export const localDict: Record<Lang, Record<OKey, string>> = { ru, en };

export function ot(key: OKey | Key, lang: Lang, params: Record<string, string | number> = {}): string {
  const d = localDict[lang] as Record<string, string>;
  const s = d[key];
  if (s === undefined) return libT(key as Key, params, lang);
  return s.replace(/\{(\w+)\}/g, (m, k: string) => (k in params ? String(params[k]) : m));
}

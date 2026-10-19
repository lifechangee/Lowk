// Fixture data for the plain-browser mock. Dates are relative to "now" so
// «Сегодня» / "Today" logic is always exercised. Titles are user data (not translated).
import type { CallMeta, FolderMeta } from '../../lib/ipc';
import type { CallDetail, DocItem, FolderDetail, QaItem, ShotItem, Utterance } from './types';

const ulid = (n: number): string => `01JC8Z3K6Q9X2M4N7R5T0V${String(n).padStart(4, '0')}`;
const MIN = 60_000;
const DAY = 24 * 60 * MIN;

export const F_ACME = ulid(1);
export const F_NOVA = ulid(2);
export const F_HIRE = ulid(3);
export const F_PERS = ulid(4);

const user = (id: string) => ({ kind: 'user' as const, id });

export function makeFolders(): FolderMeta[] {
  const f = (id: string, title: string, tags: string[], allow = true): FolderMeta => ({ id: user(id), title, tags, allow_cloud: allow, default_template: 'brief', path: title, call_count: 0 });
  return [
    f(F_ACME, 'Acme Corp', ['продажи', 'финтех', 'ключевой клиент']),
    f(F_NOVA, 'Nova Bank', ['интеграция']),
    f(F_HIRE, 'Собеседования', ['hiring'], false),
    f(F_PERS, 'Личное', []),
  ];
}

export function makeCalls(now: number): CallMeta[] {
  const c = (n: number, title: string, folder: CallMeta['folder'], startedAgoMs: number, durMin: number, o: Partial<CallMeta> = {}): CallMeta => ({
    id: ulid(100 + n),
    title,
    folder,
    started: new Date(now - startedAgoMs).toISOString(),
    ended: new Date(now - startedAgoMs + durMin * MIN).toISOString(),
    duration_s: durMin * 60,
    languages: ['ru', 'en'],
    tags: [],
    sources: { me: 'MacBook Air Microphone', system: 'Zoom' },
    stt_engine: 'whisper-large-v3-turbo',
    path: `${title}.md`,
    deleted: false,
    ...o,
  });
  return [
    c(1, 'Квартальное планирование Q4', user(F_ACME), 3 * 60 * MIN, 72, { tags: ['продажи', 'финансы'] }),
    c(2, 'Демо для CFO', user(F_ACME), 3 * DAY, 48, { tags: ['продажи'] }),
    c(3, 'Weekly sync', user(F_ACME), 8 * DAY, 31, { languages: ['en'], sources: { me: 'MacBook Air Microphone', system: 'Google Meet (Chrome)' } }),
    c(4, 'Обсуждение договора', user(F_ACME), 11 * DAY, 55, { tags: ['договор'] }),
    c(5, 'Kickoff интеграции', user(F_NOVA), 6 * DAY, 62, { tags: ['интеграция'], sources: { me: 'MacBook Air Microphone', system: 'Microsoft Teams' } }),
    c(6, 'Ретро спринта 4', user(F_NOVA), 10 * DAY, 40),
    c(7, 'Backend, Иван К.', user(F_HIRE), 4 * DAY, 58, { tags: ['hiring'] }),
    c(8, 'Product, Мария Л.', user(F_HIRE), 5 * DAY, 44, { tags: ['hiring'] }),
    c(9, 'Консультация, налоги', user(F_PERS), 18 * DAY, 25, { languages: ['ru'] }),
    c(10, 'Быстрый созвон с подрядчиком', { kind: 'unsorted' }, 26 * 60 * MIN, 17, { languages: ['ru'] }),
    c(11, 'Very long call title that would never fit in one line of the sidebar, to test the strict text bounds', { kind: 'unsorted' }, 2 * DAY, 9, { languages: ['en'] }),
    c(12, 'Старая демо-запись', user(F_NOVA), 30 * DAY, 20, { deleted: true }),
    c(13, 'Тестовый звонок', { kind: 'unsorted' }, 33 * DAY, 3, { deleted: true }),
  ];
}

const U = (at_s: number, speaker: 'me' | 'system', text: string): Utterance => ({ at_s, speaker, text });

const q4Transcript: Utterance[] = [
  U(12, 'system', 'Всем привет, давайте начнём. Сегодня хотим обсудить цели на Q4 и бюджет на маркетинг.'),
  U(21, 'me', 'Привет! Да, у меня подготовлены цифры по пайплайну и по conversion rate.'),
  U(290, 'system', 'Нам важно удержать темпы роста выше 30%, при этом не раздувать CAC.'),
  U(302, 'me', 'Понял. Тогда предлагаю сделать ставку на partner-канал, он дешевле на 20–25%.'),
  U(1421, 'system', 'А какой у вас сейчас EBITDA margin по сегменту B2B?'),
  U(1450, 'me', 'Сейчас покажу слайд, мы как раз это считали.'),
  U(1473, 'system', 'Отлично, это совпадает с нашей моделью. Давайте перейдём к бюджету на партнёров.'),
  U(2480, 'system', 'И как объясните рост CAC в сентябре?'),
  U(2543, 'me', 'Там была разовая история с конференцией.'),
  U(3500, 'system', 'Какой срок интеграции вы можете гарантировать?'),
  U(3511, 'me', 'Ориентируемся на шесть-восемь недель, если успеем со sandbox.'),
];

const q4Qa: QaItem[] = [
  { id: ulid(201), at_s: 1424, question: 'А какой у вас сейчас EBITDA margin по сегменту B2B?', question_stream: 'system', template: 'Кратко ответить', answer: '**EBITDA margin по B2B — 23,4%** за Q3 (+2,1 п.п. к Q2). Драйвер: CAC −11% после запуска partner-канала.', my_answer: 'Если коротко, около двадцати трёх процентов, плюс два пункта к прошлому кварталу.', my_answer_at_s: 1432 },
  { id: ulid(202), at_s: 2481, question: 'И как объясните рост CAC в сентябре?', question_stream: 'system', template: 'Дополни мой ответ', answer: '**Добавь:** сентябрьский всплеск — разовая закупка на конференцию; без неё CAC ниже августовского на 6%.', my_answer: 'Там была разовая история с конференцией.', my_answer_at_s: 2463 },
  { id: ulid(203), at_s: 3500, question: 'Какой срок интеграции вы можете гарантировать?', question_stream: 'system', template: 'Что сказать?', answer: 'Не обещай жёстких сроков: назови 6–8 недель при условии, что sandbox будет доступен на следующей неделе.', my_answer: 'Ориентируемся на шесть-восемь недель, если успеем со sandbox.', my_answer_at_s: 3511 },
];

const q4Shots: ShotItem[] = [
  { id: ulid(301), at_s: 1450, rel_path: 'shot-0001.png', title: 'Выручка +38% год к году', caption: 'весь экран · слайд 7 из 18', source: 'manual' },
  { id: ulid(302), at_s: 3151, rel_path: 'shot-0002.png', title: 'План на Q4: 3 приоритета', caption: 'весь экран · слайд 12 из 18', source: 'auto' },
];

const Q4_SUMMARY = '- Цель Q4: рост выручки выше 30% без роста CAC; ставка на partner-канал (дешевле на 20–25%).\n- EBITDA margin B2B за Q3: 23,4% (около 25% без one-off миграции CRM).\n- Следующий шаг: обсудить бюджет на партнёров.';

/** Calls that already have a summary in the fixtures (suffix of the id). */
const SUMMARIZED = ['0102', '0105', '0107'];
export const summarizedIds = (calls: CallMeta[]): string[] => calls.filter((c) => SUMMARIZED.some((x) => c.id.endsWith(x))).map((c) => c.id);

export function makeDetail(meta: CallMeta): CallDetail {
  const base = { meta, keep_audio: false, model: 'DeepSeek V4.1 Flash' };
  const n = meta.id.endsWith('0101');
  if (n) return { ...base, summary: null, transcript: q4Transcript, qa: q4Qa, shots: q4Shots };
  const summary = SUMMARIZED.some((x) => meta.id.endsWith(x)) ? Q4_SUMMARY : null;
  // Other calls: a short synthetic transcript; "Weekly sync" has no AI usage at all (empty states).
  const empty = meta.id.endsWith('0103');
  const tr: Utterance[] = [
    U(8, 'system', 'Hi everyone, quick agenda for today: blockers, the contract, and next steps.'),
    U(19, 'me', 'Sounds good. I have two blockers to raise.'),
    U(95, 'system', 'Понятно. Давайте зафиксируем владельцев по каждому пункту.'),
    U(140, 'me', 'Согласен, я возьму интеграцию и вернусь с оценкой.'),
  ];
  return {
    ...base,
    summary,
    transcript: tr,
    qa: empty ? [] : [{ ...q4Qa[0]!, id: meta.id + 'q', at_s: 96, question: 'Кто владелец по каждому пункту?', answer: 'Назначь владельцев по пунктам и зафиксируй сроки в письме после созвона.', my_answer: null, my_answer_at_s: null }],
    shots: [],
  };
}

export const DOCS: Record<string, DocItem[]> = {
  [F_ACME]: [
    { id: ulid(401), name: 'Договор-2026.pdf', ext: 'pdf', pages: 24, state: 'converted' },
    { id: ulid(402), name: 'Бриф на кампанию.docx', ext: 'docx', pages: 6, state: 'ready' },
    { id: ulid(403), name: 'Прайс.md', ext: 'md', pages: null, state: 'indexing' },
  ],
  [F_NOVA]: [{ id: ulid(404), name: 'API-спецификация.pdf', ext: 'pdf', pages: 58, state: 'ready' }],
};

export const INSTRUCTIONS: Record<string, string> = {
  [F_ACME]: 'Клиент — финтех, общение на «вы». Отвечай кратко, цифры приводи с единицами измерения. Термины оставляй на английском (EBITDA, CAC). Не обещай сроков без подтверждения.',
};

export function makeFolderDetail(meta: FolderMeta): FolderDetail {
  const id = meta.id.kind === 'user' ? meta.id.id : '';
  return { meta, instructions: INSTRUCTIONS[id] ?? '', docs: DOCS[id] ?? [] };
}

export const LIVE_LINES: { speaker: 'me' | 'system'; text: string }[] = [
  { speaker: 'system', text: 'А какой у вас сейчас EBITDA margin по сегменту B2B?' },
  { speaker: 'me', text: 'Секунду, сверюсь с цифрами за квартал.' },
  { speaker: 'system', text: 'И как это соотносится с планом на Q4?' },
  { speaker: 'me', text: 'Сейчас покажу, мы как раз это считали.' },
];

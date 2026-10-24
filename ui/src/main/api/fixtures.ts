// Fixture data for the plain-browser mock. Dates are relative to "now" so
// «Сегодня» / "Today" logic is always exercised. Titles are user data (not translated).
import type { CallMeta, DocKind, Document, FolderMeta, IngestWarning, SourceRef } from '../../lib/ipc';
import type { CallDetail, FolderDetail, QaItem, ShotItem, Utterance } from './types';

const ulid = (n: number): string => `01JC8Z3K6Q9X2M4N7R5T0V${String(n).padStart(4, '0')}`;
const MIN = 60_000;
const DAY = 24 * 60 * MIN;

export const F_ACME = ulid(1);
export const F_NOVA = ulid(2);
export const F_HIRE = ulid(3);
export const F_PERS = ulid(4);
/** A folder with no calls, documents or instructions (empty states). */
export const F_EMPTY = ulid(5);

const user = (id: string) => ({ kind: 'user' as const, id });

export function makeFolders(): FolderMeta[] {
  const f = (id: string, title: string, tags: string[], allow = true): FolderMeta => ({ id: user(id), title, tags, allow_cloud: allow, default_template: 'brief', mic: 'inherit', retx: 'inherit', path: title, call_count: 0 });
  return [
    f(F_ACME, 'Acme Corp', ['продажи', 'финтех', 'ключевой клиент']),
    f(F_NOVA, 'Nova Bank', ['интеграция']),
    f(F_HIRE, 'Собеседования', ['hiring'], false),
    f(F_PERS, 'Личное', []),
    f(F_EMPTY, 'Новый проект', []),
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

const ref = (kind: SourceRef['kind'], id: string, title: string, o: Partial<SourceRef> = {}): SourceRef => ({ kind, id, folder: { kind: 'user', id: F_ACME }, title, at_s: null, page: null, heading_path: [], chunk_id: null, pinned: false, ...o });
const SRC_CONTRACT = ref('doc', ulid(401), 'Договор-2026.pdf', { page: 14, pinned: true, heading_path: ['4. Оплата'] });
const SRC_BRIEF = ref('doc', ulid(402), 'Бриф на кампанию.docx', { page: 3, pinned: true });
const SRC_MODEL = ref('doc', ulid(405), 'Квартальная модель.xlsx', { page: 2, heading_path: ['Маржа B2B'] });
const SRC_PREV = ref('call', ulid(102), 'Демо для CFO', { at_s: 1105 });
const SRC_INSTR = ref('instructions', F_ACME, 'Инструкции папки', { pinned: true });

const q4Qa: QaItem[] = [
  { id: ulid(201), at_s: 1424, question: 'А какой у вас сейчас EBITDA margin по сегменту B2B?', question_stream: 'system', template: 'Кратко ответить', answer: '**EBITDA margin по B2B — 23,4%** за Q3 (+2,1 п.п. к Q2). Драйвер: CAC −11% после запуска partner-канала.', my_answer: 'Если коротко, около двадцати трёх процентов, плюс два пункта к прошлому кварталу.', my_answer_at_s: 1432, sources: [SRC_INSTR, SRC_MODEL, SRC_PREV] },
  { id: ulid(202), at_s: 2481, question: 'И как объясните рост CAC в сентябре?', question_stream: 'system', template: 'Дополни мой ответ', answer: '**Добавь:** сентябрьский всплеск — разовая закупка на конференцию; без неё CAC ниже августовского на 6%.', my_answer: 'Там была разовая история с конференцией.', my_answer_at_s: 2463, sources: [SRC_INSTR, SRC_CONTRACT] },
  { id: ulid(203), at_s: 3500, question: 'Какой срок интеграции вы можете гарантировать?', question_stream: 'system', template: 'Что сказать?', answer: 'Не обещай жёстких сроков: назови 6–8 недель при условии, что sandbox будет доступен на следующей неделе.', my_answer: 'Ориентируемся на шесть-восемь недель, если успеем со sandbox.', my_answer_at_s: 3511, sources: [SRC_BRIEF] },
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
  if (n) return { ...base, summary: null, transcript: q4Transcript, qa: q4Qa, shots: q4Shots, sources: dedupeSources(q4Qa.flatMap((q) => q.sources ?? [])) };
  const summary = SUMMARIZED.some((x) => meta.id.endsWith(x)) ? Q4_SUMMARY : null;
  // Other calls: a short synthetic transcript; "Weekly sync" has no AI usage at all (empty states).
  const empty = meta.id.endsWith('0103');
  // Local-only folder («Собеседования»): the cloud model never saw its documents; the answer says how many chunks were withheld.
  const hire = meta.id.endsWith('0107');
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
    qa: empty ? [] : [{ ...q4Qa[0]!, id: meta.id + 'q', at_s: 96, question: 'Кто владелец по каждому пункту?', answer: 'Назначь владельцев по пунктам и зафиксируй сроки в письме после созвона.', my_answer: null, my_answer_at_s: null, sources: [], withheld: hire ? 3 : 0 }],
    shots: [],
    sources: [],
  };
}

function dedupeSources(list: SourceRef[]): SourceRef[] {
  const seen = new Set<string>();
  return list.filter((r) => {
    const k = `${r.kind}:${r.id}:${r.page ?? ''}:${r.at_s ?? ''}`;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  });
}

// ---- documents ----
export function kindOfFile(name: string): DocKind | null {
  const ext = (name.split('.').pop() ?? '').toLowerCase();
  if (ext === 'key') return 'keynote';
  if (ext === 'tsv') return 'csv';
  if (['png', 'jpg', 'jpeg', 'heic', 'webp', 'tiff'].includes(ext)) return 'image';
  return (['pdf', 'docx', 'pptx', 'xlsx', 'csv', 'txt', 'md'] as DocKind[]).find((k) => k === ext) ?? null;
}
/** Page / slide / sheet: the `unit` of the page markers. */
export const unitOf = (kind: DocKind): 'page' | 'slide' | 'sheet' => (kind === 'pptx' || kind === 'keynote' ? 'slide' : kind === 'xlsx' || kind === 'csv' ? 'sheet' : 'page');

const w = (code: IngestWarning['code'], page: number | null = null): IngestWarning => ({ code, page, detail: null });

export function makeDocuments(now: number): Document[] {
  const d = (n: number, folder: string, file: string, o: Partial<Document> & { daysAgo?: number } = {}): Document => {
    const kind = kindOfFile(file)!;
    const { daysAgo = n % 9, ...rest } = o;
    const stem = file.replace(/\.[^.]+$/, '');
    return {
      id: ulid(n), folder: { kind: 'user', id: folder }, title: stem, kind, file_name: file,
      md_path: `Docs/${stem}.md`, original_path: `Docs/${file}`, size_bytes: 480_000 + n * 1000, pages: null,
      status: 'ready', error: null, warnings: [], pinned: false, tokens: 800, tags: [], added: new Date(now - daysAgo * 86_400_000).toISOString(),
      source_sha256: 'a3f1c2'.repeat(10) + 'beef', converter: kind === 'pdf' ? 'pdf-text' : kind, deleted: false, ...rest,
    };
  };
  const err = (code: NonNullable<Document['error']>['code']): Document['error'] => ({ code, params: {}, detail: null });
  return [
    d(401, F_ACME, 'Договор-2026.pdf', { pages: 24, pinned: true, tokens: 2400, converter: 'pdf-ocr', warnings: [w('ocr_used')], size_bytes: 3_400_000 }),
    d(402, F_ACME, 'Бриф на кампанию.docx', { pages: 6, pinned: true, tokens: 1500, warnings: [w('tables_flattened', 3)] }),
    d(403, F_ACME, 'Прайс.md', { pinned: true, tokens: 300, size_bytes: 9_800 }),
    d(404, F_ACME, 'Product deck.pptx', { pages: 18, tokens: 2300, warnings: [w('slide_images_ignored')], size_bytes: 12_600_000 }),
    d(405, F_ACME, 'Квартальная модель.xlsx', { pages: 4, tokens: 1900, daysAgo: 1 }),
    d(406, F_ACME, 'Лог интеграции.csv', { pages: 1, tokens: 3400, warnings: [w('sheet_truncated'), w('large_file')], size_bytes: 64_000_000, daysAgo: 2 }),
    d(407, F_ACME, 'Скан акта.png', { status: 'queued', tokens: 0, daysAgo: 0, converter: '' }),
    d(408, F_ACME, 'Отчёт по рынку.pdf', { status: 'converting', pages: 300, tokens: 0, daysAgo: 0, size_bytes: 41_000_000 }),
    d(409, F_ACME, 'Защищённый отчёт.pdf', { status: 'failed', error: err('doc_encrypted'), tokens: 0, daysAgo: 0, converter: '' }),
    d(410, F_ACME, 'Старый черновик.docx', { status: 'cancelled', tokens: 0, daysAgo: 5, converter: '' }),
    d(411, F_NOVA, 'API-спецификация.pdf', { pages: 58, pinned: true, tokens: 3000 }),
    d(412, F_NOVA, 'Схема интеграции.key', { pages: 22, tokens: 1700, warnings: [w('slide_images_ignored')] }),
    d(413, F_HIRE, 'Вакансия Backend.docx', { pages: 3, pinned: true, tokens: 3200 }),
    d(414, F_HIRE, 'Scorecard.xlsx', { pages: 2, pinned: true, tokens: 2800 }),
    d(415, F_HIRE, 'Культура компании.pdf', { pages: 12, pinned: true, tokens: 1500 }),
  ];
}

const LINES = [
  'Бюджет на партнёрский канал утверждается ежеквартально; CAC считается без разовых расходов.',
  'EBITDA margin по сегменту B2B за Q3 — 23,4%. Срок интеграции: 6–8 недель при доступном sandbox.',
  'Оплата в течение 14 дней после подписания акта. SLA на ответ службы поддержки: 4 часа.',
  'Seat licenses are billed annually; the integration budget covers two engineers for eight weeks.',
];
/** The converted Markdown of a document: frontmatter + `<!-- p:N -->` markers (what `doc_get_md` returns). */
export function docMd(doc: Document): string {
  const total = Math.max(1, Math.min(doc.pages ?? 1, 40));
  const fm = ['---', 'lowk: 1', 'type: doc', `id: ${doc.id}`, `title: ${doc.title}`, `source_sha256: ${doc.source_sha256}`, `converter: ${doc.converter || 'pdf-text'}`, `unit: ${unitOf(doc.kind)}`, `pages: ${doc.pages ?? 1}`, '---', ''];
  const body: string[] = [`# ${doc.title}`, ''];
  for (let p = 1; p <= total; p++) body.push(`<!-- p:${p} -->`, `## ${p}.`, '', LINES[(p + doc.tokens) % LINES.length]!, '', LINES[(p + 1) % LINES.length]!, '');
  return [...fm, ...body].join('\n');
}

/** Tag vocabulary the AI prefers (what the vault already uses) plus one invented tag. */
export const TAG_SUGGESTIONS: { tag: string; existing: boolean; confidence: number; reason: string }[] = [
  { tag: 'продажи', existing: true, confidence: 0.92, reason: 'партнёрский канал, CAC' },
  { tag: 'финансы', existing: true, confidence: 0.81, reason: 'EBITDA, бюджет' },
  { tag: 'q4-план', existing: false, confidence: 0.64, reason: 'цели на квартал' },
];

export const SUMMARY_MD = [
  '### Ключевые пункты',
  '- Цель Q4: рост выручки выше 30% без роста CAC; ставка на partner-канал (дешевле на 20–25%).',
  '- EBITDA margin B2B за Q3: 23,4% (около 25% без one-off миграции CRM).',
  '- Рост CAC в сентябре объясняется разовой закупкой на конференцию.',
  '### Решения',
  '- Делаем ставку на partner-канал в Q4.',
  '- Срок интеграции называем в диапазоне 6–8 недель при условии доступного sandbox.',
  '### Задачи',
  '- **Я**: подготовить бюджет на партнёров до пятницы.',
  '- **Клиент**: дать доступ к sandbox на следующей неделе.',
  '### Вопросы ко мне',
  '- Какой EBITDA margin по B2B? — «около двадцати трёх процентов, плюс два пункта к Q2».',
  '- Как объясните рост CAC? — «разовая история с конференцией».',
].join('\n');
export const SUMMARY_MD_EN = [
  '### Key points',
  '- Q4 goal: revenue growth above 30% without raising CAC; bet on the partner channel (20-25% cheaper).',
  '- B2B EBITDA margin for Q3: 23.4% (about 25% without the one-off CRM migration).',
  '- The September CAC rise is explained by a one-off conference purchase.',
  '### Decisions',
  '- Go with the partner channel in Q4.',
  '- Quote the integration timeline as 6-8 weeks, provided the sandbox is available.',
  '### Tasks',
  '- **Me**: prepare the partner budget by Friday.',
  '- **Client**: give access to the sandbox next week.',
  '### Questions to me',
  '- What is the B2B EBITDA margin? — "about twenty-three percent, two points up on Q2".',
  '- How do you explain the CAC rise? — "a one-off conference purchase".',
].join('\n');

export const INSTRUCTIONS: Record<string, string> = {
  [F_ACME]: 'Клиент — финтех, общение на «вы». Отвечай кратко, цифры приводи с единицами измерения. Термины оставляй на английском (EBITDA, CAC). Не обещай сроков без подтверждения.',
};

export function makeFolderDetail(meta: FolderMeta): FolderDetail {
  const id = meta.id.kind === 'user' ? meta.id.id : '';
  return { meta, instructions: INSTRUCTIONS[id] ?? '' };
}

export const LIVE_LINES: { speaker: 'me' | 'system'; text: string }[] = [
  { speaker: 'system', text: 'А какой у вас сейчас EBITDA margin по сегменту B2B?' },
  { speaker: 'me', text: 'Секунду, сверюсь с цифрами за квартал.' },
  { speaker: 'system', text: 'И как это соотносится с планом на Q4?' },
  { speaker: 'me', text: 'Сейчас покажу, мы как раз это считали.' },
];

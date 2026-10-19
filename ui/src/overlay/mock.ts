// Mock event source: replays a demo call with RU/EN code-switching through the same handlers
// the real IPC events use. Runs in `vite dev`; never bundled into the app entry (dynamic import).
import type { OverlayStore } from './store.svelte';
import type { OverlaySource } from './source';
import type { Stream } from '../lib/ipc/bindings';

interface Beat {
  stream: Stream;
  text: string;
}

interface Scenario {
  lang: 'ru' | 'en';
  question: Beat;
  answer: string;
  mine: string;
}

const SCENARIOS: Scenario[] = [
  {
    lang: 'ru',
    question: { stream: 'system', text: 'А что у нас по EBITDA margin и по customer acquisition cost за третий квартал?' },
    answer:
      '**EBITDA margin** вырос с 21,3% до **23,4%** (+2,1 п.п.). **CAC** снизился на 11%, с 86 до 77 тыс. ₽, главным образом за счёт partner-канала. Стоит упомянуть, что в марже есть one-off эффект.',
    mine: 'EBITDA margin вырос до двадцати трёх и четырёх, а CAC упал на одиннадцать процентов благодаря партнёрам.',
  },
  {
    lang: 'en',
    question: { stream: 'system', text: "Got it. And how risky are the one-off items for the Q4 forecast?" },
    answer:
      '**One-off items** added about **1.2 pp** to the Q3 margin, so the Q4 forecast should exclude them. Underlying margin is roughly 22.2%. Suggest showing both numbers.',
    mine: 'The one-offs added around one point two, so I would exclude them from the forecast.',
  },
];

const OPENING: Beat[] = [
  { stream: 'system', text: 'Давайте посмотрим на итоги третьего квартала. Выручка выросла на 18% к прошлому кварталу.' },
  { stream: 'me', text: 'Да, я подготовил отчёт, сейчас покажу слайды.' },
];

const SEED_ELAPSED_MS = (47 * 60 + 12) * 1000;

export class MockSource implements OverlaySource {
  #store!: OverlayStore;
  #gen = 0;
  #utt = 1;
  #scenario = 0;
  #tick: ReturnType<typeof setInterval> | undefined;
  #autoplay: boolean;

  constructor(opts: { autoplay?: boolean } = {}) {
    this.#autoplay = opts.autoplay ?? true;
  }

  start(store: OverlayStore): void {
    this.#store = store;
    store.phase = 'recording';
    store.elapsedMs = SEED_ELAPSED_MS;
    this.#tick = setInterval(() => {
      if (this.#store.phase === 'recording') this.#store.elapsedMs += 1000;
    }, 1000);
    if (this.#autoplay) void this.#run(++this.#gen);
  }

  stop(): void {
    this.#gen++;
    clearInterval(this.#tick);
  }

  async #sleep(ms: number, g: number): Promise<void> {
    await new Promise((r) => setTimeout(r, ms));
    if (g !== this.#gen) throw new Error('cancelled');
  }

  async #say(b: Beat, g: number, wordMs = 140): Promise<void> {
    const utt = this.#utt++;
    const words = b.text.split(' ');
    let acc = '';
    for (let i = 0; i < words.length; i++) {
      acc += (i ? ' ' : '') + words[i];
      this.#store.applyStt(ev(b.stream, utt, i === words.length - 1 ? 'final' : 'partial', acc));
      await this.#sleep(wordMs, g);
    }
  }

  async #answer(text: string, g: number): Promise<void> {
    const s = this.#store;
    s.applyLlm({ type: 'start', req_id: 'r' + g, qa_id: 'q' + g });
    const chunks = text.match(/.{1,6}/gs) ?? [];
    for (const c of chunks) {
      s.applyLlm({ type: 'delta', req_id: 'r' + g, text: c });
      await this.#sleep(38, g);
    }
    s.applyLlm({ type: 'done', req_id: 'r' + g, truncated: false, usage: null });
  }

  async #run(g: number): Promise<void> {
    const s = this.#store;
    try {
      for (;;) {
        s.reset();
        s.selectTemplate('brief');
        for (const b of OPENING) {
          await this.#say(b, g);
          await this.#sleep(900, g);
        }
        for (let i = 0; i < SCENARIOS.length; i++) {
          const sc = SCENARIOS[(this.#scenario + i) % SCENARIOS.length]!;
          if (i === 1) s.selectTemplate('complement');
          await this.#say(sc.question, g);
          await this.#sleep(500, g);
          this.screenshot();
          await this.#sleep(500, g);
          if (i === 0) this.screenshot();
          await this.#sleep(600, g);
          await this.#answer(sc.answer, g);
          s.setQueue(0);
          await this.#sleep(1200, g);
          await this.#say({ stream: 'me', text: sc.mine }, g, 160);
          await this.#sleep(2500, g);
        }
        await this.#sleep(2500, g);
      }
    } catch {
      /* cancelled */
    }
  }

  ask(): void {
    if (this.#store.streaming) return;
    const sc = SCENARIOS[this.#scenario++ % SCENARIOS.length]!;
    void this.#answer(sc.answer, ++this.#gen).then(() => this.#store.setQueue(0));
  }

  screenshot(): void {
    const s = this.#store;
    s.setQueue(s.shots + 1, [...s.thumbs, 'x']);
    s.flash(s.lang === 'ru' ? 'Скриншот добавлен' : 'Screenshot added');
  }

  submitText(text: string): void {
    const t = text.trim();
    if (!t) return;
    const cyr = /[Ѐ-ӿ]/.test(t);
    const reply = cyr
      ? `По вашему вопросу «${t}»: коротко, **главное**: цифры по кварталу совпадают с отчётом.`
      : `On "${t}": in short, the **key point** is that the quarterly numbers match the report.`;
    void this.#answer(reply, ++this.#gen);
  }

  setTemplate(id: string): void {
    this.#store.selectTemplate(id);
  }

  toggleRecord(): void {
    const s = this.#store;
    s.phase = s.phase === 'recording' ? 'idle' : 'recording';
    if (s.phase === 'recording') {
      s.elapsedMs = 0;
      s.reset();
    }
  }
}

function ev(stream: Stream, utt: number, kind: 'partial' | 'final', text: string) {
  return { stream, utt, rev: 0, kind, text, lang: null, t_start_ns: 0, t_audio_end_ns: 0, t_emit_ns: 0 };
}

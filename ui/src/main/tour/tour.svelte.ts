// First-run tour state. The tour is offered right after onboarding («Готово»), can be skipped at any step
// (button + Esc) and replayed from Settings and the «?» in the sidebar. `onboarding.tour_done` remembers it.
import { prefs } from '../state.svelte';

export const TOUR_STEPS = 6;

/** What each step highlights on the real UI (`data-tour` marker) or shows in the overlay thumbnail. */
export type TourStepDef = {
  /** `data-tour="…"` element to spotlight; `null` = the overlay thumbnail card only. */
  target: 'newcall' | 'folders' | null;
  /** The overlay thumbnail section shown for steps 2-5. */
  thumb: 'subtitles' | 'answer' | 'header' | 'shot' | null;
  /** The hands-on demo of the step: a key to press in the tour (never touches the network). */
  tryIt: 'ask' | 'hold' | 'shot' | null;
};
export const TOUR: TourStepDef[] = [
  { target: 'newcall', thumb: null, tryIt: null },
  { target: null, thumb: 'subtitles', tryIt: null },
  { target: null, thumb: 'answer', tryIt: 'ask' },
  { target: null, thumb: 'header', tryIt: 'hold' },
  { target: null, thumb: 'shot', tryIt: 'shot' },
  { target: 'folders', thumb: null, tryIt: null },
];

class Tour {
  /** The invitation after onboarding. */
  offer = $state(false);
  open = $state(false);
  step = $state(0);

  start(): void {
    this.offer = false;
    this.step = 0;
    this.open = true;
  }
  next(): void {
    if (this.step >= TOUR_STEPS - 1) void this.finish();
    else this.step += 1;
  }
  back(): void { this.step = Math.max(0, this.step - 1); }
  /** Done or skipped: both mean «do not show it again by itself». */
  async finish(): Promise<void> {
    this.open = false;
    this.offer = false;
    await prefs.patch({ onboarding: { tour_done: true } });
  }
}
export const tour = new Tour();

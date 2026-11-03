import { splitBold } from './store.svelte';

/** Convert the overlay's current bold-only answer markup to text for a plain-text clipboard. */
export function plainAnswer(text: string): string {
  return splitBold(text).map((part) => part.text).join('');
}

export async function copyPlainAnswer(text: string): Promise<void> {
  await navigator.clipboard.writeText(plainAnswer(text));
}

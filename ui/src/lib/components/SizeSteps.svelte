<script lang="ts" module>
  export type SizeStep = { id: 's' | 'm' | 'l' | 'xl'; label: string; px: number };

  /** G1: text size presets. `px` is what lands in settings (`ui.font_px` / `overlay.font_px`). */
  export const MAIN_STEPS: SizeStep[] = [
    { id: 's', label: 'S', px: 13 },
    { id: 'm', label: 'M', px: 14 },
    { id: 'l', label: 'L', px: 16 },
    { id: 'xl', label: 'XL', px: 18 },
  ];
  export const OVERLAY_STEPS: SizeStep[] = [
    { id: 's', label: 'S', px: 12 },
    { id: 'm', label: 'M', px: 14 },
    { id: 'l', label: 'L', px: 16 },
    { id: 'xl', label: 'XL', px: 18 },
  ];

  /** The preset closest to a stored px value (older settings may hold any number). */
  export function nearestStep(steps: SizeStep[], px: number): SizeStep {
    return steps.reduce((a, b) => (Math.abs(b.px - px) < Math.abs(a.px - px) ? b : a));
  }
</script>

<script lang="ts">
  // Segmented S / M / L / XL control, applied on click (no dragging, so nothing rescales under the pointer).
  let { value, steps, label, onpick }: { value: number; steps: SizeStep[]; label: string; onpick: (px: number) => void } = $props();
  const current = $derived(nearestStep(steps, value));
</script>

<div class="seg" role="radiogroup" aria-label={label}>
  {#each steps as s (s.id)}
    <button type="button" role="radio" aria-checked={current.id === s.id} class:on={current.id === s.id} data-size={s.id} onclick={() => onpick(s.px)}>{s.label}</button>
  {/each}
</div>

<style>
  .seg { display: inline-flex; gap: var(--s1); padding: var(--s1); border-radius: var(--r); border: 1px solid var(--line); background: var(--bg); }
  button { all: unset; box-sizing: border-box; min-width: 40px; height: 28px; padding: 0 var(--s3); border-radius: var(--s1); display: grid; place-items: center; font: 600 12px/16px var(--font); color: var(--tx2); cursor: pointer; }
  button:hover { background: var(--hv); color: var(--tx); }
  button.on { background: var(--sel); color: var(--tx); box-shadow: inset 0 0 0 1px var(--line-strong); }
  button:focus-visible { outline: 2px solid var(--ac); outline-offset: 1px; }
</style>

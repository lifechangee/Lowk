<script lang="ts">
  let { value, min, max, step = 1, label, unit = '%', onchange }: { value: number; min: number; max: number; step?: number; label: string; unit?: string; onchange: (v: number) => void } = $props();
</script>

<span class="sl">
  <input type="range" {min} {max} {step} {value} aria-label={label} oninput={(e) => onchange(+e.currentTarget.value)} />
  <b>{value}{unit}</b>
</span>

<style>
  .sl { display: flex; gap: var(--s3); align-items: center; }
  input { width: 144px; accent-color: var(--ac); margin: 0; }
  b { min-width: 40px; text-align: right; font-weight: 600; font-variant-numeric: tabular-nums; }
</style>

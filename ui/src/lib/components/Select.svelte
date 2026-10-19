<script lang="ts" generics="V extends string">
  // Styled native <select> (main window only; the overlay never uses native popups).
  type Opt = { value: V; label: string };
  let { value, options, label, size = 'md', onchange }: { value: V; options: Opt[]; label: string; size?: 'sm' | 'md' | 'lg'; onchange: (v: V) => void } = $props();
</script>

<select class="sel {size}" aria-label={label} {value} onchange={(e) => onchange(e.currentTarget.value as V)}>
  {#each options as o (o.value)}<option value={o.value} selected={o.value === value}>{o.label}</option>{/each}
</select>

<style>
  .sel {
    appearance: none; -webkit-appearance: none; height: 32px; width: 100%; max-width: 100%; min-width: 0; border-radius: var(--r); border: 1px solid var(--line); color: var(--tx);
    background: var(--bg) url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='10' viewBox='0 0 10 10' fill='none' stroke='%238e8e96' stroke-width='1.6' stroke-linecap='round'%3E%3Cpath d='M2 3.5 5 6.5 8 3.5'/%3E%3C/svg%3E") no-repeat right 12px center;
    padding: 0 32px 0 var(--s3); font: 13px/20px var(--font); cursor: pointer; outline: none; text-overflow: ellipsis;
  }
  .lg { height: 44px; border-radius: var(--r-lg); padding: 0 40px 0 var(--s4); font-size: 14px; background-position: right 16px center; }
  .sel:hover, .sel:focus { border-color: var(--ac); }
</style>

<script lang="ts">
  // Call languages: compact toggle chips. Shows the selected languages and the common ones; «Ещё…» opens the
  // rest of what the engine supports. At least one language stays selected.
  import { tm, type MKey } from './i18n';
  import { ALL_LANGS, COMMON_LANGS } from './langs';

  let { value, supported, label, onchange }: { value: string[]; supported: string[]; label?: string; onchange: (v: string[]) => void } = $props();

  let more = $state(false);
  let hint = $state(false);
  const order = (c: string) => ALL_LANGS.indexOf(c as (typeof ALL_LANGS)[number]);
  const known = $derived(supported.filter((c) => order(c) >= 0));
  const shown = $derived(
    more ? known : known.filter((c) => value.includes(c) || COMMON_LANGS.includes(c)),
  );
  const hasMore = $derived(known.length > shown.length || more);
  const name = (c: string) => tm(`m.lang.${c}` as MKey);

  function toggle(c: string) {
    if (value.includes(c)) {
      if (value.length === 1) { hint = true; return; }
      onchange(value.filter((x) => x !== c));
    } else {
      onchange([...value, c].sort((a, b) => order(a) - order(b)));
    }
    hint = false;
  }
</script>

<div class="lp" role="group" aria-label={label ?? tm('m.callLangs')} data-testid="lang-picker">
  {#each shown as c (c)}
    <button type="button" class="chip" class:on={value.includes(c)} aria-pressed={value.includes(c)} data-lang={c} onclick={() => toggle(c)}>{name(c)}</button>
  {/each}
  {#if hasMore}<button type="button" class="chip more" onclick={() => (more = !more)}>{more ? tm('m.callLangsFewer') : tm('m.callLangsMore')}</button>{/if}
</div>
{#if hint}<p class="hint" role="status">{tm('m.callLangsKeepOne')}</p>{/if}

<style>
  .lp { display: flex; flex-wrap: wrap; gap: var(--s1); min-width: 0; }
  .chip { all: unset; box-sizing: border-box; height: 28px; padding: 0 var(--s3); border-radius: 99px; border: 1px solid var(--line); color: var(--tx2); background: var(--bg); font: var(--fs-sm) var(--font); line-height: 26px; cursor: pointer; white-space: nowrap; }
  .chip:hover { border-color: var(--tx3); color: var(--tx); }
  .chip.on { background: var(--acbg); border-color: var(--ac); color: var(--ac); }
  .chip.more { border-style: dashed; color: var(--ac); }
  .chip:focus-visible { outline: 2px solid var(--ac); outline-offset: 1px; }
  .hint { margin: var(--s1) 0 0; font-size: 12px; color: var(--warn); }
</style>

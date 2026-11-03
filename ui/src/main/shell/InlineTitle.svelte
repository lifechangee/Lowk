<script lang="ts">
  // The page title that can be renamed in place: click it or press F2, Enter saves, Esc cancels, leaving the
  // field saves. An empty name is refused with a message and the field stays open.
  import { tick } from 'svelte';
  import { tm } from '../i18n';

  type Props = {
    value: string;
    /** Resolves true when the new title was stored (or nothing changed). */
    onsave: (title: string) => Promise<boolean>;
    /** Parent can open the editor (menu item «Переименовать»). */
    editing?: boolean;
    readonly?: boolean;
    testid?: string;
  };
  let { value, onsave, editing = $bindable(false), readonly = false, testid = 'title' }: Props = $props();

  let draft = $state('');
  let error = $state('');
  let saving = false;
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (editing) { draft = value; error = ''; void tick().then(() => { input?.focus(); input?.select(); }); }
  });

  function begin() { if (!readonly) editing = true; }
  function cancel() { editing = false; error = ''; }
  async function commit() {
    if (saving || !editing) return;
    const t = draft.trim();
    if (!t) { error = tm('m.rename.empty'); input?.focus(); return; }
    if (t === value) return cancel();
    saving = true;
    const ok = await onsave(t).catch(() => false);
    saving = false;
    if (ok) cancel();
    else input?.focus();
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === 'F2' && !editing && !readonly && !(e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement)) { e.preventDefault(); begin(); } }} />

{#if editing}
  <div class="ed">
    <input bind:this={input} class="inp" bind:value={draft} aria-label={tm('m.rename.label')} aria-invalid={error ? 'true' : undefined} data-testid="{testid}-input" maxlength="200"
      oninput={() => (error = '')}
      onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); void commit(); } else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); cancel(); } }}
      onblur={() => void commit()} />
    {#if error}<p class="er" role="alert" data-testid="{testid}-error">{error}</p>{/if}
  </div>
{:else if readonly}
  <h1 class="lbl" data-testid={testid} title={value}>{value}</h1>
{:else}
  <h1 class="lbl" data-testid={testid}><span class="ren" role="button" tabindex="0" title="{value} · {tm('m.rename.hint')}" onclick={begin}
    onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); begin(); } }}>{value}</span></h1>
{/if}

<style>
  h1 { font-size: 24px; line-height: 32px; margin: 0; font-weight: 650; letter-spacing: -0.025em; }
  .ren { cursor: text; border-radius: var(--r-sm); display: block; overflow: hidden; text-overflow: ellipsis; }
  .ren:hover { background: var(--hv); }
  .ren:focus-visible { outline: 2px solid var(--ac); outline-offset: 2px; }
  .ed { min-width: 0; }
  .inp { all: unset; box-sizing: border-box; width: 100%; height: 32px; padding: 0 var(--s2); margin: 0 calc(var(--s2) * -1); font: 650 24px/32px var(--font); letter-spacing: -0.025em; color: var(--tx); background: var(--panel); border: 1px solid var(--ac); border-radius: var(--r); }
  .er { margin: var(--s1) 0 0; color: #ff8f90; font-size: 12px; line-height: 16px; }
</style>

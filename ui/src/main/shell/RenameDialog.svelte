<script lang="ts">
  // «Переименовать» from a list item or the sidebar: one field, Enter saves. Pages rename in place instead.
  import Button from '../../lib/components/Button.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import { tick } from 'svelte';
  import { api } from '../api';
  import { errText } from '../errors';
  import { tm } from '../i18n';
  import { library, ui } from '../state.svelte';

  const t = $derived(ui.renameTarget);
  let draft = $state('');
  let error = $state('');
  let busy = $state(false);
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (t) { draft = t.title; error = ''; void tick().then(() => { input?.focus(); input?.select(); }); }
  });

  function close() { ui.renameTarget = null; }
  async function save() {
    if (!t || busy) return;
    const title = draft.trim();
    if (!title) { error = tm('m.rename.empty'); input?.focus(); return; }
    if (title === t.title) return close();
    busy = true;
    try {
      if (t.kind === 'doc') { await api.docPatch(t.id, { title }); ui.docsRev++; }
      else if (!(await library.rename(t.kind, t.id, title))) { busy = false; return; }
      close();
    } catch (e) { ui.notify(errText(e)); }
    busy = false;
  }
</script>

<Modal open={!!t} title={tm('m.rename')} width={420} closeLabel={tm('common.close')} onclose={close}>
  <form onsubmit={(e) => { e.preventDefault(); void save(); }}>
    <input bind:this={input} class="inp" bind:value={draft} aria-label={tm('m.rename.label')} aria-invalid={error ? 'true' : undefined} data-testid="rename-input" maxlength="200" oninput={() => (error = '')} />
    {#if error}<p class="er" role="alert" data-testid="rename-error">{error}</p>{/if}
  </form>
  {#snippet footer()}
    <Button onclick={close}>{tm('common.cancel')}</Button>
    <Button variant="primary" onclick={save} disabled={busy}>{tm('common.save')}</Button>
  {/snippet}
</Modal>

<style>
  .inp { all: unset; box-sizing: border-box; width: 100%; height: 36px; padding: 0 var(--s3); font-size: 14px; color: var(--tx); background: var(--bg); border: 1px solid var(--line-strong); border-radius: var(--r); }
  .inp:focus-visible { border-color: var(--ac); outline: none; }
  .er { margin: var(--s2) 0 0; color: #ff8f90; font-size: 12px; line-height: 16px; }
</style>

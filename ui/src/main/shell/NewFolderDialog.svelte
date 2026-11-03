<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import TextInput from '../../lib/components/TextInput.svelte';
  import { api } from '../api';
  import { errText } from '../errors';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';
  import { library, ui } from '../state.svelte';

  let name = $state('');
  let localOnly = $state(false);
  let error = $state('');
  $effect(() => { if (ui.newFolderOpen) { name = ''; localOnly = false; error = ''; } });

  async function create() {
    const title = name.trim();
    if (!title) return;
    try {
      const f = await api.folderCreate({ title, allow_cloud: !localOnly, instructions: '' });
      await library.reload();
      ui.newFolderOpen = false;
      if (f.id.kind === 'user') router.go(hrefs.folder(f.id.id));
    } catch (e) { error = errText(e); }
  }
</script>

<Modal open={ui.newFolderOpen} title={tm('main.newFolder')} width={400} closeLabel={tm('common.close')} onclose={() => (ui.newFolderOpen = false)}>
  <TextInput bind:value={name} label={tm('m.folderName')} placeholder={tm('m.folderName')} autofocus onenter={create} />
  <label class="privacy" data-testid="new-folder-local-only"><input type="checkbox" bind:checked={localOnly} /><span><b>{tm('m.folderLocalOnly')}</b><small>{tm('m.folderLocalOnlyHint')}</small></span></label>
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  {#snippet footer()}
    <Button onclick={() => (ui.newFolderOpen = false)}>{tm('common.cancel')}</Button>
    <Button variant="primary" disabled={!name.trim()} onclick={create}>{tm('m.create')}</Button>
  {/snippet}
</Modal>

<style>
  .err { margin: var(--s2) 0 0; color: var(--warn); font-size: 12px; }
  .privacy { display: flex; align-items: flex-start; gap: var(--s2); margin-top: var(--s3); cursor: pointer; font-size: 13px; }
  .privacy input { margin: 2px 0 0; accent-color: var(--ac); }
  .privacy span { display: grid; gap: 2px; }
  .privacy small { color: var(--tx2); font-size: 12px; }
</style>

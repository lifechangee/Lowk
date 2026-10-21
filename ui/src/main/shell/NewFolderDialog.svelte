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
  let error = $state('');
  $effect(() => { if (ui.newFolderOpen) { name = ''; error = ''; } });

  async function create() {
    const title = name.trim();
    if (!title) return;
    try {
      const f = await api.folderCreate({ title, tags: [], allow_cloud: true, instructions: '' });
      await library.reload();
      ui.newFolderOpen = false;
      if (f.id.kind === 'user') router.go(hrefs.folder(f.id.id));
    } catch (e) { error = errText(e); }
  }
</script>

<Modal open={ui.newFolderOpen} title={tm('main.newFolder')} width={400} closeLabel={tm('common.close')} onclose={() => (ui.newFolderOpen = false)}>
  <TextInput bind:value={name} label={tm('m.folderName')} placeholder={tm('m.folderName')} autofocus onenter={create} />
  {#if error}<p class="err" role="alert">{error}</p>{/if}
  {#snippet footer()}
    <Button onclick={() => (ui.newFolderOpen = false)}>{tm('common.cancel')}</Button>
    <Button variant="primary" disabled={!name.trim()} onclick={create}>{tm('m.create')}</Button>
  {/snippet}
</Modal>

<style>
  .err { margin: var(--s2) 0 0; color: var(--warn); font-size: 12px; }
</style>

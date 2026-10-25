<script lang="ts">
  // «Удалить папку»: confirm with the counts, then the folder (all calls and documents) goes to the Trash.
  import Button from '../../lib/components/Button.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import { tPlural } from '../../lib/i18n/locale.svelte';
  import { api } from '../api';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';
  import { library, ui } from '../state.svelte';

  const key = $derived(ui.deleteFolderKey);
  const folder = $derived(key ? library.folder(key) : undefined);
  const calls = $derived(key ? library.callsIn(key).length : 0);
  let docs = $state(0);
  let busy = $state(false);
  const recording = $derived(key ? library.recordingIn(key) : false);

  $effect(() => {
    const k = key;
    docs = 0;
    if (k) void api.docList(k).then((d) => { if (k === ui.deleteFolderKey) docs = d.length; }).catch(() => undefined);
  });

  const body = $derived.by(() => {
    const title = folder?.title ?? '';
    const c = tPlural('main.calls', calls);
    const d = tPlural('main.documents', docs);
    if (calls && docs) return tm('m.folderDelete.body', { title, calls: c, docs: d });
    if (calls) return tm('m.folderDelete.bodyNoDocs', { title, calls: c });
    if (docs) return tm('m.folderDelete.bodyOnlyDocs', { title, docs: d });
    return tm('m.folderDelete.bodyEmpty', { title });
  });

  const close = () => (ui.deleteFolderKey = null);
  async function go() {
    const k = key;
    if (!k || busy || recording) return;
    busy = true;
    const ok = await library.deleteFolder(k);
    busy = false;
    if (!ok) return;
    close();
    if (router.route.name === 'folder' && router.route.id === k) router.go(hrefs.home);
  }
</script>

<Modal open={key !== null && !!folder} title={tm('m.folderDelete.title')} width={440} closeLabel={tm('common.close')} onclose={close}>
  <div data-testid="folder-delete-confirm">
    {#if recording}
      <p class="warn" role="alert" data-testid="folder-delete-recording">{tm('m.folderDelete.recording')}</p>
    {:else}
      <p class="body" data-testid="folder-delete-body">{body}</p>
      <p class="note">{tm('m.folderDelete.note')}</p>
    {/if}
  </div>
  {#snippet footer()}
    <Button onclick={close}>{tm('common.cancel')}</Button>
    <Button variant="danger" icon="trash" disabled={busy || recording} onclick={go}>{tm('m.folderDelete')}</Button>
  {/snippet}
</Modal>

<style>
  .body { margin: 0; color: var(--tx); }
  .note { margin: var(--s2) 0 0; color: var(--tx2); font-size: 13px; }
  .warn { margin: 0; color: var(--warn); }
</style>

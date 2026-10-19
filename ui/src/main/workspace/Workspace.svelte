<script lang="ts">
  import Button from '../../lib/components/Button.svelte';
  import Modal from '../../lib/components/Modal.svelte';
  import TextInput from '../../lib/components/TextInput.svelte';
  import { api, folderKey, isSystemKey } from '../api';
  import { tm } from '../i18n';
  import { hrefs, router, type Route } from '../router.svelte';
  import { library } from '../state.svelte';
  import CallPage from './CallPage.svelte';
  import FolderTree from './FolderTree.svelte';
  import FolderView from './FolderView.svelte';
  import LivePage from './LivePage.svelte';
  import RightPane from './RightPane.svelte';

  let { route }: { route: Extract<Route, { name: 'folder' | 'call' }> } = $props();

  const call = $derived(route.name === 'call' ? library.call(route.id) : undefined);
  const folder = $derived(route.name === 'folder' ? route.id : call ? (call.deleted ? 'sys:trash' : folderKey(call.folder)) : '');
  const callId = $derived(route.name === 'call' ? route.id : null);
  const showRight = $derived(!isSystemKey(folder) || !!callId);

  let creating = $state(false);
  let name = $state('');
  async function create() {
    const title = name.trim();
    if (!title) return;
    const f = await api.folderCreate({ title, tags: [], allow_cloud: true, instructions: '' });
    await library.reload();
    creating = false; name = '';
    if (f.id.kind === 'user') router.go(hrefs.folder(f.id.id));
  }
</script>

<div class="ws" class:norigh={!showRight}>
  <FolderTree {folder} {callId} onnewfolder={() => (creating = true)} />
  <main class="ctr">
    {#if route.name === 'call'}
      {#if call && call.ended === null}<LivePage />{:else if call || !library.loaded}{#key route.id}<CallPage id={route.id} />{/key}{/if}
    {:else}
      <FolderView folder={route.id} />
    {/if}
  </main>
  {#if showRight}<RightPane {folder} {callId} />{/if}
</div>

<Modal open={creating} title={tm('main.newFolder')} width={400} closeLabel={tm('common.close')} onclose={() => (creating = false)}>
  <TextInput bind:value={name} label={tm('m.folderName')} placeholder={tm('m.folderName')} autofocus onenter={create} />
  {#snippet footer()}
    <Button onclick={() => (creating = false)}>{tm('common.cancel')}</Button>
    <Button variant="primary" disabled={!name.trim()} onclick={create}>{tm('m.create')}</Button>
  {/snippet}
</Modal>

<style>
  .ws { display: grid; grid-template-columns: 264px minmax(0, 1fr) 304px; height: 100%; min-height: 0; }
  .ws.norigh { grid-template-columns: 264px minmax(0, 1fr); }
  .ctr { display: flex; flex-direction: column; overflow: hidden; background: var(--bg); min-width: 0; min-height: 0; }
  @media (max-width: 1180px) { .ws { grid-template-columns: 232px minmax(0, 1fr); } .ws > :global(.right) { display: none; } }
</style>

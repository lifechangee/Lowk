<script lang="ts">
  import Button from '../lib/components/Button.svelte';
  import Modal from '../lib/components/Modal.svelte';
  import { tm } from './i18n';
  import { hrefs, router } from './router.svelte';
  import { ui } from './state.svelte';
  import EngineCard from './settings/EngineCard.svelte';

  const need = $derived(ui.engineNeeded);
  const close = () => (ui.engineNeeded = null);
  const toSettings = () => { close(); router.go(hrefs.settings('providers')); };
</script>

<Modal open={need !== null} title={tm('m.engineNeeded.title')} closeLabel={tm('common.close')} onclose={close}>
  {#if need}
    <div data-testid="engine-needed">
      <p class="msg">{need.kind === 'cloud' ? tm('m.engineNeeded.cloud') : tm('m.engineNeeded.local')}</p>
      {#if need.kind === 'local'}
        <EngineCard engine={need.engine} />
      {/if}
      <div class="acts">
        <Button size="sm" onclick={toSettings}>{need.kind === 'cloud' ? tm('m.engineNeeded.settings') : tm('m.engineNeeded.chooseCloud')}</Button>
      </div>
    </div>
  {/if}
</Modal>

<style>
  .msg { margin: 0 0 var(--s3); font-size: 13px; line-height: 20px; color: var(--tx2); }
  .acts { display: flex; justify-content: flex-end; }
</style>

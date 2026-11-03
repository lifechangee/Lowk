<script lang="ts">
  import Button from '../lib/components/Button.svelte';
  import Modal from '../lib/components/Modal.svelte';
  import { tm } from './i18n';
  import { installs } from './installs.svelte';
  import { hrefs, router } from './router.svelte';
  import { ui } from './state.svelte';
  import EngineCard from './settings/EngineCard.svelte';

  const need = $derived(ui.engineNeeded);
  // The download of this engine is already running (started in onboarding / settings, or by an earlier click):
  // say so and show its progress; «download the model» is never offered over a running download.
  const downloading = $derived(!!need && need.kind === 'local' && isDownloadActive(installs.active[need.engine], need.downloading === true));
  const close = () => (ui.engineNeeded = null);
  const toSettings = () => { close(); router.go(hrefs.settings('providers')); };
</script>

<script module lang="ts">
  export function isDownloadActive(live: boolean | undefined, initial: boolean): boolean {
    return live ?? initial;
  }
</script>

<Modal open={need !== null} title={downloading ? tm('m.engineNeeded.titleBusy') : tm('m.engineNeeded.title')} closeLabel={tm('common.close')} onclose={close}>
  {#if need}
    <div data-testid="engine-needed" data-downloading={downloading}>
      <p class="msg">{need.kind === 'cloud' ? tm('m.engineNeeded.cloud') : downloading ? tm('m.engineNeeded.busy') : tm('m.engineNeeded.local')}</p>
      {#if need.kind === 'local'}
        <EngineCard engine={need.engine} />
      {/if}
      <div class="acts">
        {#if downloading}
          <Button size="sm" onclick={toSettings}>{tm('m.engineNeeded.showProgress')}</Button>
        {:else}
          <Button size="sm" onclick={toSettings}>{need.kind === 'cloud' ? tm('m.engineNeeded.settings') : tm('m.engineNeeded.chooseCloud')}</Button>
        {/if}
      </div>
    </div>
  {/if}
</Modal>

<style>
  .msg { margin: 0 0 var(--s3); font-size: 13px; line-height: 20px; color: var(--tx2); }
  .acts { display: flex; justify-content: flex-end; }
</style>

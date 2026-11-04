<script lang="ts">
  import Button from '../lib/components/Button.svelte';
  import Modal from '../lib/components/Modal.svelte';
  import { tm } from './i18n';
  import { hrefs, router } from './router.svelte';
  import { api } from './api';
  import { installs } from './installs.svelte';
  import { session, ui } from './state.svelte';
  import EngineCard from './settings/EngineCard.svelte';

  const need = $derived(ui.engineNeeded);
  // The download of this engine is already running (started in onboarding / settings, or by an earlier click):
  // say so and show its progress; «download the model» is never offered over a running download.
  const downloading = $derived(!!need && need.kind === 'local' && isDownloadActive(installs.active[need.engine], need.downloading === true));
  const close = () => { ui.engineNeeded = null; session.discardPendingStart(); };
  const toSettings = () => { close(); router.go(hrefs.settings('providers')); };
  $effect(() => {
    const current = need;
    installs.ended;
    if (!current || current.kind !== 'local' || installs.active[current.engine]) return;
    let cancelled = false;
    void api.engineStatus().then((rows) => {
      const ready = isEngineReady(rows.find((r) => r.descriptor.id === current.engine));
      if (cancelled || ui.engineNeeded?.engine !== current.engine) return;
      completeEngineNeeded(ready, () => { ui.engineNeeded = null; }, () => {
        ui.notify(tm('m.engineNeeded.ready'), {
          label: tm('m.engineNeeded.startCall'),
          run: () => void session.retryAfterEngineReady(),
          dismissLabel: tm('common.close'),
        }, 12000);
      });
    }).catch(() => {});
    return () => { cancelled = true; };
  });
</script>

<script module lang="ts">
  export function isDownloadActive(live: boolean | undefined, initial: boolean): boolean {
    return live ?? initial;
  }
  export function isEngineReady(engine: { installed: boolean; runtime_missing: boolean; extra_mb?: number } | undefined): boolean {
    return !!engine && engine.installed && !engine.runtime_missing && (engine.extra_mb ?? 0) === 0;
  }
  export function completeEngineNeeded(ready: boolean, close: () => void, notify: () => void): boolean {
    if (!ready) return false;
    close();
    notify();
    return true;
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

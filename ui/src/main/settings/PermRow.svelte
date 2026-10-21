<script lang="ts">
  import type { PermKind } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon, { type IconName } from '../../lib/components/Icon.svelte';
  import { tm, type MKey } from '../i18n';
  import { perms } from '../permissions.svelte';

  let { kind, withOpen = false }: { kind: PermKind; withOpen?: boolean } = $props();
  const icons: Record<PermKind, IconName> = { microphone: 'mic', system_audio: 'monitor', screen_recording: 'eye', input_monitoring: 'keyboard', accessibility: 'shield' };
  const st = $derived(perms.state[kind]);
  const restart = $derived(perms.needsRestart(kind));
</script>

<div class="perm" data-testid="perm-{kind}">
  <span class="ic"><Icon name={icons[kind]} size={18} /></span>
  <div class="t">
    <b class="lbl">{tm(`m.perm.${kind}` as MKey)}</b>
    <small class="lbl">{tm(`m.perm.${kind}.why` as MKey)}</small>
    {#if restart}<small class="warn" role="status">{tm('m.perm.restartWhy')}</small>{/if}
  </div>
  {#if st === 'granted'}
    <span class="pill lbl"><Icon name="check" size={12} />{tm('onboarding.permissions.granted')}</span>
  {:else if restart}
    <Button size="sm" variant="primary" onclick={() => perms.relaunch()}>{tm('m.perm.relaunch')}</Button>
  {:else}
    {#if withOpen}<span class="state lbl">{st === 'denied' ? tm('m.permDenied') : tm('m.permUnknown')}</span>{/if}
    <Button size="sm" onclick={() => perms.request(kind)}>{tm('onboarding.permissions.allow')}</Button>
    {#if withOpen}<Button size="sm" variant="ghost" onclick={() => perms.open(kind)}>{tm('onboarding.permissions.open')}</Button>{/if}
  {/if}
</div>

<style>
  .perm { display: flex; gap: var(--s3); align-items: center; padding: var(--s3) var(--s4); border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--panel); margin-bottom: var(--s2); }
  .ic { width: 36px; height: 36px; border-radius: var(--r-lg); background: var(--hv); display: grid; place-items: center; color: var(--tx2); flex: none; border: 1px solid var(--line); }
  .t { flex: 1; min-width: 0; }
  .t b { display: block; font-weight: 600; }
  small { display: block; color: var(--tx2); font-size: 12px; }
  small.warn { color: var(--warn); margin-top: var(--s1); }
  .pill { display: inline-flex; align-items: center; gap: var(--s1); font-size: 12px; height: 20px; padding: 0 var(--s2); border-radius: 99px; background: var(--acbg); color: var(--ac); font-weight: 600; max-width: 40%; }
  .state { font-size: 12px; color: var(--tx3); flex: none; }
</style>

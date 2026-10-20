<script lang="ts">
  import { onMount } from 'svelte';
  import type { PermKind } from '../../lib/ipc';
  import Button from '../../lib/components/Button.svelte';
  import Icon, { type IconName } from '../../lib/components/Icon.svelte';
  import { tm, type MKey } from '../i18n';
  import { PERM_ORDER, perms } from '../permissions.svelte';

  const icons: Record<PermKind, IconName> = { microphone: 'mic', system_audio: 'monitor', screen_recording: 'eye', input_monitoring: 'keyboard', accessibility: 'shield' };
  onMount(() => { perms.refresh(); });
</script>

<svelte:window onfocus={() => perms.refresh()} />

<h2>{tm('m.set.permissions')}</h2>
<p class="lead">{tm('m.permissionsHint')}</p>
{#each PERM_ORDER as k (k)}
  {@const st = perms.state[k]}
  {#if st !== 'not_applicable'}
    <div class="perm">
      <span class="ic"><Icon name={icons[k]} size={18} /></span>
      <div class="t"><b class="lbl">{tm(`m.perm.${k}` as MKey)}</b><small class="lbl">{tm(`m.perm.${k}.why` as MKey)}</small></div>
      {#if st === 'granted'}
        <span class="pill lbl"><Icon name="check" size={12} />{tm('onboarding.permissions.granted')}</span>
      {:else}
        <span class="state lbl">{st === 'denied' ? tm('m.permDenied') : tm('m.permUnknown')}</span>
        <Button size="sm" onclick={() => perms.request(k)}>{tm('onboarding.permissions.allow')}</Button>
        <Button size="sm" variant="ghost" onclick={() => perms.open(k)}>{tm('onboarding.permissions.open')}</Button>
      {/if}
    </div>
  {/if}
{/each}
<p class="note">{tm('onboarding.permissions.windowsNote')}</p>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s1); }
  .lead { color: var(--tx2); margin: 0 0 var(--s4); font-size: 13px; }
  .perm { display: flex; gap: var(--s3); align-items: center; padding: var(--s3) var(--s4); border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--panel); margin-bottom: var(--s2); }
  .ic { width: 36px; height: 36px; border-radius: var(--r-lg); background: var(--hv); display: grid; place-items: center; color: var(--tx2); flex: none; border: 1px solid var(--line); }
  .t { flex: 1; min-width: 0; }
  .t b { display: block; font-weight: 600; }
  small { display: block; color: var(--tx2); font-size: 12px; }
  .pill { display: inline-flex; align-items: center; gap: var(--s1); font-size: 12px; height: 20px; padding: 0 var(--s2); border-radius: 99px; background: var(--acbg); color: var(--ac); font-weight: 600; max-width: 40%; }
  .state { font-size: 12px; color: var(--tx3); flex: none; max-width: none; overflow: visible; }
  .note { color: var(--tx3); font-size: 12px; margin-top: var(--s3); }
</style>

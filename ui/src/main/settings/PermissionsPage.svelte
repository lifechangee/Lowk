<script lang="ts">
  import { onMount } from 'svelte';
  import Button from '../../lib/components/Button.svelte';
  import { api } from '../api';
  import { tm } from '../i18n';
  import { PERM_ORDER, perms } from '../permissions.svelte';
  import PermRow from './PermRow.svelte';

  onMount(() => perms.watch());
</script>

<h2>{tm('m.set.permissions')}</h2>
<p class="lead">{tm('m.permissionsHint')}</p>
{#each PERM_ORDER as k (k)}
  {#if perms.state[k] !== 'not_applicable'}<PermRow kind={k} withOpen />{/if}
{/each}
{#if perms.anyNeedsRestart}
  <div class="restart" role="status"><span class="lbl">{tm('m.perm.restartNeeded')}</span><Button size="sm" variant="primary" onclick={() => perms.relaunch()}>{tm('m.perm.relaunch')}</Button></div>
{/if}
{#if api.platform() === 'windows'}<p class="note">{tm('onboarding.permissions.windowsNote')}</p>{/if}

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s1); }
  .lead { color: var(--tx2); margin: 0 0 var(--s4); font-size: 13px; }
  .restart { display: flex; align-items: center; justify-content: space-between; gap: var(--s3); padding: var(--s3) var(--s4); border: 1px solid var(--warn); border-radius: var(--r-lg); margin-top: var(--s3); font-size: 13px; }
  .note { color: var(--tx3); font-size: 12px; margin-top: var(--s3); }
</style>

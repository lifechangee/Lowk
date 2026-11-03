<script lang="ts">
  // A call page whose file is gone (deleted outside Lowk, vault moved), or could not be read: a clear state with a way back.
  import Button from '../../lib/components/Button.svelte';
  import Icon from '../../lib/components/Icon.svelte';
  import { tm } from '../i18n';
  import { hrefs, router } from '../router.svelte';

  let { error = '' }: { error?: string } = $props();

  function back() {
    if (history.length > 1) history.back();
    else router.go(hrefs.home);
  }
</script>

<div class="state" role={error ? 'alert' : 'status'} data-testid="call-missing">
  <Icon name={error ? 'alert' : 'doc'} size={20} />
  <b>{error ? tm('m.callLoadFailed') : tm('m.callMissing.title')}</b>
  <p class="lbl">{error || tm('m.callMissing.body')}</p>
  <Button onclick={back}>{tm('m.callMissing.back')}</Button>
</div>

<style>
  .state { display: flex; flex-direction: column; align-items: center; gap: var(--s3); padding: var(--s12) var(--s8); color: var(--tx2); text-align: center; }
  .state b { color: var(--tx); font-size: 16px; }
  .state p { margin: 0; max-width: 420px; }
</style>

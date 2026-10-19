<script lang="ts">
  import { ACTIONS_EXTRA, ACTIONS_PRIMARY } from '../binds';
  import { tm } from '../i18n';
  import BindEditor from './BindEditor.svelte';
  let more = $state(false);
</script>

<h2>{tm('settings.hotkeys')}</h2>
<p class="lead">{tm('settings.hotkeys.hint')}</p>
<BindEditor actions={more ? [...ACTIONS_PRIMARY, ...ACTIONS_EXTRA] : ACTIONS_PRIMARY} />
<button class="more" onclick={() => (more = !more)}>{more ? tm('m.fewerActions') : tm('m.moreActions')}</button>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s1); }
  .lead { color: var(--tx2); margin: 0 0 var(--s3); font-size: 13px; }
  .more { all: unset; cursor: pointer; color: var(--ac); font-size: 13px; margin-top: var(--s2); }
  .more:hover { text-decoration: underline; }
</style>

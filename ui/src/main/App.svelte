<script lang="ts">
  import '../lib/tokens.css';
  import { onMount } from 'svelte';
  import Badge from '../lib/components/Badge.svelte';
  import { resolveLang, setLang, t } from '../lib/i18n';
  import { commands } from '../lib/ipc';
  import { settings } from '../lib/stores/settings.svelte';

  let ready = $state(false);
  onMount(async () => {
    await settings.load();
    const locale = await commands.localeGet();
    setLang(resolveLang(settings.value?.ui?.language ?? 'auto', locale));
    ready = true;
  });
</script>

{#if ready}
  <main>
    <h1>{t('app.name')}</h1>
    <Badge text={t('main.title')} />
  </main>
{/if}

<style>
  main { padding: var(--s6); }
  h1 { font-weight: 600; margin: 0 0 var(--s3); }
</style>

<script lang="ts">
  import type { Template } from '../../lib/ipc';
  import { locale } from '../../lib/i18n/locale.svelte';
  import { ACTIONS_EXTRA, ACTIONS_PRIMARY, HIDDEN_TEMPLATES, orphanTemplateBinds, templateSlot, type Slot } from '../binds';
  import { tm, type MKey } from '../i18n';
  import { bindSlot, defaultId } from '../templates';
  import { prefs } from '../state.svelte';
  import BindEditor from './BindEditor.svelte';
  let more = $state(false);

  // «Brief» and «Complement my answer» sit in the main list; every other template (built-in or created
  // by the user) gets its own row here, bound through `ask_template` + the template id.
  /** Built-in templates the overlay offers even when settings.toml does not list them. */
  const BUILTIN_EXTRA = ['say', 'factcheck', 'followup', 'recap'];
  const templates = $derived<Template[]>(prefs.value.templates ?? []);
  const rows = $derived.by(() => {
    const out: { slot: Slot; label: string }[] = [];
    const ids = new Set<string>();
    const def = defaultId(templates, prefs.value.llm?.default_template);
    for (const t of templates) {
      if (HIDDEN_TEMPLATES.has(t.id) || ids.has(t.id)) continue;
      ids.add(t.id);
      // the default template owns «Ask: brief answer», complement its own main-list row; a non-default «brief» gets a row here
      const slot = bindSlot(t.id, def);
      if (slot.action === 'ask_brief' || slot.action === 'ask_complement') continue;
      const name = locale.lang === 'ru' ? t.names.ru : t.names.en;
      out.push({ slot, label: slot.action === 'ask_template' ? tm('m.act.askTemplate', { name }) : tm(`m.act.${slot.action}` as MKey) });
    }
    for (const id of BUILTIN_EXTRA) {
      if (ids.has(id)) continue;
      const slot = templateSlot(id);
      out.push({ slot, label: tm(`m.act.${slot.action}` as MKey) });
    }
    // A bind whose template was deleted stays listed (so it can be cleared) instead of silently failing.
    for (const b of orphanTemplateBinds(prefs.value.binds ?? [], [...templates.map((t) => t.id), ...ids])) {
      out.push({ slot: { action: 'ask_template', template: b.template }, label: tm('m.act.askTemplate', { name: b.template ?? '' }) });
    }
    return out;
  });
</script>

<h2>{tm('settings.hotkeys')}</h2>
<p class="lead">{tm('settings.hotkeys.hint')}</p>
<BindEditor actions={more ? [...ACTIONS_PRIMARY, ...ACTIONS_EXTRA] : ACTIONS_PRIMARY} />
<button class="more" onclick={() => (more = !more)}>{more ? tm('m.fewerActions') : tm('m.moreActions')}</button>

<h2 class="gap">{tm('m.act.templates')}</h2>
<p class="lead">{tm('m.act.templatesHint')}</p>
<div data-testid="template-binds"><BindEditor actions={[]} templates={rows} showReset={false} hints={false} /></div>

<style>
  h2 { font: 600 var(--fs-lg) var(--font); margin: 0 0 var(--s1); }
  .gap { margin-top: var(--s8); }
  .lead { color: var(--tx2); margin: 0 0 var(--s3); font-size: 13px; }
  .more { all: unset; cursor: pointer; color: var(--ac); font-size: 13px; margin-top: var(--s2); }
  .more:hover { text-decoration: underline; }
</style>

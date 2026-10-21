<script lang="ts">
  // Markdown editor on CodeMirror 6, loaded lazily (its own chunk) the first time a document is opened for editing.
  // If the chunk cannot load, a plain textarea takes over, so editing never silently disappears.
  import { onMount } from 'svelte';
  import { tm } from '../i18n';

  let { value, onchange, onsave }: { value: string; onchange: (v: string) => void; onsave: () => void } = $props();

  let host: HTMLDivElement | undefined = $state();
  let fallback = $state(false);
  let ready = $state(false);
  let local = $state('');
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  let view: any = null;

  onMount(() => {
    local = value;
    let dead = false;
    void (async () => {
      try {
        const [{ EditorView, keymap, drawSelection }, { EditorState }, { defaultKeymap, history, historyKeymap }, { markdown, markdownKeymap }] = await Promise.all([
          import('@codemirror/view'),
          import('@codemirror/state'),
          import('@codemirror/commands'),
          import('@codemirror/lang-markdown'),
        ]);
        if (dead || !host) return;
        const theme = EditorView.theme(
          {
            '&': { color: 'var(--tx)', backgroundColor: 'var(--bg)', fontSize: '13px', height: '100%' },
            '.cm-scroller': { fontFamily: 'var(--mono)', lineHeight: '20px', overflow: 'auto' },
            '.cm-content': { caretColor: 'var(--tx)', padding: '12px 0' },
            '.cm-line': { padding: '0 16px' },
            '&.cm-focused': { outline: 'none' },
            '.cm-cursor': { borderLeftColor: 'var(--tx)' },
            '&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection': { backgroundColor: 'var(--acbg)' },
          },
          { dark: true },
        );
        view = new EditorView({
          parent: host,
          state: EditorState.create({
            doc: value,
            extensions: [
              history(),
              drawSelection(),
              EditorView.lineWrapping,
              markdown(),
              theme,
              EditorView.contentAttributes.of({ 'aria-label': tm('docs.editor'), 'aria-multiline': 'true' }),
              keymap.of([{ key: 'Mod-s', preventDefault: true, run: () => { onsave(); return true; } }, ...markdownKeymap, ...defaultKeymap, ...historyKeymap]),
              EditorView.updateListener.of((u) => { if (u.docChanged) onchange(u.state.doc.toString()); }),
            ],
          }),
        });
        ready = true;
      } catch {
        fallback = true;
      }
    })();
    return () => { dead = true; view?.destroy(); };
  });

  // The document was replaced from outside (revert): push it into the editor.
  $effect(() => {
    const v = value;
    if (view && view.state.doc.toString() !== v) view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: v } });
    if (fallback) local = v;
  });
</script>

{#if fallback}
  <textarea class="fb" aria-label={tm('docs.editor')} bind:value={local} oninput={() => onchange(local)} onkeydown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === 's') { e.preventDefault(); onsave(); } }}></textarea>
{:else}
  <div class="ed" class:ready bind:this={host} data-testid="md-editor"></div>
{/if}

<style>
  .ed { height: 100%; min-height: 320px; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg); overflow: hidden; }
  .ed:focus-within { border-color: var(--ac); }
  .fb { width: 100%; height: 100%; min-height: 320px; resize: none; border: 1px solid var(--line); border-radius: var(--r-lg); background: var(--bg); color: var(--tx); padding: var(--s3) var(--s4); font: 13px/20px var(--mono); outline: none; }
  .fb:focus { border-color: var(--ac); }
</style>

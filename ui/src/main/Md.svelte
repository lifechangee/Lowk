<script lang="ts">
  import { blocks, type Inline } from './md';

  let { md, cursor = false }: { md: string; cursor?: boolean } = $props();
  const parsed = $derived(blocks(md));
</script>

{#snippet txt(parts: Inline[])}{#each parts as p, i (i)}{#if p.b}<b>{p.t}</b>{:else if p.code}<code>{p.t}</code>{:else}{p.t}{/if}{/each}{/snippet}

<div class="md" data-testid="md">
  {#each parsed as b, i (i)}
    {#if b.k === 'h'}
      {#if b.level <= 2}<h3 class="h2">{@render txt(b.text)}</h3>{:else}<h4 class="h3">{@render txt(b.text)}</h4>{/if}
    {:else if b.k === 'p'}<p>{@render txt(b.text)}</p>
    {:else if b.k === 'ul'}<ul>{#each b.items as it, j (j)}<li>{@render txt(it)}</li>{/each}</ul>
    {:else if b.k === 'ol'}<ol>{#each b.items as it, j (j)}<li value={b.nums[j]}>{@render txt(it)}</li>{/each}</ol>
    {:else if b.k === 'quote'}<blockquote>{@render txt(b.text)}</blockquote>
    {:else if b.k === 'code'}<pre>{b.text}</pre>
    {:else if b.k === 'table'}
      <div class="tw"><table><thead><tr>{#each b.head as c, j (j)}<th>{@render txt(c)}</th>{/each}</tr></thead>
        <tbody>{#each b.rows as r, j (j)}<tr>{#each r as c, k (k)}<td>{@render txt(c)}</td>{/each}</tr>{/each}</tbody></table></div>
    {/if}
  {/each}
  {#if cursor}<span class="cur" aria-hidden="true"></span>{/if}
</div>

<style>
  .md { color: var(--tx); font-size: 14px; line-height: 20px; }
  .md :global(:first-child) { margin-top: 0; }
  h3, h4 { margin: var(--s5) 0 var(--s2); font-weight: 600; }
  .h2 { font-size: 16px; line-height: 24px; }
  .h3 { font-size: 11px; line-height: 16px; letter-spacing: 0.08em; text-transform: uppercase; color: var(--tx3); }
  p { margin: 0 0 var(--s3); }
  ul, ol { margin: 0 0 var(--s3); padding-left: var(--s5); }
  li { margin: var(--s1) 0; }
  blockquote { margin: 0 0 var(--s3); padding-left: var(--s3); border-left: 2px solid var(--line-strong); color: var(--tx2); }
  pre { margin: 0 0 var(--s3); padding: var(--s3); background: var(--bg); border: 1px solid var(--line); border-radius: var(--r); font: 12px/20px var(--mono); overflow: auto; white-space: pre-wrap; }
  code { font: 12px var(--mono); background: var(--hv); border-radius: var(--s1); padding: 0 var(--s1); }
  .tw { overflow: auto; margin: 0 0 var(--s3); }
  table { border-collapse: collapse; font-size: 13px; min-width: 100%; }
  th, td { text-align: left; padding: var(--s1) var(--s3); border: 1px solid var(--line); }
  th { background: var(--hv); font-weight: 600; }
  .cur { display: inline-block; width: 8px; height: 16px; margin-left: 2px; background: var(--ac); vertical-align: text-bottom; animation: blink 1s steps(2) infinite; }
  @keyframes blink { 50% { opacity: 0; } }
  @media (prefers-reduced-motion: reduce) { .cur { animation: none; } }
</style>

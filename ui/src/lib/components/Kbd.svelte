<script lang="ts">
  let { text, onprimary = false }: { text: string; onprimary?: boolean } = $props();
</script>

<kbd class:onprimary>{text}</kbd>

<style>
  kbd { font: 600 10px/16px var(--mono); padding: 0 var(--s1); border-radius: var(--s1); border: 1px solid var(--line); color: var(--tx2); background: var(--bg); white-space: nowrap; flex: none; }
  .onprimary { background: rgba(0, 0, 0, 0.12); border-color: rgba(0, 0, 0, 0.2); color: var(--acfg); }
</style>

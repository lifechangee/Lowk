<script lang="ts">
  let { text }: { text: string } = $props();
</script>

<span class="lbl badge">{text}</span>

<style>
  .badge { display: inline-block; max-width: 100%; padding: 0 var(--s2); border-radius: var(--r-sm); background: var(--acbg); color: var(--ac); font: var(--fs-sm) var(--font); }
</style>

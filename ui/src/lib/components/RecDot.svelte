<script lang="ts">
  let { pulse = false, onprimary = false }: { pulse?: boolean; onprimary?: boolean } = $props();
</script>

<span class="rdot" class:pulse style:background={onprimary ? 'var(--acfg)' : undefined}></span>

<style>
  .rdot { width: 8px; height: 8px; border-radius: 50%; background: var(--rec); flex: none; display: inline-block; }
  .pulse { animation: pl 1.6s infinite; }
  @keyframes pl { 0% { box-shadow: 0 0 0 0 rgba(255, 77, 79, 0.5); } 70% { box-shadow: 0 0 0 7px rgba(255, 77, 79, 0); } 100% { box-shadow: 0 0 0 0 rgba(255, 77, 79, 0); } }
  @media (prefers-reduced-motion: reduce) { .pulse { animation: none; } }
</style>

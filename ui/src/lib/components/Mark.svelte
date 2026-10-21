<script lang="ts">
  // The Lowk ghost mark (peeking over the edge). Monochrome, follows currentColor; the eyes are cut out.
  let { size = 16 }: { size?: number } = $props();
  const id = `lm${Math.random().toString(36).slice(2, 8)}`;
</script>

<svg width={size} height={(size * 54) / 84} viewBox="8 48 84 54" aria-hidden="true" style="display:block;flex:none">
  <mask id={id} maskUnits="userSpaceOnUse" x="0" y="40" width="100" height="70">
    <rect x="0" y="40" width="100" height="70" fill="#fff" />
    <path d="M31 71H47A8 6 0 0 1 31 71Z" fill="#000" />
    <path d="M53 69H69A8 7 0 0 1 53 69Z" fill="#000" />
  </mask>
  <path d="M10 100C20 99 22 92 22 84C22 63 34 51 50 51C66 51 78 63 78 84C78 92 80 99 90 100Z" fill="currentColor" mask="url(#{id})" />
</svg>

<script lang="ts">
  import '../lib/tokens.css';
  import { t } from '../lib/i18n';
</script>

<!-- WP9 replaces this skeleton. Overlay components must use only adaptive tokens. -->
<div class="ov lbl">{t('overlay.mode.passive')}</div>

<style>
  .ov { padding: var(--s2) var(--s3); border-radius: var(--r); background: rgba(0, 0, 0, 0.5); color: #fff; }
</style>

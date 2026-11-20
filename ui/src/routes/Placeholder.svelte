<script lang="ts">
  import type { Route } from '../lib/router.ts';

  let { route }: { route: Route } = $props();
</script>

<section>
  <h1>{route}</h1>
  <p>Window not built yet (wp-ui).</p>
</section>

<style>
  section {
    padding: 12px;
  }
  h1 {
    font-size: 15px;
    margin: 0 0 4px;
    text-transform: capitalize;
  }
</style>

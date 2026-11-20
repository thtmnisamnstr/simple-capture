<script lang="ts">
  import { routeFromHash, type Route } from './lib/router.ts';
  import Placeholder from './routes/Placeholder.svelte';

  let route: Route = $state(routeFromHash(window.location.hash));

  $effect(() => {
    const onHash = () => (route = routeFromHash(window.location.hash));
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  });
</script>

<main data-route={route}>
  <Placeholder {route} />
</main>

<style>
  :global(html, body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
    font-size: 13px;
    background: transparent;
  }
</style>

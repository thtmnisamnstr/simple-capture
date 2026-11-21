<script lang="ts">
  import { routeFromHash, type Route } from './lib/router.ts';
  import Panel from './routes/Panel.svelte';
  import Onboarding from './routes/Onboarding.svelte';
  import Bar from './routes/Bar.svelte';
  import Switcher from './routes/Switcher.svelte';
  import Region from './routes/Region.svelte';
  import Pick from './routes/Pick.svelte';
  import Saved from './routes/Saved.svelte';
  import Stage from './routes/Stage.svelte';
  import Editor from './routes/Editor.svelte';

  let route: Route = $state(routeFromHash(window.location.hash));

  $effect(() => {
    const onHash = () => (route = routeFromHash(window.location.hash));
    window.addEventListener('hashchange', onHash);
    return () => window.removeEventListener('hashchange', onHash);
  });
</script>

<main data-route={route}>
  {#if route === 'panel'}
    <Panel />
  {:else if route === 'onboarding'}
    <Onboarding />
  {:else if route === 'bar'}
    <Bar />
  {:else if route === 'switcher'}
    <Switcher />
  {:else if route === 'region'}
    <Region />
  {:else if route === 'pick'}
    <Pick />
  {:else if route === 'stage'}
    <Stage />
  {:else if route === 'editor'}
    <Editor />
  {:else}
    <Saved />
  {/if}
</main>

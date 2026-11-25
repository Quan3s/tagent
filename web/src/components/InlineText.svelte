<script lang="ts">
  import type { Inline } from '../lib/markdown';
  export let items: Inline[] = [];
</script>

{#each items as item}
  {#if item.kind === 'code'}
    <code class="inline-code">{item.text}</code>
  {:else if item.kind === 'strong'}
    <strong>{item.text}</strong>
  {:else if item.kind === 'em'}
    <em>{item.text}</em>
  {:else if item.kind === 'del'}
    <del>{item.text}</del>
  {:else if item.kind === 'link'}
    <a href={item.href} target="_blank" rel="noopener noreferrer">{item.text}</a>
  {:else}{item.text}{/if}
{/each}

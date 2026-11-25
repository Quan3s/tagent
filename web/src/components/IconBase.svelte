<script lang="ts">
  import type { Shape } from '../icons';

  export let shape: Shape[] = [];
  export let name = '';
  export let size = 20;
  export let cls = '';

  // Lucide-style icons are only rendered at 16 or 20 px.
  $: px = size >= 18 ? 20 : 16;
</script>

<svg
  xmlns="http://www.w3.org/2000/svg"
  width={px}
  height={px}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
  focusable="false"
  data-icon={name}
  class={cls}
  {...$$restProps}
>
  {#each shape as el}
    {#if el.tag === 'path'}
      <path d={el.d} />
    {:else if el.tag === 'circle'}
      <circle cx={el.cx} cy={el.cy} r={el.r} />
    {:else if el.tag === 'rect'}
      <rect x={el.x} y={el.y} width={el.w} height={el.h} rx={el.rx ?? 0} />
    {:else if el.tag === 'line'}
      <line x1={el.x1} y1={el.y1} x2={el.x2} y2={el.y2} />
    {:else}
      <polyline points={el.points} />
    {/if}
  {/each}
</svg>

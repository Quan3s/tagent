<script lang="ts">
  import InlineText from './InlineText.svelte';
  import { Check, Copy } from '../icons';
  import { parseMarkdown } from '../lib/markdown';
  import { copyText } from '../lib/clipboard';
  import { t } from '../lib/i18n';
  import { toast } from '../lib/store';

  export let text = '';
  let copied = -1;

  $: blocks = parseMarkdown(text);

  async function copyCode(code: string, index: number) {
    const ok = await copyText(code);
    if (ok) {
      copied = index;
      toast($t('copied'), 'success');
      setTimeout(() => (copied = -1), 1600);
    } else toast($t('error'), 'error');
  }
</script>

<div class="agent-md">
  {#each blocks as block, index}
    {#if block.kind === 'code'}
      <div class="code-block">
        <div class="code-head">
          <span class="mono">{block.lang || 'text'}</span>
          <button
            class="btn btn-ghost btn-sm"
            style="min-height:32px"
            aria-label={$t('copy')}
            on:click={() => copyCode(block.code, index)}
          >
            {#if copied === index}<Check size={16} />{:else}<Copy size={16} />{/if}
            <span class="tiny">{copied === index ? $t('copied') : $t('copy')}</span>
          </button>
        </div>
        <pre><code>{block.code}</code></pre>
      </div>
    {:else if block.kind === 'heading'}
      {#if block.level === 1}<h1><InlineText items={block.inline} /></h1>
      {:else if block.level === 2}<h2><InlineText items={block.inline} /></h2>
      {:else}<h3><InlineText items={block.inline} /></h3>{/if}
    {:else if block.kind === 'list'}
      {#if block.ordered}
        <ol>{#each block.items as item}<li><InlineText items={item} /></li>{/each}</ol>
      {:else}
        <ul>{#each block.items as item}<li><InlineText items={item} /></li>{/each}</ul>
      {/if}
    {:else if block.kind === 'quote'}
      <blockquote><InlineText items={block.inline} /></blockquote>
    {:else if block.kind === 'hr'}
      <hr />
    {:else}
      <p><InlineText items={block.inline} /></p>
    {/if}
  {/each}
</div>

<script lang="ts">
  import { List, CheckCircle, Info, Clock } from '../icons';
  import { todos } from '../lib/store';
  import { t } from '../lib/i18n';
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="flex min-h-[40px] flex-none items-center gap-2 border-b border-border px-3">
    <List size={16} class="text-accent" />
    <span class="text-[12px] font-semibold">{$t('todos')}</span>
    <span class="ml-auto text-[11px] muted">{$todos.length}</span>
  </div>
  <div class="scroll-thin min-h-0 flex-1 overflow-y-auto p-3">
    {#if $todos.length === 0}
      <div class="flex h-full items-center justify-center text-[13px] muted">{$t('noTodos')}</div>
    {:else}
      <ul class="space-y-2">
        {#each $todos as todo, index}
          <li class="card flex items-start gap-2 p-3 text-[13px]">
            <span class="mt-0.5 flex-none {todo.status === 'completed' || todo.done ? 'text-accent' : 'text-muted'}">
              {#if todo.status === 'completed' || todo.done}<CheckCircle size={16} />{:else if todo.status === 'in_progress'}<Clock size={16} />{:else}<Info size={16} />{/if}
            </span>
            <span class="min-w-0 flex-1">{todo.content || todo.text || String(todo)}</span>
            <span class="tiny muted">#{index + 1}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</div>

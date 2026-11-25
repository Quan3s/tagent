<script lang="ts">
  import { api } from '../lib/api';
  import { config, providers, extensions, theme, lang, toast, permissionMode } from '../lib/store';
  import { t } from '../lib/i18n';
  import { Check, Key, Cpu, Shield, Sun, Moon, Plug, Sparkles, Server, User, Refresh, Download } from '../icons';
  import type { Theme, Lang, PermissionMode } from '../lib/types';

  export let initialTab = 'providers';
  export let onProvidersChanged: () => void = () => {};
  export let onExtensionsChanged: () => void = () => {};

  type Tab = 'providers' | 'roles' | 'permissions' | 'appearance' | 'plugins' | 'skills' | 'mcp' | 'agents';
  const tabs: { id: Tab; label: string; icon: any }[] = [
    { id: 'providers', label: 'provider', icon: Key },
    { id: 'roles', label: 'roles', icon: Cpu },
    { id: 'permissions', label: 'permissions', icon: Shield },
    { id: 'appearance', label: 'theme', icon: Sun },
    { id: 'plugins', label: 'plugins', icon: Plug },
    { id: 'skills', label: 'skills', icon: Sparkles },
    { id: 'mcp', label: 'mcp', icon: Server },
    { id: 'agents', label: 'agents', icon: User },
  ];

  let tab: Tab = (tabs.some((item) => item.id === initialTab) ? initialTab : 'providers') as Tab;
  $: if (initialTab && tabs.some((item) => item.id === initialTab)) tab = initialTab as Tab;

  const keys: Record<string, string> = {};
  let roles: Record<string, string> = {};
  let rules = { allow: '', ask: '', deny: '' };
  let installSource = '';
  let busy = false;

  $: if ($config) {
    roles = { ...(($config.roles as Record<string, string>) || {}) };
    const p = ($config.permissions as Record<string, string[]>) || {};
    rules = {
      allow: (p.allow || []).join('\n'),
      ask: (p.ask || []).join('\n'),
      deny: (p.deny || []).join('\n'),
    };
  }

  $: modelOptions = Array.from(
    new Set([
      ...$providers.flatMap((p) => (p.models || []).map((m) => (m.includes('/') ? m : `${p.name}/${m}`))),
      ...Object.values(roles).filter((v) => typeof v === 'string' && v.includes('/')),
    ]),
  ).sort();

  async function saveKey(provider: string) {
    const key = keys[provider] || '';
    if (!key.trim()) return;
    busy = true;
    try {
      await api.auth(provider, key.trim());
      keys[provider] = '';
      toast(`${provider}: ${$t('saved')}`, 'success');
      onProvidersChanged();
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
    } finally {
      busy = false;
    }
  }

  async function saveSection(patch: Record<string, any>) {
    try {
      const next = await api.saveConfig({ ...$config, ...patch });
      config.set(next || { ...$config, ...patch });
      toast($t('saved'), 'success');
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
    }
  }

  function lines(value: string): string[] {
    return value.split('\n').map((line) => line.trim()).filter(Boolean);
  }

  async function extensionAction(kind: string, action: string, name?: string) {
    try {
      await api.extensionAction(kind, action, name ? { name } : { source: installSource });
      installSource = '';
      toast(`${name || action}: OK`, 'success');
      onExtensionsChanged();
    } catch (err) {
      toast(err instanceof Error ? err.message : String(err), 'error');
    }
  }

  const languageOptions: Lang[] = ['vi', 'en'];
  const ruleFields: { field: 'allow' | 'ask' | 'deny'; label: string }[] = [
    { field: 'allow', label: 'allowRules' },
    { field: 'ask', label: 'askRules' },
    { field: 'deny', label: 'denyRules' },
  ];
  const themeOptions: { id: Theme; label: string; icon: any }[] = [
    { id: 'dark', label: 'dark', icon: Moon },
    { id: 'light', label: 'light', icon: Sun },
    { id: 'amoled', label: 'amoled', icon: Moon },
  ];
</script>

<div class="flex h-full min-h-0 flex-col">
  <div class="scroll-thin flex flex-none gap-1 overflow-x-auto border-b border-border p-2">
    {#each tabs as item}
      <button class="chip flex-none" class:chip-accent={tab === item.id} on:click={() => (tab = item.id)}>
        <svelte:component this={item.icon} size={16} /> {$t(item.label)}
      </button>
    {/each}
  </div>

  <div class="scroll-thin min-h-0 flex-1 space-y-4 overflow-y-auto p-4">
    {#if tab === 'providers'}
      {#each $providers as provider}
        <div class="card space-y-2 p-3">
          <div class="flex items-center gap-2">
            <Key size={16} class="text-accent" />
            <strong class="min-w-0 flex-1 truncate">{provider.name}</strong>
            <span class="chip" class:chip-accent={provider.configured}>{provider.configured ? $t('configured') : $t('notConfigured')}</span>
          </div>
          <div class="tiny muted mono">{provider.type}</div>
          <div class="flex gap-2">
            <input class="input mono flex-1" type="password" autocomplete="off" aria-label={`${$t('apiKey')} ${provider.name}`} placeholder={$t('apiKey')} bind:value={keys[provider.name]} />
            <button class="btn btn-primary" disabled={busy || !keys[provider.name]?.trim()} on:click={() => saveKey(provider.name)}>{$t('saveKey')}</button>
          </div>
          <div class="tiny muted">{$t('apiKeyHint')}</div>
        </div>
      {:else}
        <div class="text-center text-[13px] muted">{$t('notConfigured')}</div>
      {/each}
      <button class="btn btn-ghost w-full" on:click={onProvidersChanged}><Refresh size={16} /> {$t('refresh')}</button>
    {:else if tab === 'roles'}
      {#each ['main', 'fast', 'plan', 'subagent'] as role}
        <div>
          <label class="label" for="role-{role}">{role}</label>
          <select id="role-{role}" class="input" bind:value={roles[role]}>
            <option value="">—</option>
            {#each modelOptions as option}<option value={option}>{option}</option>{/each}
          </select>
        </div>
      {/each}
      <label class="label" for="default-mode">{$t('permissionMode')}</label>
      <select id="default-mode" class="input" bind:value={$permissionMode}>
        <option value="ask">{$t('ask')}</option>
        <option value="auto-edit">{$t('autoEdit')}</option>
        <option value="plan">{$t('plan')}</option>
        <option value="yolo">{$t('yolo')}</option>
      </select>
      <button class="btn btn-primary w-full" on:click={() => saveSection({ roles })}><Check size={16} /> {$t('save')}</button>
    {:else if tab === 'permissions'}
      {#each ruleFields as rule}
        <div>
          <label class="label" for="perm-{rule.field}">{$t(rule.label)}</label>
          <textarea id="perm-{rule.field}" class="input mono" rows="4" placeholder={$t('oneRulePerLine')} bind:value={rules[rule.field]}></textarea>
        </div>
      {/each}
      <button class="btn btn-primary w-full" on:click={() => saveSection({ permissions: { allow: lines(rules.allow), ask: lines(rules.ask), deny: lines(rules.deny) } })}>
        <Check size={16} /> {$t('save')}
      </button>
    {:else if tab === 'appearance'}
      <div>
        <div class="label">{$t('theme')}</div>
        <div class="flex flex-wrap gap-2">
          {#each themeOptions as option}
            <button class="chip flex-1 justify-center" class:chip-accent={$theme === option.id} on:click={() => theme.set(option.id)}>
              <svelte:component this={option.icon} size={16} /> {$t(option.label)}
            </button>
          {/each}
        </div>
      </div>
      <div>
        <div class="label">{$t('language')}</div>
        <div class="flex gap-2">
          {#each languageOptions as code}
            <button class="chip flex-1 justify-center" class:chip-accent={$lang === code} on:click={() => lang.set(code)}>{code === 'vi' ? 'Tiếng Việt' : 'English'}</button>
          {/each}
        </div>
      </div>
    {:else}
      {#if tab === 'plugins'}
        <div class="flex gap-2">
          <input class="input mono" placeholder="owner/repo@marketplace" aria-label={$t('install')} bind:value={installSource} />
          <button class="btn btn-primary" disabled={!installSource.trim()} on:click={() => extensionAction('plugins', 'install')}><Download size={16} /> {$t('install')}</button>
        </div>
      {/if}
      {#each $extensions[tab] || [] as item}
        <div class="card flex items-start gap-2 p-3">
          <div class="min-w-0 flex-1">
            <div class="truncate text-[13px] font-medium">{item.name}</div>
            {#if item.description}<div class="tiny muted">{item.description}</div>{/if}
            {#if item.version}<div class="tiny muted mono">v{item.version}</div>{/if}
          </div>
          <button class="btn btn-sm" disabled={item.enabled === undefined} on:click={() => extensionAction(tab, item.enabled ? 'disable' : 'enable', item.name)}>
            {item.enabled ? $t('disable') : $t('enable')}
          </button>
        </div>
      {:else}
        <div class="text-center text-[13px] muted">{$t('noFiles')}</div>
      {/each}
    {/if}
  </div>
</div>

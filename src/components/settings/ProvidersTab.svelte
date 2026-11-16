<script lang="ts">
import { slide, fade, scale } from 'svelte/transition';
import { flip } from 'svelte/animate';
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { settingsStore, type ApiProtocol, type DetectedModelInfo, type ProviderConfig } from '../../lib/stores/settings.svelte';
import Icon from '../Icon.svelte';

let showForm = $state(false);
let editingId = $state<string | null>(null);
let name = $state('');
let baseUrl = $state('');
let apiKeys = $state<string[]>([]);
let newKeyInput = $state('');
let revealedKeys = $state<Record<number, boolean>>({});
let keyTestStatus = $state<Record<number, { state: 'idle' | 'testing' | 'ok' | 'err'; count?: number; error?: string }>>({});
let apiProtocol = $state<ApiProtocol>('auto');
let proxyEnabled = $state(false);
let proxyUrl = $state('');
let modelTestStatus = $state<Record<string, { state: 'idle' | 'testing' | 'ok' | 'err'; latencyMs?: number; error?: string }>>({});

let fetchedModels = $state<DetectedModelInfo[]>([]);
let modelSearch = $state('');
let customModelId = $state('');
let isFetchingModels = $state(false);
let fetchError = $state<string | null>(null);
let pendingModels = $state<DetectedModelInfo[]>([]);
let cardModelInputs = $state<Record<string, string>>({});

let fetchTimer: ReturnType<typeof setTimeout> | null = null;

interface OAuthAccountItem {
  id: string;
  provider: string;
  email: string;
  projectId?: string | null;
  accountId?: string | null;
  enabled: boolean;
  expiresAtMs: number;
  cooldownUntilMs?: number | null;
  lastError?: string | null;
}

const OAUTH_PROVIDERS = [
  { id: 'antigravity', title: 'Antigravity (Google)', subtitle: 'Gemini, Claude & GPT-OSS via Google OAuth' },
  { id: 'openai_codex', title: 'ChatGPT Codex (OpenAI)', subtitle: 'Plus/Pro/Team subscription via OAuth' },
  { id: 'claude_oauth', title: 'Claude (Anthropic)', subtitle: 'Pro/Max subscription via Claude Code OAuth' }
] as const;

let oauthAccounts = $state<OAuthAccountItem[]>([]);
let oauthStatus = $state<string | null>(null);
let oauthManualInputs = $state<Record<string, string>>({});
let oauthPendingMode = $state<Record<string, string>>({});
let oauthBusy = $state<Record<string, boolean>>({});
let oauthProxyOpen = $state<boolean>(false);
let oauthProxyEnabled = $state<boolean>(typeof localStorage !== 'undefined' && localStorage.getItem('omni_oauth_proxy_enabled') === '1');
let oauthProxyUrl = $state<string>(typeof localStorage !== 'undefined' ? (localStorage.getItem('omni_oauth_proxy_url') || '') : '');

function saveOAuthProxy() {
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('omni_oauth_proxy_enabled', oauthProxyEnabled ? '1' : '0');
    localStorage.setItem('omni_oauth_proxy_url', oauthProxyUrl.trim());
  }
  for (const prov of Object.values(settingsStore.data.providers)) {
    if (prov.apiProtocol === 'antigravity' || prov.apiProtocol === 'openai_codex' || prov.apiProtocol === 'claude_oauth') {
      settingsStore.updateProvider(prov.id, {
        proxyEnabled: oauthProxyEnabled,
        proxyUrl: oauthProxyUrl.trim()
      });
    }
  }
  oauthStatus = oauthProxyEnabled && oauthProxyUrl.trim() ? `OAuth proxy saved: ${oauthProxyUrl.trim()}` : 'OAuth proxy disabled';
}

function getOAuthProxyParams(oauthProv?: string): { proxyEnabled: boolean; proxyUrl: string | null } {
  const existing = oauthProv
    ? Object.values(settingsStore.data.providers).find((p) => p.apiProtocol === oauthProv)
    : null;
  if (existing && existing.proxyEnabled && existing.proxyUrl?.trim()) {
    return { proxyEnabled: true, proxyUrl: existing.proxyUrl.trim() };
  }
  if (oauthProxyEnabled && oauthProxyUrl.trim()) {
    return { proxyEnabled: true, proxyUrl: oauthProxyUrl.trim() };
  }
  return { proxyEnabled: false, proxyUrl: null };
}

async function loadOAuthAccounts() {
  try {
    const res = await invoke<OAuthAccountItem[]>('oauth_list_accounts', { provider: null });
    if (Array.isArray(res)) {
      oauthAccounts = res;
    }
  } catch {}
}

onMount(() => {
  loadOAuthAccounts();
  let unlisten: (() => void) | null = null;
  listen<{ provider?: string }>('oauth-accounts-updated', async (ev) => {
    await loadOAuthAccounts();
    if (ev.payload?.provider) {
      await ensureOAuthProviderRegistered(ev.payload.provider);
    }
  }).then((fn) => (unlisten = fn)).catch(() => {});
  return () => {
    if (unlisten) unlisten();
  };
});

async function ensureOAuthProviderRegistered(oauthProv: string) {
  const existing = Object.values(settingsStore.data.providers).find((p) => p.apiProtocol === oauthProv);
  const px = getOAuthProxyParams(oauthProv);
  const cmdName =
    oauthProv === 'antigravity'
      ? 'oauth_fetch_antigravity_models'
      : oauthProv === 'openai_codex'
        ? 'oauth_fetch_codex_models'
        : 'oauth_fetch_claude_models';

  let models: DetectedModelInfo[] = [];
  try {
    const raw = await invoke<any[]>(cmdName, px);
    if (Array.isArray(raw)) {
      models = raw.map((m) => ({
        id: m.id,
        name: m.name || m.id,
        contextWindow: m.contextWindow || 200000,
        maxOutputTokens: m.maxOutputTokens || 32768,
        supportsReasoning: Boolean(m.supportsReasoning),
        supportsVision: m.supportsVision !== false,
        supportsTools: m.supportsTools !== false,
        supportsAudio: Boolean(m.supportsAudio),
        supportsFiles: true
      }));
    }
  } catch (err: any) {
    oauthStatus = err instanceof Error ? err.message : String(err);
  }

  if (models.length === 0) {
    const fallbackIds =
      oauthProv === 'antigravity'
        ? ['gemini-3.8-flash', 'gemini-4-argon', 'claude-opus-5-5', 'claude-sonnet-5-5', 'gpt-oss-120b']
        : oauthProv === 'openai_codex'
          ? ['gpt-6.1-sol', 'gpt-6-astra', 'gpt-6-sol', 'gpt-6.1-terra', 'gpt-6-luna', 'gpt-6-chat-thinking']
          : ['claude-opus-4-6', 'claude-sonnet-4-6', 'claude-haiku-4-5'];
    models = fallbackIds.map((id) => ({
      id,
      name: id,
      contextWindow: oauthProv === 'openai_codex' ? 524288 : 200000,
      maxOutputTokens: 65536,
      supportsReasoning: true,
      supportsVision: true,
      supportsTools: true,
      supportsAudio: id.includes('gemini'),
      supportsFiles: true
    }));
  }

  const capsMap: Record<string, DetectedModelInfo> = {};
  for (const m of models) capsMap[m.id] = m;

  if (existing) {
    settingsStore.updateProvider(existing.id, {
      availableModels: models.map((m) => m.id),
      modelCapabilities: capsMap,
      defaultModel: models.some((m) => m.id === existing.defaultModel) ? existing.defaultModel : models[0].id,
      proxyEnabled: px.proxyEnabled,
      proxyUrl: px.proxyUrl || ''
    });
  } else {
    const meta =
      oauthProv === 'antigravity'
        ? { id: 'antigravity-oauth', name: 'Antigravity', baseUrl: 'https://daily-cloudcode-pa.googleapis.com' }
        : oauthProv === 'openai_codex'
          ? { id: 'openai-codex-oauth', name: 'ChatGPT Codex', baseUrl: 'https://chatgpt.com/backend-api/codex' }
          : { id: 'claude-oauth', name: 'Claude OAuth', baseUrl: 'https://api.anthropic.com/v1' };
    settingsStore.addProvider({
      id: meta.id,
      name: meta.name,
      baseUrl: meta.baseUrl,
      apiKey: '',
      apiProtocol: oauthProv as ApiProtocol,
      proxyEnabled: px.proxyEnabled,
      proxyUrl: px.proxyUrl || '',
      defaultModel: models[0].id,
      availableModels: models.map((m) => m.id),
      modelCapabilities: capsMap,
      supportsReasoning: true,
      enabled: true
    });
  }
}

async function startOAuthLogin(provId: string) {
  oauthBusy = { ...oauthBusy, [provId]: true };
  oauthStatus = null;
  try {
    const res = await invoke<{ authUrl: string; mode: string }>('oauth_start_login', {
      provider: provId,
      ...getOAuthProxyParams(provId)
    });
    oauthPendingMode = { ...oauthPendingMode, [provId]: res.mode || 'manual' };
    oauthStatus = res.mode === 'callback'
      ? `Browser opened for ${provId}. Waiting for callback (or paste redirect URL below)...`
      : `Browser opened for ${provId}. Paste the authorization code or redirect URL below.`;
  } catch (err: any) {
    oauthStatus = err instanceof Error ? err.message : String(err);
  } finally {
    oauthBusy = { ...oauthBusy, [provId]: false };
  }
}

async function completeManualOAuth(provId: string) {
  const code = (oauthManualInputs[provId] || '').trim();
  if (!code) return;
  oauthBusy = { ...oauthBusy, [provId]: true };
  oauthStatus = null;
  try {
    const acc = await invoke<OAuthAccountItem>('oauth_complete_manual_login', {
      provider: provId,
      codeOrUrl: code,
      ...getOAuthProxyParams(provId)
    });
    oauthManualInputs = { ...oauthManualInputs, [provId]: '' };
    oauthPendingMode = { ...oauthPendingMode, [provId]: '' };
    await loadOAuthAccounts();
    await ensureOAuthProviderRegistered(provId);
    oauthStatus = `Connected ${acc.email} (${provId})`;
  } catch (err: any) {
    oauthStatus = err instanceof Error ? err.message : String(err);
  } finally {
    oauthBusy = { ...oauthBusy, [provId]: false };
  }
}

async function importLocalOAuth() {
  oauthBusy = { ...oauthBusy, import: true };
  oauthStatus = null;
  try {
    const res = await invoke<OAuthAccountItem[]>('oauth_import_local_accounts', getOAuthProxyParams());
    await loadOAuthAccounts();
    const provs = new Set(res.map((a) => a.provider));
    for (const p of provs) {
      await ensureOAuthProviderRegistered(p);
    }
    oauthStatus = res.length > 0 ? `Imported ${res.length} local account(s)` : 'No local Antigravity/Codex accounts found on this PC';
  } catch (err: any) {
    oauthStatus = err instanceof Error ? err.message : String(err);
  } finally {
    oauthBusy = { ...oauthBusy, import: false };
  }
}

async function toggleOAuthAcc(acc: OAuthAccountItem) {
  try {
    await invoke('oauth_toggle_account', { id: acc.id, enabled: !acc.enabled });
    await loadOAuthAccounts();
  } catch {}
}

async function deleteOAuthAcc(acc: OAuthAccountItem) {
  try {
    await invoke('oauth_delete_account', { id: acc.id });
    await loadOAuthAccounts();
  } catch {}
}

let providersList = $derived(Object.values(settingsStore.data.providers));

let filteredFetchedModels = $derived.by(() => {
  const q = modelSearch.trim().toLowerCase();
  if (!q) return fetchedModels;
  return fetchedModels.filter((m) => m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q));
});

function openCreateForm() {
  editingId = null;
  name = '';
  baseUrl = '';
  apiKeys = [];
  newKeyInput = '';
  revealedKeys = {};
  keyTestStatus = {};
  apiProtocol = 'auto';
  proxyEnabled = false;
  proxyUrl = '';
  fetchedModels = [];
  pendingModels = [];
  modelSearch = '';
  fetchError = null;
  showForm = true;
}

function openEditForm(prov: ProviderConfig) {
  editingId = prov.id;
  name = prov.name;
  baseUrl = prov.baseUrl;
  apiKeys = (prov.apiKey || '')
    .split(/[\n,;]+/)
    .map((k) => k.trim())
    .filter(Boolean);
  newKeyInput = '';
  revealedKeys = {};
  keyTestStatus = {};
  apiProtocol = prov.apiProtocol || 'auto';
  proxyEnabled = Boolean(prov.proxyEnabled);
  proxyUrl = prov.proxyUrl || '';
  fetchedModels = [];
  modelSearch = '';
  fetchError = null;
  pendingModels = prov.availableModels.map((id) => {
    const cap = prov.modelCapabilities?.[id];
    if (cap) return cap;
    return {
      id,
      name: id,
      contextWindow: 128000,
      maxOutputTokens: 16384,
      supportsReasoning: id.toLowerCase().includes('r1') || id.toLowerCase().includes('reason'),
      supportsVision: false,
      supportsTools: true,
      supportsAudio: false,
      supportsFiles: false
    };
  });
  showForm = true;
  if (prov.baseUrl.trim().startsWith('http')) {
    discoverModels();
  }
}

function maskKey(k: string): string {
  if (k.length <= 10) return '••••••••';
  return `${k.slice(0, 5)}••••${k.slice(-4)}`;
}

function addApiKeyToList() {
  const k = newKeyInput.trim();
  if (!k) return;
  if (!apiKeys.includes(k)) {
    apiKeys = [...apiKeys, k];
    if (apiKeys.length === 1 && baseUrl.trim().startsWith('http')) {
      discoverModels();
    }
  }
  newKeyInput = '';
}

function removeApiKeyFromList(idx: number) {
  apiKeys = apiKeys.filter((_, i) => i !== idx);
  const nextRevealed: Record<number, boolean> = {};
  const nextTests: typeof keyTestStatus = {};
  for (const [k, v] of Object.entries(revealedKeys)) {
    const n = Number(k);
    if (n < idx) nextRevealed[n] = v;
    else if (n > idx) nextRevealed[n - 1] = v;
  }
  for (const [k, v] of Object.entries(keyTestStatus)) {
    const n = Number(k);
    if (n < idx) nextTests[n] = v;
    else if (n > idx) nextTests[n - 1] = v;
  }
  revealedKeys = nextRevealed;
  keyTestStatus = nextTests;
}

async function testSingleApiKey(idx: number) {
  const url = baseUrl.trim();
  const key = apiKeys[idx];
  if (!url || !key) return;
  keyTestStatus = { ...keyTestStatus, [idx]: { state: 'testing' } };
  try {
    const res = await invoke<DetectedModelInfo[]>('fetch_provider_models', {
      baseUrl: url,
      apiKey: key,
      proxyEnabled,
      proxyUrl: proxyUrl.trim() || null
    });
    keyTestStatus = { ...keyTestStatus, [idx]: { state: 'ok', count: Array.isArray(res) ? res.length : 0 } };
  } catch (err: unknown) {
    keyTestStatus = {
      ...keyTestStatus,
      [idx]: { state: 'err', error: err instanceof Error ? err.message : String(err) }
    };
  }
}

function closeForm() {
  showForm = false;
  editingId = null;
  name = '';
  baseUrl = '';
  apiKeys = [];
  newKeyInput = '';
  revealedKeys = {};
  keyTestStatus = {};
  apiProtocol = 'auto';
  proxyEnabled = false;
  proxyUrl = '';
  fetchedModels = [];
  pendingModels = [];
  modelSearch = '';
  fetchError = null;
}

function scheduleDiscovery() {
  if (fetchTimer) clearTimeout(fetchTimer);
  if (!baseUrl.trim() || !baseUrl.trim().startsWith('http')) return;
  fetchTimer = setTimeout(() => {
    discoverModels();
  }, 500);
}

async function discoverModels() {
  const url = baseUrl.trim();
  if (!url) return;
  isFetchingModels = true;
  fetchError = null;
  try {
    const activeKey = apiKeys[0] || newKeyInput.trim();
    const res = await invoke<DetectedModelInfo[]>('fetch_provider_models', {
      baseUrl: url,
      apiKey: activeKey,
      proxyEnabled,
      proxyUrl: proxyUrl.trim() || null
    });
    fetchedModels = Array.isArray(res) ? res : [];
    if (fetchedModels.length > 0 && pendingModels.length > 0) {
      pendingModels = pendingModels.map((pm) => {
        const fresh = fetchedModels.find((f) => f.id === pm.id);
        return fresh || pm;
      });
    }
  } catch (err: unknown) {
    fetchError = err instanceof Error ? err.message : String(err);
    fetchedModels = [];
  } finally {
    isFetchingModels = false;
  }
}

async function testModel(prov: ProviderConfig, modelId: string) {
  const key = `${prov.id}:${modelId}`;
  modelTestStatus = { ...modelTestStatus, [key]: { state: 'testing' } };
  try {
    const firstKey = (prov.apiKey || '').split(/[\n,;]+/).map((k) => k.trim()).find(Boolean) || '';
    const res = await invoke<{ ok: boolean; latencyMs: number; error?: string }>('test_provider_model', {
      baseUrl: prov.baseUrl,
      apiKey: firstKey,
      modelId,
      apiProtocol: prov.apiProtocol || 'auto',
      proxyEnabled: Boolean(prov.proxyEnabled),
      proxyUrl: prov.proxyUrl || null
    });
    modelTestStatus = {
      ...modelTestStatus,
      [key]: { state: res.ok ? 'ok' : 'err', latencyMs: res.latencyMs, error: res.error }
    };
  } catch (err: any) {
    modelTestStatus = {
      ...modelTestStatus,
      [key]: { state: 'err', error: err instanceof Error ? err.message : String(err) }
    };
  }
}

function formatCtx(ctx: number): string {
  if (ctx >= 1000000) return `${Math.round(ctx / 1000000)}M`;
  return `${Math.round(ctx / 1000)}k`;
}

function addPendingModel(model: DetectedModelInfo) {
  if (!pendingModels.some((m) => m.id === model.id)) {
    pendingModels = [...pendingModels, model];
  }
}

function removePendingModel(id: string) {
  pendingModels = pendingModels.filter((m) => m.id !== id);
}

async function addCustomModel() {
  const id = customModelId.trim();
  if (!id) return;
  try {
    const detected = await invoke<DetectedModelInfo>('detect_model_capabilities', { modelId: id });
    addPendingModel(detected);
  } catch {
    addPendingModel({
      id,
      name: id,
      contextWindow: 128000,
      maxOutputTokens: 16384,
      supportsReasoning: id.toLowerCase().includes('r1') || id.toLowerCase().includes('reason'),
      supportsVision: false,
      supportsTools: true,
      supportsAudio: false,
      supportsFiles: false
    });
  }
  customModelId = '';
}

async function addCustomModelToExisting(providerId: string) {
  const id = (cardModelInputs[providerId] || '').trim();
  if (!id) return;
  try {
    const detected = await invoke<DetectedModelInfo>('detect_model_capabilities', { modelId: id });
    settingsStore.addModelToProvider(providerId, detected);
  } catch {
    settingsStore.addModelToProvider(providerId, {
      id,
      name: id,
      contextWindow: 128000,
      maxOutputTokens: 16384,
      supportsReasoning: false,
      supportsVision: false,
      supportsTools: true,
      supportsAudio: false,
      supportsFiles: false
    });
  }
  cardModelInputs[providerId] = '';
}

function handleSaveProvider(e: Event) {
  e.preventDefault();
  const cleanName = name.trim();
  const cleanUrl = baseUrl.trim();
  if (!cleanName || !cleanUrl || pendingModels.length === 0) return;

  const finalKeys = [...apiKeys];
  const pendingInput = newKeyInput.trim();
  if (pendingInput && !finalKeys.includes(pendingInput)) {
    finalKeys.push(pendingInput);
  }
  const joinedKeys = finalKeys.join('\n');

  const capsMap: Record<string, DetectedModelInfo> = {};
  for (const m of pendingModels) {
    capsMap[m.id] = m;
  }

  if (editingId) {
    settingsStore.updateProvider(editingId, {
      name: cleanName,
      baseUrl: cleanUrl,
      apiKey: joinedKeys,
      apiProtocol,
      proxyEnabled,
      proxyUrl: proxyUrl.trim(),
      defaultModel: pendingModels[0].id,
      availableModels: pendingModels.map((m) => m.id),
      modelCapabilities: capsMap,
      supportsReasoning: pendingModels.some((m) => m.supportsReasoning)
    });
  } else {
    const id = `${cleanName.toLowerCase().replace(/[^a-z0-9]+/g, '-')}-${Date.now().toString(36)}`;
    settingsStore.addProvider({
      id,
      name: cleanName,
      baseUrl: cleanUrl,
      apiKey: joinedKeys,
      apiProtocol,
      proxyEnabled,
      proxyUrl: proxyUrl.trim(),
      defaultModel: pendingModels[0].id,
      availableModels: pendingModels.map((m) => m.id),
      modelCapabilities: capsMap,
      supportsReasoning: pendingModels.some((m) => m.supportsReasoning),
      enabled: true
    });
  }

  closeForm();
}
</script>

<div class="space-y-5 text-[13px] text-[#E5E5E8]">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h2 class="text-sm font-medium text-[#EDEDEF]">Models & Providers</h2>
      <p class="text-xs text-[#73737A] mt-0.5">Connect API keys or subscription OAuth accounts.</p>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        onclick={() => (oauthProxyOpen = !oauthProxyOpen)}
        class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border text-xs transition-all whitespace-nowrap {oauthProxyEnabled && oauthProxyUrl.trim() ? 'border-[#8B5CF6]/50 text-[#D8B4FE]' : 'border-white/[0.08] text-[#A3A3AA]'}"
      >
        {oauthProxyEnabled && oauthProxyUrl.trim() ? 'Proxy: ON' : 'OAuth Proxy'}
      </button>
      <button
        type="button"
        disabled={oauthBusy.import}
        onclick={importLocalOAuth}
        class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#D4D4D8] active:scale-[0.97] transition-all whitespace-nowrap disabled:opacity-50"
        title="Import existing Antigravity, Codex, or Claude credentials from this PC"
      >
        {oauthBusy.import ? 'Importing...' : 'Import from PC'}
      </button>

      {#if providersList.length > 0 || showForm}
        <button
          type="button"
          class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs font-medium text-[#E5E5E8] flex items-center gap-1.5 active:scale-[0.97] transition-all whitespace-nowrap"
          onclick={() => (showForm ? closeForm() : openCreateForm())}
        >
          {#if showForm}
            <Icon name="close" class="w-3.5 h-3.5 text-[#A3A3AA]" strokeWidth={1.8} />
            <span>Cancel</span>
          {:else}
            <Icon name="plus" class="w-3.5 h-3.5 text-[#D4D4D8]" strokeWidth={1.8} />
            <span>Add Provider</span>
          {/if}
        </button>
      {/if}
    </div>
  </div>

  {#if oauthProxyOpen}
    <div class="p-3 rounded-[6px] bg-[#18181B]/70 border border-white/[0.08] flex items-center gap-2.5">
      <label class="flex items-center gap-1.5 text-xs text-[#D4D4D8] shrink-0 cursor-pointer">
        <input type="checkbox" bind:checked={oauthProxyEnabled} class="accent-[#8B5CF6]" />
        <span>Enable Proxy</span>
      </label>
      <input
        type="text"
        bind:value={oauthProxyUrl}
        placeholder="http://127.0.0.1:7890 or socks5://127.0.0.1:1080"
        class="flex-1 h-7 px-2.5 rounded-[5px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#E5E5E8] placeholder-[#5C5C64] outline-none"
      />
      <button
        type="button"
        onclick={saveOAuthProxy}
        class="h-7 px-3 rounded-[5px] bg-[#27272A] hover:bg-[#323236] border border-white/[0.08] text-[11px] text-[#EDEDEF] shrink-0"
      >
        Apply
      </button>
    </div>
  {/if}

  <div class="rounded-[6px] bg-[#18181B]/70 border border-white/[0.07] divide-y divide-white/[0.06]">
    {#if oauthStatus}
      <div class="px-3.5 py-2 bg-[#121214] text-[11px] text-[#A3A3AA]">
        {oauthStatus}
      </div>
    {/if}

    {#each OAUTH_PROVIDERS as op}
      {@const accs = oauthAccounts.filter((a) => a.provider === op.id)}
      <div class="px-3.5 py-2.5 space-y-2">
        <div class="flex items-center justify-between gap-3">
          <div class="flex items-center gap-2.5 min-w-0">
            <span class="text-xs font-medium text-[#E5E5E8] truncate">{op.title}</span>
            <span class="text-[11px] text-[#686870] truncate hidden sm:inline">{op.subtitle}</span>
            {#if accs.length > 0}
              <span class="px-1.5 py-0.5 rounded-[4px] bg-white/[0.05] font-mono text-[10px] text-[#A3A3AA] shrink-0">{accs.length}</span>
            {/if}
          </div>
          <div class="flex items-center gap-1.5 shrink-0">
            {#if accs.length > 0}
              <button
                type="button"
                onclick={() => ensureOAuthProviderRegistered(op.id)}
                class="h-7 px-2.5 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.07] text-[11px] text-[#D4D4D8] transition-colors"
              >
                Sync Models
              </button>
            {/if}
            <button
              type="button"
              onclick={() => (oauthPendingMode = { ...oauthPendingMode, [op.id]: oauthPendingMode[op.id] ? '' : 'manual' })}
              class="h-7 px-2 rounded-[5px] bg-transparent hover:bg-white/[0.05] border border-white/[0.06] text-[11px] text-[#8E8E95] hover:text-[#D4D4D8] transition-colors"
              title="Paste callback URL or code manually"
            >
              Code
            </button>
            <button
              type="button"
              disabled={oauthBusy[op.id]}
              onclick={() => startOAuthLogin(op.id)}
              class="h-7 px-2.5 rounded-[5px] bg-[#27272A] hover:bg-[#323236] border border-white/[0.08] text-[11px] text-[#EDEDEF] transition-colors disabled:opacity-50"
            >
              {oauthBusy[op.id] ? 'Opening...' : '+ Connect'}
            </button>
          </div>
        </div>

        {#if oauthPendingMode[op.id]}
          <div class="flex items-center gap-1.5">
            <input
              type="text"
              bind:value={oauthManualInputs[op.id]}
              placeholder="Paste redirect URL or authorization code..."
              class="flex-1 h-7 px-2.5 rounded-[5px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#E5E5E8] placeholder-[#5C5C64] outline-none focus:border-white/[0.2]"
            />
            <button
              type="button"
              disabled={oauthBusy[op.id] || !(oauthManualInputs[op.id] || '').trim()}
              onclick={() => completeManualOAuth(op.id)}
              class="h-7 px-2.5 rounded-[5px] bg-[#27272A] hover:bg-[#323236] border border-white/[0.08] text-[11px] text-[#EDEDEF] disabled:opacity-40"
            >
              Verify
            </button>
          </div>
        {/if}

        {#if accs.length > 0}
          <div class="space-y-1">
            {#each accs as acc (acc.id)}
              <div class="flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[5px] bg-[#121214] border border-white/[0.05] text-[11px]">
                <div class="flex items-center gap-2 min-w-0">
                  <span class="w-1.5 h-1.5 rounded-full shrink-0 {acc.enabled ? (acc.cooldownUntilMs && acc.cooldownUntilMs > Date.now() ? 'bg-[#F59E0B]' : 'bg-[#22C55E]') : 'bg-[#52525B]'}"></span>
                  <span class="font-mono text-[#D4D4D8] truncate">{acc.email}</span>
                  {#if acc.projectId}
                    <span class="font-mono text-[10px] text-[#686870] truncate">{acc.projectId}</span>
                  {/if}
                  {#if acc.lastError}
                    <span class="text-[10px] text-[#F87171] truncate max-w-[180px]" title={acc.lastError}>{acc.lastError}</span>
                  {/if}
                </div>
                <div class="flex items-center gap-1 shrink-0">
                  <button
                    type="button"
                    onclick={() => toggleOAuthAcc(acc)}
                    class="px-2 py-0.5 rounded bg-white/[0.04] hover:bg-white/[0.08] text-[10px] text-[#A3A3AA]"
                  >
                    {acc.enabled ? 'Disable' : 'Enable'}
                  </button>
                  <button
                    type="button"
                    onclick={() => deleteOAuthAcc(acc)}
                    class="px-2 py-0.5 rounded bg-white/[0.04] hover:bg-[#EF4444]/15 text-[10px] text-[#73737A] hover:text-[#FCA5A5]"
                  >
                    Remove
                  </button>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>

{#snippet providerForm()}
  <form
    onsubmit={handleSaveProvider}
    class="p-5 rounded-[6px] bg-[#1C1C1F] border border-white/[0.12] space-y-4"
    in:fade={{ duration: 150 }}
  >
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <label class="block space-y-1.5">
            <span class="text-xs text-[#A3A3AA]">Provider Name</span>
            <input
              bind:value={name}
              type="text"
              placeholder="OpenRouter / Local"
              required
              class="w-full px-3 py-2 rounded-[6px] bg-[#121214] border border-white/[0.08] text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none focus:border-white/[0.2]"
            />
          </label>

          <label class="block space-y-1.5">
            <span class="text-xs text-[#A3A3AA]">Base URL</span>
            <input
              bind:value={baseUrl}
              oninput={scheduleDiscovery}
              type="url"
              placeholder="https://api.example.com/v1"
              required
              class="w-full px-3 py-2 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none focus:border-white/[0.2]"
            />
          </label>
        </div>

        <div class="space-y-2">
          <span class="text-xs text-[#A3A3AA] block">API Keys ({apiKeys.length})</span>
          <div class="flex items-center gap-2">
            <input
              bind:value={newKeyInput}
              type="password"
              placeholder="Paste API key (sk-...)"
              onkeydown={(e) => {
                if (e.key === 'Enter') {
                  e.preventDefault();
                  addApiKeyToList();
                }
              }}
              class="flex-1 px-3 py-2 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none focus:border-white/[0.2]"
            />
            <button
              type="button"
              onclick={addApiKeyToList}
              class="h-8 px-3 rounded-[6px] bg-[#27272A] hover:bg-[#323236] border border-white/[0.08] text-xs font-medium text-[#EDEDEF] flex items-center gap-1 transition-colors shrink-0"
              title="Add key to rotation pool"
            >
              <Icon name="plus" class="w-3.5 h-3.5" strokeWidth={1.8} />
              <span>Add</span>
            </button>
          </div>

          {#if apiKeys.length > 0}
            <div class="space-y-1.5 max-h-40 overflow-y-auto pr-1">
              {#each apiKeys as key, idx (idx)}
                {@const tStatus = keyTestStatus[idx]}
                <div class="flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.06]">
                  <div class="flex items-center gap-2 min-w-0 flex-1">
                    <span class="font-mono text-[10px] text-[#66666D] shrink-0">#{idx + 1}</span>
                    <span class="font-mono text-[11px] text-[#E5E5E8] truncate select-all">
                      {revealedKeys[idx] ? key : maskKey(key)}
                    </span>
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    {#if tStatus?.state === 'testing'}
                      <span class="font-mono text-[10px] text-[#A3A3AA] px-1">testing...</span>
                    {:else if tStatus?.state === 'ok'}
                      <span class="font-mono text-[10px] text-[#4ADE80] px-1" title="Key is valid">OK ({tStatus.count})</span>
                    {:else if tStatus?.state === 'err'}
                      <span class="font-mono text-[10px] text-[#F87171] px-1" title={tStatus.error || 'Failed'}>ERR</span>
                    {/if}
                    <button
                      type="button"
                      onclick={() => testSingleApiKey(idx)}
                      class="px-2 py-0.5 rounded-[4px] bg-white/[0.05] hover:bg-white/[0.1] text-[10.5px] text-[#D4D4D8] transition-colors"
                      title="Test this key"
                    >
                      Test
                    </button>
                    <button
                      type="button"
                      onclick={() => (revealedKeys = { ...revealedKeys, [idx]: !revealedKeys[idx] })}
                      class="px-2 py-0.5 rounded-[4px] bg-white/[0.05] hover:bg-white/[0.1] text-[10.5px] text-[#A3A3AA] hover:text-white transition-colors"
                      title={revealedKeys[idx] ? 'Hide key' : 'Show key'}
                    >
                      {revealedKeys[idx] ? 'Hide' : 'Show'}
                    </button>
                    <button
                      type="button"
                      onclick={() => removeApiKeyFromList(idx)}
                      class="p-1 rounded-[4px] text-[#73737A] hover:text-[#F87171] hover:bg-white/[0.05] transition-colors"
                      title="Remove key"
                    >
                      <Icon name="close" class="w-3 h-3" strokeWidth={1.8} />
                    </button>
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>

        <div class="space-y-1.5">
          <span class="text-xs text-[#A3A3AA] block">API Protocol</span>
          <div class="grid grid-cols-4 gap-1 p-1 rounded-[6px] bg-[#121214] border border-white/[0.06]">
            {#each [['auto', 'Auto'], ['chat_completions', 'Chat'], ['responses', 'Responses'], ['anthropic_messages', 'Anthropic'], ['google_gemini', 'Google AI'], ['antigravity', 'Antigravity'], ['openai_codex', 'Codex OAuth'], ['claude_oauth', 'Claude OAuth']] as [val, label]}
              <button
                type="button"
                class="py-1.5 px-2 rounded-[4px] font-mono text-[11px] transition-colors {apiProtocol === val ? 'bg-white/[0.12] text-[#EDEDEF] font-medium' : 'text-[#73737A] hover:text-[#D4D4D8]'}"
                onclick={() => (apiProtocol = val as ApiProtocol)}
              >
                {label}
              </button>
            {/each}
          </div>
        </div>

        <div class="space-y-2">
          <div class="flex items-center justify-between">
            <span class="text-xs text-[#A3A3AA]">Enable Proxy (HTTP / HTTPS / SOCKS5 / SOCKS5h)</span>
            <button
              type="button"
              class="w-9 h-5 rounded-full p-0.5 transition-colors {proxyEnabled ? 'bg-[#EDEDEF]' : 'bg-[#27272A]'}"
              onclick={() => (proxyEnabled = !proxyEnabled)}
              aria-label="Enable Proxy"
            >
              <div class="w-4 h-4 rounded-full transition-transform {proxyEnabled ? 'translate-x-4 bg-[#111113]' : 'translate-x-0 bg-[#73737A]'}"></div>
            </button>
          </div>
          {#if proxyEnabled}
            <input
              bind:value={proxyUrl}
              type="text"
              placeholder="socks5h://127.0.0.1:1080 or http://user:pass@host:port"
              class="w-full px-3 py-2 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none focus:border-white/[0.2]"
            />
          {/if}
        </div>

        <div class="space-y-2.5 pt-1 border-t border-white/[0.06]">
          <div class="flex items-center justify-between gap-2">
            <span class="font-mono text-[10.5px] text-[#A3A3AA] uppercase tracking-wider">
              Endpoint Discovery (/models)
            </span>
            <button
              type="button"
              class="h-7 px-2.5 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] font-mono text-[11px] text-[#D4D4D8] active:scale-[0.97] transition-all"
              onclick={discoverModels}
              disabled={isFetchingModels || !baseUrl.trim()}
            >
              {isFetchingModels ? 'Fetching...' : 'Fetch /models'}
            </button>
          </div>

          {#if fetchError}
            <div class="text-[11px] font-mono text-[#A3A3AA] bg-[#121214] border border-white/[0.06] rounded-[6px] px-3 py-2">
              {fetchError} — add custom Model ID below.
            </div>
          {/if}

          {#if fetchedModels.length > 0}
            <div class="space-y-2">
              <input
                bind:value={modelSearch}
                type="text"
                placeholder="Search discovered models ({fetchedModels.length})..."
                class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none"
              />
              <div class="max-h-48 overflow-y-auto rounded-[6px] bg-[#121214] border border-white/[0.06] divide-y divide-white/[0.04]">
                {#each filteredFetchedModels as m (m.id)}
                  <div class="px-3 py-1.5 flex items-center justify-between gap-2 hover:bg-white/[0.03]">
                    <div class="min-w-0 flex items-center gap-2">
                      <span class="font-mono text-[11.5px] text-[#E5E5E8] truncate">{m.id}</span>
                      <span class="font-mono text-[10px] text-[#73737A]">{formatCtx(m.contextWindow)}</span>
                      {#if m.supportsReasoning}
                        <span class="px-1.5 py-0.5 rounded-[4px] bg-[#1E1E22] font-mono text-[9.5px] text-[#A3A3AA]">Reasoning</span>
                      {/if}
                      {#if m.supportsVision}
                        <span class="px-1.5 py-0.5 rounded-[4px] bg-[#1E1E22] font-mono text-[9.5px] text-[#A3A3AA]">Vision</span>
                      {/if}
                    </div>
                    <button
                      type="button"
                      class="w-6 h-6 rounded-[4px] bg-[#1E1E22] hover:bg-[#EDEDEF] hover:text-[#111113] border border-white/[0.08] flex items-center justify-center text-xs shrink-0 active:scale-[0.95] transition-all"
                      onclick={() => addPendingModel(m)}
                      title="Add model"
                    >
                      +
                    </button>
                  </div>
                {/each}
              </div>
            </div>
          {/if}

          <div class="flex items-center gap-2">
            <input
              bind:value={customModelId}
              type="text"
              placeholder="Custom Model ID (if /models is restricted)"
              class="flex-1 px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none"
              onkeydown={(e) => { if (e.key === 'Enter') { e.preventDefault(); addCustomModel(); } }}
            />
            <button
              type="button"
              class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#E5E5E8] active:scale-[0.97] transition-all"
              onclick={addCustomModel}
            >
              + Add
            </button>
          </div>
        </div>

        {#if pendingModels.length > 0}
          <div class="space-y-1.5 pt-1">
            <div class="font-mono text-[10px] text-[#73737A] uppercase tracking-wider">Selected Models ({pendingModels.length})</div>
            <div class="flex flex-wrap gap-1.5">
              {#each pendingModels as pm (pm.id)}
                <div
                  animate:flip={{ duration: 160 }}
                  in:scale={{ start: 0.9, duration: 150 }}
                  class="px-2.5 py-1 rounded-[6px] bg-[#121214] border border-white/[0.08] flex items-center gap-2 text-[11px]"
                >
                  <span class="font-mono text-[#EDEDEF]">{pm.id}</span>
                  <span class="font-mono text-[10px] text-[#73737A]">{formatCtx(pm.contextWindow)}</span>
                  {#if pm.supportsReasoning}
                    <span class="font-mono text-[10px] text-[#A3A3AA]">Reasoning</span>
                  {/if}
                  {#if pm.supportsVision}
                    <span class="font-mono text-[10px] text-[#A3A3AA]">Vision</span>
                  {/if}
                  <button
                    type="button"
                    class="text-[#73737A] hover:text-[#EF4444] ml-0.5"
                    onclick={() => removePendingModel(pm.id)}
                  >
                    ×
                  </button>
                </div>
              {/each}
            </div>
          </div>
        {/if}

        <div class="flex justify-end gap-2 pt-1">
          <button
            type="button"
            onclick={closeForm}
            class="h-8 px-3 rounded-[6px] bg-[#121214] hover:bg-[#242428] border border-white/[0.08] text-xs text-[#A3A3AA] hover:text-white active:scale-[0.97] transition-all"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={pendingModels.length === 0}
            class="h-8 px-4 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium disabled:opacity-40 active:scale-[0.97] transition-all"
          >
            {editingId ? 'Save Changes' : 'Save Provider'}
          </button>
        </div>
      </form>
{/snippet}

  {#if showForm && !editingId}
    <div transition:slide={{ duration: 180 }}>
      {@render providerForm()}
    </div>
  {:else if providersList.length === 0}
    <div
      transition:slide={{ duration: 180 }}
      class="p-10 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] text-center space-y-3"
    >
      <div class="text-sm text-[#E5E5E8] font-medium">No providers configured</div>
      <p class="text-xs text-[#73737A] max-w-md mx-auto leading-relaxed">
        Add an API endpoint to auto-discover models and detect capabilities.
      </p>
      <button
        type="button"
        class="mt-2 h-8 px-4 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium active:scale-[0.97] transition-all"
        onclick={openCreateForm}
      >
        Add Provider
      </button>
    </div>
  {/if}

  {#if providersList.length > 0}
    <div class="space-y-3">
      {#each providersList as prov (prov.id)}
        {#if showForm && editingId === prov.id}
          <div transition:slide={{ duration: 180 }}>
            {@render providerForm()}
          </div>
        {:else}
        <div transition:slide={{ duration: 160 }} class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] space-y-3">
          <div class="flex items-center justify-between gap-3">
            <div class="flex items-center gap-3 min-w-0">
              <span class="font-medium text-[#EDEDEF] truncate">{prov.name}</span>
              <span class="px-1.5 py-0.5 rounded-[4px] bg-[#121214] border border-white/[0.06] font-mono text-[10px] text-[#8E8E95] shrink-0">{prov.apiProtocol || 'auto'}</span>
              <span class="font-mono text-[11px] text-[#73737A] truncate">{prov.baseUrl}</span>
            </div>

            <div class="flex items-center gap-1 shrink-0">
              <button
                type="button"
                class="p-1.5 rounded-[6px] text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
                onclick={() => openEditForm(prov)}
                title="Edit provider & add from /models"
              >
                <Icon name="edit" class="w-3.5 h-3.5" strokeWidth={1.6} />
              </button>
              <button
                type="button"
                class="p-1.5 rounded-[6px] text-[#73737A] hover:text-[#EF4444] hover:bg-white/[0.06] transition-colors"
                onclick={() => settingsStore.removeProvider(prov.id)}
                title="Remove provider"
              >
                <Icon name="trash" class="w-3.5 h-3.5" strokeWidth={1.6} />
              </button>
            </div>
          </div>

          <div class="space-y-2.5 pt-1">
            <label class="block space-y-1">
              <span class="text-[11px] text-[#73737A]">API Key</span>
              <input
                type="password"
                value={prov.apiKey}
                oninput={(e) => settingsStore.setApiKey(prov.id, (e.currentTarget as HTMLInputElement).value)}
                placeholder="Enter API key"
                class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] outline-none"
              />
            </label>

            <div class="space-y-1.5">
              <span class="text-[11px] text-[#73737A] block">Enabled Models</span>
              <div class="flex flex-wrap gap-1.5">
                {#each prov.availableModels as m (m)}
                  {@const cap = prov.modelCapabilities?.[m]}
                  {@const tStatus = modelTestStatus[`${prov.id}:${m}`]}
                  <div
                    animate:flip={{ duration: 160 }}
                    in:scale={{ start: 0.9, duration: 150 }}
                    class="flex items-center rounded-[6px] border bg-[#121214] text-[#D4D4D8] border-white/[0.08]"
                  >
                    <button
                      type="button"
                      class="pl-2 pr-0.5 py-1 flex items-center justify-center transition-colors"
                      onclick={() => testModel(prov, m)}
                      title={tStatus?.state === 'ok' ? `Available (${tStatus.latencyMs}ms)` : tStatus?.state === 'err' ? `Unavailable: ${tStatus.error || 'Error'}` : 'Test model availability'}
                    >
                      <span
                        class="w-2 h-2 rounded-full transition-colors {tStatus?.state === 'ok' ? 'bg-[#4ADE80]' : tStatus?.state === 'err' ? 'bg-[#F87171]' : tStatus?.state === 'testing' ? 'bg-[#EDEDEF] animate-pulse' : 'bg-[#52525B] hover:bg-[#A3A3AA]'}"
                      ></span>
                    </button>
                    <div class="pl-1.5 pr-2 py-1 font-mono text-[11px] flex items-center gap-1.5">
                      <span>{m}</span>
                      {#if cap}
                        <span class="text-[9.5px] text-[#73737A]">{formatCtx(cap.contextWindow)}</span>
                        {#if cap.supportsReasoning}
                          <span class="text-[9.5px] text-[#8E8E95]">Reasoning</span>
                        {/if}
                        {#if cap.supportsVision}
                          <span class="text-[9.5px] text-[#8E8E95]">Vision</span>
                        {/if}
                      {/if}
                    </div>
                    <button
                      type="button"
                      class="pr-2 pl-1 text-xs text-[#73737A] hover:text-[#EF4444] transition-colors"
                      onclick={() => settingsStore.removeModelFromProvider(prov.id, m)}
                      title="Remove model"
                    >
                      ×
                    </button>
                  </div>
                {/each}
              </div>

              <div class="flex items-center gap-1.5 pt-1">
                <input
                  type="text"
                  bind:value={cardModelInputs[prov.id]}
                  placeholder="Add model ID..."
                  class="flex-1 px-2.5 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#E5E5E8] placeholder-[#55555C] outline-none"
                  onkeydown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      addCustomModelToExisting(prov.id);
                    }
                  }}
                />
                <button
                  type="button"
                  class="w-7 h-7 rounded-[6px] bg-[#121214] hover:bg-[#EDEDEF] hover:text-[#111113] border border-white/[0.08] flex items-center justify-center text-sm text-[#D4D4D8] shrink-0 active:scale-[0.95] transition-all"
                  onclick={() => addCustomModelToExisting(prov.id)}
                  title="Add model"
                >
                  +
                </button>
              </div>
            </div>
          </div>
        </div>
        {/if}
      {/each}
    </div>
  {/if}
</div>
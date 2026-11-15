<script lang="ts">
import { slide, fade, scale } from 'svelte/transition';
import { flip } from 'svelte/animate';
import { invoke } from '@tauri-apps/api/core';
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
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-sm font-medium text-[#EDEDEF]">Models & Providers</h2>
      <p class="text-xs text-[#73737A] mt-0.5">Auto-discover models via /models and detect capabilities.</p>
    </div>

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

  {#if showForm}
    <div transition:slide={{ duration: 180 }}>
      <form
        onsubmit={handleSaveProvider}
        class="p-5 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] space-y-4"
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
          <div class="grid grid-cols-5 gap-1 p-1 rounded-[6px] bg-[#121214] border border-white/[0.06]">
            {#each [['auto', 'Auto'], ['chat_completions', 'Chat'], ['responses', 'Responses'], ['anthropic_messages', 'Anthropic'], ['google_gemini', 'Google AI']] as [val, label]}
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

        <div class="flex justify-end pt-1">
          <button
            type="submit"
            disabled={pendingModels.length === 0}
            class="h-8 px-4 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium disabled:opacity-40 active:scale-[0.97] transition-all"
          >
            {editingId ? 'Save Changes' : 'Save Provider'}
          </button>
        </div>
      </form>
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
        <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] space-y-3">
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
      {/each}
    </div>
  {/if}
</div>
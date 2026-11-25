<script lang="ts">
import { slide, fade } from 'svelte/transition';
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { settingsStore, groupProviderModels, isNonReasoningModelId, type ApiProtocol, type DetectedModelInfo, type ProviderConfig, type ReasoningLevel } from '../../lib/stores/settings.svelte';
import Icon from '../Icon.svelte';

interface ApiKeyEntry {
  id: string;
  value: string;
}

let showForm = $state(false);
let editingId = $state<string | null>(null);
let name = $state('');
let baseUrl = $state('');
let apiKeys = $state<ApiKeyEntry[]>([]);
let newKeyInput = $state('');
let revealedKeys = $state<Record<string, boolean>>({});
let keyTestStatus = $state<Record<string, { state: 'idle' | 'testing' | 'ok' | 'err'; count?: number; error?: string }>>({});
let apiProtocol = $state<ApiProtocol>('auto');
let proxyEnabled = $state(false);
let proxyUrl = $state('');
let modelTestStatus = $state<Record<string, { state: 'idle' | 'testing' | 'ok' | 'err'; latencyMs?: number; error?: string }>>({});

let fetchedModels = $state<DetectedModelInfo[]>([]);
let modelSearch = $state('');
let debouncedModelSearch = $state('');
let modelSearchTimer: ReturnType<typeof setTimeout> | null = null;
let visibleFetchedLimit = $state(80);
let customModelId = $state('');
let isFetchingModels = $state(false);
let fetchError = $state<string | null>(null);
let pendingModels = $state<DetectedModelInfo[]>([]);
let cardModelInputs = $state<Record<string, string>>({});

let fetchTimer: ReturnType<typeof setTimeout> | null = null;
let discoverReqId = 0;

interface OAuthAccountItem {
  id: string;
  provider: string;
  email: string;
  projectId?: string | null;
  accountId?: string | null;
  proxyUrl?: string | null;
  enabled: boolean;
  expiresAtMs: number;
  cooldownUntilMs?: number | null;
  lastError?: string | null;
  quota5hRemaining?: number | null;
  quota5hResetAtMs?: number | null;
  quotaWeeklyRemaining?: number | null;
  quotaWeeklyResetAtMs?: number | null;
  quotaLabel?: string | null;
  quotaUpdatedAtMs?: number | null;
}

const OAUTH_PROTOCOLS = ['antigravity', 'openai_codex', 'claude_oauth', 'kimi_oauth', 'xai_oauth', 'zcode_oauth', 'perplexity_oauth'] as const;

function isOAuthProvider(protocol?: string): boolean {
  return Boolean(protocol && OAUTH_PROTOCOLS.includes(protocol as any) && protocol !== 'perplexity_oauth');
}

const SHORT_REASONING_LABELS: Record<ReasoningLevel, string> = {
  OFF: 'OFF',
  LOW: 'LOW',
  MEDIUM: 'MED',
  HIGH: 'HIGH',
  MAX: 'MAX'
};

const OAUTH_PROVIDERS = [
  { id: 'antigravity', title: 'Antigravity (Google)', subtitle: 'Gemini, Claude & GPT-OSS via Google OAuth', kind: 'oauth' },
  { id: 'openai_codex', title: 'ChatGPT Codex (OpenAI)', subtitle: 'Plus/Pro/Team subscription via OAuth', kind: 'oauth' },
  { id: 'claude_oauth', title: 'Claude (Anthropic)', subtitle: 'Pro/Max subscription via Claude Code OAuth', kind: 'oauth' },
  { id: 'kimi_oauth', title: 'Kimi (Moonshot)', subtitle: 'Kimi Code subscription via Device Flow', kind: 'device' },
  { id: 'xai_oauth', title: 'Grok (xAI)', subtitle: 'SuperGrok / X Premium+ via Device Flow', kind: 'device' },
  { id: 'zcode_oauth', title: 'ZCode (Z.ai)', subtitle: 'GLM Coding / Start Plan via OAuth JWT', kind: 'oauth' },
  { id: 'perplexity_oauth', title: 'Perplexity', subtitle: 'API Key / Session pool (no public LLM OAuth)', kind: 'token' }
] as const;

let oauthAccounts = $state<OAuthAccountItem[]>([]);
let oauthStatus = $state<string | null>(null);
let oauthManualInputs = $state<Record<string, string>>({});
let oauthPendingMode = $state<Record<string, string>>({});
let oauthBusy = $state<Record<string, boolean>>({});
let accCheckStatus = $state<Record<string, { state: 'idle' | 'testing' | 'ok' | 'err'; error?: string }>>({});
let accProxyEditId = $state<string | null>(null);
let accProxyDraft = $state<string>('');
let oauthProxyOpen = $state<boolean>(false);
let oauthProxyEnabled = $state<boolean>(typeof localStorage !== 'undefined' && localStorage.getItem('omni_oauth_proxy_enabled') === '1');
let oauthProxyUrl = $state<string>(typeof localStorage !== 'undefined' ? (localStorage.getItem('omni_oauth_proxy_url') || '') : '');
let oauthProxyPoolText = $state<string>(typeof localStorage !== 'undefined' ? (localStorage.getItem('omni_oauth_proxy_pool') || '') : '');
let oauthRotationMode = $state<'sticky' | 'round_robin' | 'random' | 'none'>(
  (typeof localStorage !== 'undefined' ? (localStorage.getItem('omni_oauth_proxy_rotation') as any) : null) || 'sticky'
);
let proxySaveTimer: ReturnType<typeof setTimeout> | null = null;

function parseProxyPool(): string[] {
  return oauthProxyPoolText
    .split(/[\n,;]+/)
    .map((s) => s.trim())
    .filter(Boolean);
}

function saveOAuthProxy(silent = false) {
  const cleanUrl = oauthProxyUrl.trim();
  const pool = parseProxyPool();
  if (typeof localStorage !== 'undefined') {
    localStorage.setItem('omni_oauth_proxy_enabled', oauthProxyEnabled ? '1' : '0');
    localStorage.setItem('omni_oauth_proxy_url', cleanUrl);
    localStorage.setItem('omni_oauth_proxy_pool', pool.join('\n'));
    localStorage.setItem('omni_oauth_proxy_rotation', oauthRotationMode);
  }
  invoke('oauth_set_proxy', {
    enabled: oauthProxyEnabled,
    url: cleanUrl,
    proxyPool: pool,
    rotationMode: oauthRotationMode
  }).catch(() => {});
  for (const prov of Object.values(settingsStore.data.providers)) {
    if (OAUTH_PROTOCOLS.includes(prov.apiProtocol as any)) {
      settingsStore.updateProvider(prov.id, {
        proxyEnabled: oauthProxyEnabled,
        proxyUrl: cleanUrl
      });
    }
  }
  if (!silent) {
    oauthStatus = oauthProxyEnabled
      ? pool.length > 0
        ? `Global proxy rotation saved (${pool.length} proxies, mode: ${oauthRotationMode})`
        : cleanUrl
          ? `Global proxy saved: ${cleanUrl}`
          : 'Global proxy enabled (enter URL or pool)'
      : 'Global proxy disabled';
  }
}

function scheduleAutoSaveProxy() {
  if (proxySaveTimer) clearTimeout(proxySaveTimer);
  proxySaveTimer = setTimeout(() => {
    saveOAuthProxy(true);
  }, 350);
}

async function saveAccountProxy(acc: OAuthAccountItem, value: string | null) {
  const cleaned = value?.trim() || null;
  try {
    await invoke('oauth_set_account_proxy', { id: acc.id, proxyUrl: cleaned });
    accProxyEditId = null;
    accProxyDraft = '';
    await loadOAuthAccounts();
    oauthStatus = cleaned ? `Proxy set for ${acc.email}` : `Proxy cleared for ${acc.email}`;
  } catch (err: unknown) {
    oauthStatus = err instanceof Error ? err.message : String(err);
  }
}

function handleModelSearchInput(e: Event) {
  modelSearch = (e.currentTarget as HTMLInputElement).value;
  if (modelSearchTimer) clearTimeout(modelSearchTimer);
  modelSearchTimer = setTimeout(() => {
    debouncedModelSearch = modelSearch.trim().toLowerCase();
    visibleFetchedLimit = 80;
  }, 180);
}

function formatResetDelta(ms?: number | null): string {
  if (!ms) return '';
  const diff = Math.max(0, ms - Date.now());
  if (diff <= 0) return 'resetting';
  const mins = Math.floor(diff / 60000);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 48) return `${hrs}h ${mins % 60}m`;
  const days = Math.floor(hrs / 24);
  return `${days}d ${hrs % 24}h`;
}

function formatAccQuotaTooltip(acc: OAuthAccountItem): string {
  const parts: string[] = [];
  if (acc.quotaLabel) parts.push(`Plan: ${acc.quotaLabel}`);
  if (acc.proxyUrl) parts.push(`Proxy: ${acc.proxyUrl}`);
  if (typeof acc.quota5hRemaining === 'number') {
    const d = formatResetDelta(acc.quota5hResetAtMs);
    parts.push(`5h: ${Math.round(acc.quota5hRemaining)}%${d ? ` (resets ${d})` : ''}`);
  }
  if (typeof acc.quotaWeeklyRemaining === 'number') {
    const d = formatResetDelta(acc.quotaWeeklyResetAtMs);
    parts.push(`Weekly: ${Math.round(acc.quotaWeeklyRemaining)}%${d ? ` (resets ${d})` : ''}`);
  }
  if (acc.lastError) parts.push(`Error: ${acc.lastError}`);
  return parts.join(' · ') || 'Quota not fetched yet';
}

function aggregateLiveQuota(accs: OAuthAccountItem[]): { q5h: number | null; q7d: number | null; liveCount: number } {
  const now = Date.now();
  const live = accs.filter((a) => a.enabled && (!a.cooldownUntilMs || a.cooldownUntilMs <= now));
  const q5Vals = live.map((a) => a.quota5hRemaining).filter((v): v is number => typeof v === 'number');
  const q7Vals = live.map((a) => a.quotaWeeklyRemaining).filter((v): v is number => typeof v === 'number');
  const q5h = q5Vals.length > 0 ? Math.min(100, Math.round(q5Vals.reduce((s, x) => s + x, 0) / q5Vals.length)) : null;
  const q7d = q7Vals.length > 0 ? Math.min(100, Math.round(q7Vals.reduce((s, x) => s + x, 0) / q7Vals.length)) : null;
  return { q5h, q7d, liveCount: live.length };
}

function tierRank(label: string): number {
  const l = label.toLowerCase();
  if (l.includes('ultra') || l === 'gpt pro' || l.includes('max')) return 4;
  if (l.includes('enterprise') || l.includes('business') || l.includes('team') || l.includes('edu')) return 3;
  if (l.includes('pro') || l.includes('plus')) return 2;
  if (l.includes('go') || l.includes('standard')) return 1;
  return 0;
}

function tierBadgeClass(label?: string | null): string {
  const l = (label || '').toLowerCase();
  if (l.includes('ultra') || l === 'gpt pro' || l.includes('max')) {
    return 'border-[#8B5CF6]/45 bg-[#8B5CF6]/12 text-[#DDD6FE]';
  }
  if (l.includes('pro') || l.includes('plus') || l.includes('team') || l.includes('business') || l.includes('enterprise') || l.includes('edu') || l.includes('go')) {
    return 'border-white/[0.12] bg-white/[0.06] text-[#E5E5E8]';
  }
  return 'border-white/[0.06] bg-white/[0.03] text-[#8E8E95]';
}

function summarizeProviderTiers(accs: OAuthAccountItem[]): string[] {
  const active = accs.filter((a) => a.enabled && a.quotaLabel);
  const pool = active.length > 0 ? active : accs.filter((a) => a.quotaLabel);
  const unique = Array.from(new Set(pool.map((a) => a.quotaLabel!.trim()).filter(Boolean)));
  unique.sort((a, b) => tierRank(b) - tierRank(a));
  return unique.slice(0, 2);
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
  const pool = parseProxyPool();
  if (oauthProxyEnabled && pool.length > 0) {
    return { proxyEnabled: true, proxyUrl: pool[0] };
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
  let disposed = false;
  invoke<{ enabled: boolean; url: string; proxyPool?: string[]; rotationMode?: string }>('oauth_get_proxy')
    .then((px) => {
      if (disposed || !px) return;
      if (px.enabled || px.url || (px.proxyPool && px.proxyPool.length > 0)) {
        oauthProxyEnabled = Boolean(px.enabled);
        oauthProxyUrl = px.url || '';
        if (Array.isArray(px.proxyPool)) {
          oauthProxyPoolText = px.proxyPool.join('\n');
        }
        if (px.rotationMode && ['sticky', 'round_robin', 'random', 'none'].includes(px.rotationMode)) {
          oauthRotationMode = px.rotationMode as any;
        }
      }
    })
    .catch(() => {});
  loadOAuthAccounts().then(() => {
    if (disposed) return;
    const missingProviders = Array.from(
      new Set(
        oauthAccounts
          .filter((a) => a.enabled && !a.quotaLabel && (a.provider === 'antigravity' || a.provider === 'openai_codex'))
          .map((a) => a.provider)
      )
    );
    for (const prov of missingProviders) {
      refreshQuotas(prov);
    }
  });
  let unlistenZcode: (() => void) | null = null;
  listen<string>('zcode-oauth-callback', async (ev) => {
    const cbUrl = ev.payload;
    if (!cbUrl) return;
    oauthBusy = { ...oauthBusy, zcode_oauth: true };
    oauthStatus = 'Completing ZCode login...';
    try {
      await invoke('oauth_complete_manual_login', {
        provider: 'zcode_oauth',
        codeOrUrl: cbUrl,
        ...getOAuthProxyParams('zcode_oauth')
      });
      await loadOAuthAccounts();
      await ensureOAuthProviderRegistered('zcode_oauth');
      oauthPendingMode = { ...oauthPendingMode, zcode_oauth: '' };
      oauthStatus = 'ZCode account connected.';
    } catch (err: any) {
      oauthStatus = err instanceof Error ? err.message : String(err);
    } finally {
      oauthBusy = { ...oauthBusy, zcode_oauth: false };
    }
  })
    .then((fn) => {
      if (disposed) fn();
      else unlistenZcode = fn;
    })
    .catch(() => {});

  let unlisten: (() => void) | null = null;
  listen<{ provider?: string }>('oauth-accounts-updated', async (ev) => {
    await loadOAuthAccounts();
    if (ev.payload?.provider) {
      await ensureOAuthProviderRegistered(ev.payload.provider);
    }
  })
    .then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    })
    .catch(() => {});

  return () => {
    disposed = true;
    if (fetchTimer) clearTimeout(fetchTimer);
    if (proxySaveTimer) clearTimeout(proxySaveTimer);
    if (modelSearchTimer) clearTimeout(modelSearchTimer);
    if (unlisten) unlisten();
    if (unlistenZcode) unlistenZcode();
  };
});

async function ensureOAuthProviderRegistered(oauthProv: string) {
  const existing = Object.values(settingsStore.data.providers).find((p) => p.apiProtocol === oauthProv);
  const px = getOAuthProxyParams(oauthProv);
  const cmdMap: Record<string, string> = {
    antigravity: 'oauth_fetch_antigravity_models',
    openai_codex: 'oauth_fetch_codex_models',
    claude_oauth: 'oauth_fetch_claude_models',
    kimi_oauth: 'oauth_fetch_kimi_models',
    xai_oauth: 'oauth_fetch_xai_models',
    zcode_oauth: 'oauth_fetch_zcode_models',
    perplexity_oauth: 'oauth_fetch_perplexity_models'
  };
  const cmdName = cmdMap[oauthProv] || 'oauth_fetch_claude_models';

  let models: DetectedModelInfo[] = [];
  try {
    const raw = await invoke<any[]>(cmdName, px);
    if (Array.isArray(raw)) {
      models = raw.map((m) => {
        const idLow = (m.id || '').toLowerCase();
        const defaultCtx = idLow.includes('gemini') ? 1048576 : (idLow.includes('sonnet-4') || idLow.includes('sonnet-5') || idLow.includes('sol') || idLow.includes('astra')) ? 1000000 : 200000;
        return {
          id: m.id,
          name: m.name || m.id,
          contextWindow: m.contextWindow || defaultCtx,
          maxOutputTokens: m.maxOutputTokens || 32768,
          supportsReasoning: Boolean(m.supportsReasoning),
          supportsVision: m.supportsVision !== false,
          supportsTools: m.supportsTools !== false,
          supportsAudio: Boolean(m.supportsAudio),
          supportsFiles: true
        };
      });
    }
  } catch (err: any) {
    oauthStatus = err instanceof Error ? err.message : String(err);
  }

  if (models.length === 0) {
    const fallbackIds =
      oauthProv === 'antigravity'
        ? ['gemini-3.8-flash', 'gemini-3.8-flash-high', 'gemini-3.8-flash-medium', 'gemini-3.8-flash-low', 'claude-sonnet-4-6', 'claude-opus-4-6-thinking', 'gpt-oss-120b', 'gpt-oss-120b-medium', 'gemini-3.1-flash-image']
        : oauthProv === 'openai_codex'
          ? ['gpt-6.1-sol', 'gpt-6-astra', 'gpt-6-sol', 'gpt-6.1-terra', 'gpt-6-luna', 'gpt-6-chat-thinking']
          : ['claude-opus-4-6', 'claude-sonnet-4-6', 'claude-haiku-4-5'];
    models = await Promise.all(
      fallbackIds.map(async (id) => {
        try {
          return await invoke<DetectedModelInfo>('detect_model_capabilities', { modelId: id });
        } catch {
          const isGem = id.includes('gemini');
          const isSonnet = id.includes('sonnet') || id.includes('sol') || id.includes('astra');
          return {
            id,
            name: id,
            contextWindow: isGem ? 1048576 : isSonnet ? 1000000 : oauthProv === 'openai_codex' ? 524288 : 200000,
            maxOutputTokens: 65536,
            supportsReasoning: true,
            supportsVision: true,
            supportsTools: true,
            supportsAudio: isGem,
            supportsFiles: true
          };
        }
      })
    );
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
    const metaMap: Record<string, { id: string; name: string; baseUrl: string }> = {
      antigravity: { id: 'antigravity-oauth', name: 'Antigravity', baseUrl: 'https://daily-cloudcode-pa.googleapis.com' },
      openai_codex: { id: 'openai-codex-oauth', name: 'ChatGPT Codex', baseUrl: 'https://chatgpt.com/backend-api/codex' },
      claude_oauth: { id: 'claude-oauth', name: 'Claude OAuth', baseUrl: 'https://api.anthropic.com/v1' },
      kimi_oauth: { id: 'kimi-oauth', name: 'Kimi (Moonshot)', baseUrl: 'https://api.kimi.com/coding/v1' },
      xai_oauth: { id: 'xai-oauth', name: 'Grok (xAI)', baseUrl: 'https://cli-chat-proxy.grok.com/v1' },
      zcode_oauth: { id: 'zcode-oauth', name: 'ZCode (Z.ai)', baseUrl: 'https://api.z.ai/api/anthropic/v1' },
      perplexity_oauth: { id: 'perplexity-oauth', name: 'Perplexity', baseUrl: 'https://api.perplexity.ai' }
    };
    const meta = metaMap[oauthProv] || { id: oauthProv, name: oauthProv, baseUrl: '' };
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
  if (provId === 'perplexity_oauth') {
    oauthPendingMode = { ...oauthPendingMode, [provId]: 'token' };
    oauthStatus = 'Paste your Perplexity API Key (pplx-...) or session token below.';
    return;
  }
  oauthBusy = { ...oauthBusy, [provId]: true };
  oauthStatus = null;
  try {
    const res = await invoke<{ authUrl: string; mode: string; userCode?: string }>('oauth_start_login', {
      provider: provId,
      ...getOAuthProxyParams(provId)
    });
    oauthPendingMode = { ...oauthPendingMode, [provId]: res.mode || 'manual' };
    if (res.mode === 'device_code') {
      oauthStatus = `Browser opened for ${provId}. Enter code ${res.userCode || ''} — waiting for authorization...`;
    } else if (res.mode === 'callback') {
      oauthStatus = `Browser opened for ${provId}. Waiting for callback (or paste redirect URL below)...`;
    } else {
      oauthStatus = `Browser opened for ${provId}. Paste the authorization code or redirect URL below.`;
    }
  } catch (err: any) {
    oauthStatus = err instanceof Error ? err.message : String(err);
  } finally {
    oauthBusy = { ...oauthBusy, [provId]: false };
  }
}

async function checkAccountLiveness(acc: OAuthAccountItem) {
  accCheckStatus = { ...accCheckStatus, [acc.id]: { state: 'testing' } };
  try {
    await invoke<OAuthAccountItem>('oauth_check_account', {
      id: acc.id,
      ...getOAuthProxyParams(acc.provider)
    });
    await loadOAuthAccounts();
    accCheckStatus = { ...accCheckStatus, [acc.id]: { state: 'ok' } };
  } catch (err: any) {
    const msg = err instanceof Error ? err.message : String(err);
    await loadOAuthAccounts();
    accCheckStatus = { ...accCheckStatus, [acc.id]: { state: 'err', error: msg } };
  }
}

async function refreshQuotas(provider?: string) {
  const key = provider ? `q_${provider}` : 'q_all';
  oauthBusy = { ...oauthBusy, [key]: true };
  oauthStatus = null;
  try {
    const refreshed = await invoke<OAuthAccountItem[]>('oauth_refresh_quotas', {
      provider: provider || null,
      ...getOAuthProxyParams(provider)
    });
    await loadOAuthAccounts();
    const active = refreshed.filter((a) => a.enabled);
    const failed = active.filter((a) => Boolean(a.lastError));
    if (failed.length > 0) {
      const sample = failed[0];
      oauthStatus = `Quota refresh failed for ${failed.length}/${active.length} account(s): ${sample.email} — ${sample.lastError}`;
    } else if (active.length > 0) {
      oauthStatus = `Quotas updated for ${active.length} account(s)`;
    }
  } catch (err: any) {
    oauthStatus = err instanceof Error ? err.message : String(err);
  } finally {
    oauthBusy = { ...oauthBusy, [key]: false };
  }
}

async function completeManualOAuth(provId: string) {
  const code = (oauthManualInputs[provId] || '').trim();
  if (!code) return;
  oauthBusy = { ...oauthBusy, [provId]: true };
  oauthStatus = null;
  try {
    const acc = provId === 'perplexity_oauth'
      ? await invoke<OAuthAccountItem>('oauth_add_manual_token', {
          provider: provId,
          label: 'perplexity',
          token: code
        })
      : await invoke<OAuthAccountItem>('oauth_complete_manual_login', {
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
let regularProvidersList = $derived(providersList.filter((p) => !OAUTH_PROTOCOLS.includes((p.apiProtocol || '') as any)));

let groupedModelsByProvider = $derived.by(() => {
  const map = new Map<string, ReturnType<typeof groupProviderModels>>();
  for (const p of providersList) {
    map.set(p.id, groupProviderModels(p.availableModels));
  }
  return map;
});

let filteredFetchedModels = $derived.by(() => {
  if (!debouncedModelSearch) return fetchedModels;
  return fetchedModels.filter(
    (m) => m.id.toLowerCase().includes(debouncedModelSearch) || m.name.toLowerCase().includes(debouncedModelSearch)
  );
});

function makeKeyEntry(value: string): ApiKeyEntry {
  return { id: `key-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`, value };
}

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
  debouncedModelSearch = '';
  visibleFetchedLimit = 80;
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
    .filter(Boolean)
    .map(makeKeyEntry);
  newKeyInput = '';
  revealedKeys = {};
  keyTestStatus = {};
  apiProtocol = prov.apiProtocol || 'auto';
  proxyEnabled = Boolean(prov.proxyEnabled);
  proxyUrl = prov.proxyUrl || '';
  fetchedModels = [];
  modelSearch = '';
  debouncedModelSearch = '';
  visibleFetchedLimit = 80;
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
  if (!apiKeys.some((item) => item.value === k)) {
    apiKeys = [...apiKeys, makeKeyEntry(k)];
    if (apiKeys.length === 1 && baseUrl.trim().startsWith('http')) {
      discoverModels();
    }
  }
  newKeyInput = '';
}

function removeApiKeyFromList(keyId: string) {
  apiKeys = apiKeys.filter((item) => item.id !== keyId);
  const nextRevealed = { ...revealedKeys };
  delete nextRevealed[keyId];
  revealedKeys = nextRevealed;
  const nextTests = { ...keyTestStatus };
  delete nextTests[keyId];
  keyTestStatus = nextTests;
}

async function testSingleApiKey(entry: ApiKeyEntry) {
  const url = baseUrl.trim();
  if (!url || !entry.value) return;
  keyTestStatus = { ...keyTestStatus, [entry.id]: { state: 'testing' } };
  try {
    const res = await invoke<DetectedModelInfo[]>('fetch_provider_models', {
      baseUrl: url,
      apiKey: entry.value,
      proxyEnabled,
      proxyUrl: proxyUrl.trim() || null
    });
    if (!apiKeys.some((k) => k.id === entry.id)) return;
    keyTestStatus = { ...keyTestStatus, [entry.id]: { state: 'ok', count: Array.isArray(res) ? res.length : 0 } };
  } catch (err: unknown) {
    if (!apiKeys.some((k) => k.id === entry.id)) return;
    keyTestStatus = {
      ...keyTestStatus,
      [entry.id]: { state: 'err', error: err instanceof Error ? err.message : String(err) }
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
  debouncedModelSearch = '';
  visibleFetchedLimit = 80;
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
  const reqId = ++discoverReqId;
  isFetchingModels = true;
  fetchError = null;
  try {
    const activeKey = apiKeys[0]?.value || newKeyInput.trim();
    const res = await invoke<DetectedModelInfo[]>('fetch_provider_models', {
      baseUrl: url,
      apiKey: activeKey,
      proxyEnabled,
      proxyUrl: proxyUrl.trim() || null
    });
    if (reqId !== discoverReqId) return;
    const dedupedMap = new Map<string, DetectedModelInfo>();
    if (Array.isArray(res)) {
      for (const item of res) {
        if (item?.id && !dedupedMap.has(item.id)) {
          dedupedMap.set(item.id, item);
        }
      }
    }
    fetchedModels = Array.from(dedupedMap.values());
    visibleFetchedLimit = 80;
    if (dedupedMap.size > 0 && pendingModels.length > 0) {
      pendingModels = pendingModels.map((pm) => dedupedMap.get(pm.id) || pm);
    }
  } catch (err: unknown) {
    if (reqId !== discoverReqId) return;
    fetchError = err instanceof Error ? err.message : String(err);
    fetchedModels = [];
  } finally {
    if (reqId === discoverReqId) {
      isFetchingModels = false;
    }
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

  const finalKeys = apiKeys.map((k) => k.value);
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
        class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border text-xs transition-all whitespace-nowrap {oauthProxyEnabled && (oauthProxyUrl.trim() || oauthProxyPoolText.trim()) ? 'border-[#8B5CF6]/50 text-[#D8B4FE]' : 'border-white/[0.08] text-[#A3A3AA]'}"
      >
        {oauthProxyEnabled && (oauthProxyUrl.trim() || oauthProxyPoolText.trim()) ? 'Proxy: ON' : 'Global Proxy'}
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
    <div class="p-3.5 rounded-[6px] bg-[#18181B]/70 border border-white/[0.08] space-y-2.5" in:slide={{ duration: 140 }}>
      <div class="flex items-center gap-3">
        <div class="flex items-center gap-2 shrink-0">
          <button
            type="button"
            class="w-9 h-5 rounded-full p-0.5 transition-all duration-200 {oauthProxyEnabled ? 'bg-[#8B5CF6]' : 'bg-[#27272A]'}"
            onclick={() => {
              oauthProxyEnabled = !oauthProxyEnabled;
              saveOAuthProxy(false);
            }}
            aria-label="Enable Global Proxy"
          >
            <div class="w-4 h-4 rounded-full transition-transform duration-200 {oauthProxyEnabled ? 'translate-x-4 bg-white shadow-sm' : 'translate-x-0 bg-[#73737A]'}"></div>
          </button>
          <span class="text-xs text-[#D4D4D8]">Global Proxy</span>
        </div>
        <input
          type="text"
          bind:value={oauthProxyUrl}
          oninput={scheduleAutoSaveProxy}
          placeholder="Single proxy for all: http://127.0.0.1:7890 or socks5h://user:pass@host:1080"
          class="flex-1 h-7 px-2.5 rounded-[5px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#E5E5E8] placeholder-[#5C5C64] outline-none focus:border-[#8B5CF6]/50"
        />
        <select
          bind:value={oauthRotationMode}
          onchange={() => saveOAuthProxy(false)}
          class="h-7 px-2 rounded-[5px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#D4D4D8] outline-none"
          title="Proxy pool rotation strategy"
        >
          <option value="sticky">sticky</option>
          <option value="round_robin">round_robin</option>
          <option value="random">random</option>
          <option value="none">none</option>
        </select>
        <button
          type="button"
          onclick={() => refreshQuotas()}
          disabled={oauthBusy.q_all}
          class="h-7 px-2.5 rounded-[5px] bg-[#27272A] hover:bg-[#323236] border border-white/[0.08] text-[11px] text-[#EDEDEF] shrink-0 disabled:opacity-50"
          title="Refresh quotas for all OAuth accounts"
        >
          {oauthBusy.q_all ? 'Checking...' : 'Refresh Quotas'}
        </button>
      </div>
      <div class="space-y-1">
        <div class="flex items-center justify-between text-[10.5px] text-[#73737A]">
          <span>Proxy Rotation Pool (one proxy URL per line; used across all OAuth & API requests unless overridden per account)</span>
          <span class="font-mono">{parseProxyPool().length} in pool</span>
        </div>
        <textarea
          bind:value={oauthProxyPoolText}
          oninput={scheduleAutoSaveProxy}
          rows="2"
          placeholder={"http://user:pass@proxy1:8080\nsocks5h://user:pass@proxy2:1080"}
          class="w-full px-2.5 py-1.5 rounded-[5px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#E5E5E8] placeholder-[#55555C] outline-none focus:border-[#8B5CF6]/50 resize-y"
        ></textarea>
      </div>
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
      {@const agg = aggregateLiveQuota(accs)}
      {@const provTiers = summarizeProviderTiers(accs)}
      {@const linkedProv = providersList.find((p) => p.apiProtocol === op.id)}
      {@const linkedGrouped = linkedProv ? (groupedModelsByProvider.get(linkedProv.id) || []) : []}
      <div class="px-3.5 py-2.5 space-y-2">
        <div class="flex items-center justify-between gap-3">
          <div class="flex items-center gap-2.5 min-w-0">
            <span class="text-xs font-medium text-[#E5E5E8] truncate">{op.title}</span>
            {#if provTiers.length > 0}
              {#each provTiers as tier}
                <span class="px-1.5 py-0.5 rounded-[4px] border font-mono text-[9.5px] leading-none shrink-0 {tierBadgeClass(tier)}">{tier}</span>
              {/each}
            {:else}
              <span class="text-[11px] text-[#686870] truncate hidden sm:inline">{op.subtitle}</span>
            {/if}
            {#if accs.length > 0}
              <span class="px-1.5 py-0.5 rounded-[4px] bg-white/[0.05] font-mono text-[10px] text-[#A3A3AA] shrink-0">{agg.liveCount}/{accs.length}</span>
            {/if}
            {#if agg.q5h !== null}
              <span class="inline-flex items-center gap-1 text-[10px] font-mono text-[#D4D4D8] shrink-0" title="Combined 5h remaining quota across active accounts">
                <svg class="w-3.5 h-3.5 -rotate-90" viewBox="0 0 20 20">
                  <circle cx="10" cy="10" r="7.5" fill="none" stroke="rgba(255,255,255,0.1)" stroke-width="2.5" />
                  <circle cx="10" cy="10" r="7.5" fill="none" stroke={agg.q5h > 25 ? '#8B5CF6' : '#F59E0B'} stroke-width="2.5" stroke-dasharray="47.12" stroke-dashoffset={47.12 * (1 - agg.q5h / 100)} stroke-linecap="round" />
                </svg>
                5h {agg.q5h}%
              </span>
            {/if}
            {#if agg.q7d !== null}
              <span class="inline-flex items-center gap-1 text-[10px] font-mono text-[#A3A3AA] shrink-0" title="Combined weekly remaining quota across active accounts">
                <svg class="w-3.5 h-3.5 -rotate-90" viewBox="0 0 20 20">
                  <circle cx="10" cy="10" r="7.5" fill="none" stroke="rgba(255,255,255,0.1)" stroke-width="2.5" />
                  <circle cx="10" cy="10" r="7.5" fill="none" stroke={agg.q7d > 25 ? '#22C55E' : '#F59E0B'} stroke-width="2.5" stroke-dasharray="47.12" stroke-dashoffset={47.12 * (1 - agg.q7d / 100)} stroke-linecap="round" />
                </svg>
                7d {agg.q7d}%
              </span>
            {/if}
          </div>
          <div class="flex items-center gap-1.5 shrink-0">
            {#if accs.length > 0}
              <button
                type="button"
                disabled={oauthBusy[`q_${op.id}`]}
                onclick={() => refreshQuotas(op.id)}
                class="h-7 px-2 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.07] text-[10.5px] text-[#A3A3AA] hover:text-[#E5E5E8] transition-colors disabled:opacity-50"
                title="Refresh quota for this provider"
              >
                {oauthBusy[`q_${op.id}`] ? '...' : 'Quota'}
              </button>
              <button
                type="button"
                onclick={() => ensureOAuthProviderRegistered(op.id)}
                class="h-7 px-2.5 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.07] text-[11px] text-[#D4D4D8] transition-colors"
              >
                Sync Models
              </button>
            {/if}
            {#if op.kind !== 'token'}
              <button
                type="button"
                onclick={() => (oauthPendingMode = { ...oauthPendingMode, [op.id]: oauthPendingMode[op.id] ? '' : 'manual' })}
                class="h-7 px-2 rounded-[5px] bg-transparent hover:bg-white/[0.05] border border-white/[0.06] text-[11px] text-[#8E8E95] hover:text-[#D4D4D8] transition-colors"
                title="Paste callback URL or code manually"
              >
                Code
              </button>
            {/if}
            <button
              type="button"
              disabled={oauthBusy[op.id]}
              onclick={() => startOAuthLogin(op.id)}
              class="h-7 px-2.5 rounded-[5px] bg-[#27272A] hover:bg-[#323236] border border-white/[0.08] text-[11px] text-[#EDEDEF] transition-colors disabled:opacity-50"
            >
              {oauthBusy[op.id] ? 'Opening...' : op.kind === 'token' ? '+ Add Key' : '+ Connect'}
            </button>
          </div>
        </div>

        {#if oauthPendingMode[op.id]}
          <div class="flex items-center gap-1.5">
            <input
              type="text"
              bind:value={oauthManualInputs[op.id]}
              placeholder={op.kind === 'token' ? 'Paste Perplexity API key (pplx-...) or session token...' : 'Paste redirect URL or authorization code...'}
              class="flex-1 h-7 px-2.5 rounded-[5px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#E5E5E8] placeholder-[#5C5C64] outline-none focus:border-white/[0.2]"
            />
            <button
              type="button"
              disabled={oauthBusy[op.id] || !(oauthManualInputs[op.id] || '').trim()}
              onclick={() => completeManualOAuth(op.id)}
              class="h-7 px-2.5 rounded-[5px] bg-[#27272A] hover:bg-[#323236] border border-white/[0.08] text-[11px] text-[#EDEDEF] disabled:opacity-40"
            >
              {op.kind === 'token' ? 'Save' : 'Verify'}
            </button>
          </div>
        {/if}

        {#if accs.length > 0}
          <div class="space-y-1">
            {#each accs as acc (acc.id)}
              {@const chk = accCheckStatus[acc.id]}
              <div class="rounded-[5px] bg-[#121214] border border-white/[0.05]">
                <div
                  class="flex items-center justify-between gap-2 px-2.5 py-1.5 text-[11px]"
                  title={formatAccQuotaTooltip(acc)}
                >
                  <div class="flex items-center gap-2 min-w-0">
                    <span class="w-1.5 h-1.5 rounded-full shrink-0 {acc.enabled ? (acc.cooldownUntilMs && acc.cooldownUntilMs > Date.now() ? 'bg-[#F59E0B]' : 'bg-[#22C55E]') : 'bg-[#52525B]'}"></span>
                    <span class="font-mono text-[#D4D4D8] truncate">{acc.email}</span>
                    {#if acc.quotaLabel}
                      <span class="px-1.5 py-0.5 rounded-[4px] border font-mono text-[9.5px] leading-none shrink-0 {tierBadgeClass(acc.quotaLabel)}">{acc.quotaLabel}</span>
                    {/if}
                    {#if acc.proxyUrl}
                      <span class="px-1.5 py-0.5 rounded-[4px] border border-[#8B5CF6]/40 bg-[#8B5CF6]/10 font-mono text-[9.5px] leading-none text-[#DDD6FE] shrink-0" title={acc.proxyUrl}>PX</span>
                    {/if}
                    {#if typeof acc.quota5hRemaining === 'number'}
                      <span class="inline-flex items-center gap-1 font-mono text-[10px] text-[#C4B5FD] shrink-0">
                        <svg class="w-3 h-3 -rotate-90" viewBox="0 0 20 20">
                          <circle cx="10" cy="10" r="7" fill="none" stroke="rgba(255,255,255,0.1)" stroke-width="2.5" />
                          <circle cx="10" cy="10" r="7" fill="none" stroke={acc.quota5hRemaining > 20 ? '#8B5CF6' : '#EF4444'} stroke-width="2.5" stroke-dasharray="43.98" stroke-dashoffset={43.98 * (1 - acc.quota5hRemaining / 100)} stroke-linecap="round" />
                        </svg>
                        {Math.round(acc.quota5hRemaining)}%
                      </span>
                    {/if}
                    {#if typeof acc.quotaWeeklyRemaining === 'number'}
                      <span class="inline-flex items-center gap-1 font-mono text-[10px] text-[#86EFAC] shrink-0">
                        <svg class="w-3 h-3 -rotate-90" viewBox="0 0 20 20">
                          <circle cx="10" cy="10" r="7" fill="none" stroke="rgba(255,255,255,0.1)" stroke-width="2.5" />
                          <circle cx="10" cy="10" r="7" fill="none" stroke={acc.quotaWeeklyRemaining > 20 ? '#22C55E' : '#EF4444'} stroke-width="2.5" stroke-dasharray="43.98" stroke-dashoffset={43.98 * (1 - acc.quotaWeeklyRemaining / 100)} stroke-linecap="round" />
                        </svg>
                        {Math.round(acc.quotaWeeklyRemaining)}%
                      </span>
                    {/if}
                    {#if acc.projectId}
                      <span class="font-mono text-[10px] text-[#686870] truncate">{acc.projectId}</span>
                    {/if}
                    {#if acc.lastError}
                      <span class="text-[10px] text-[#F87171] truncate max-w-[260px]" title={acc.lastError}>{acc.lastError}</span>
                    {/if}
                  </div>
                  <div class="flex items-center gap-1 shrink-0">
                    {#if chk?.state === 'testing'}
                      <span class="font-mono text-[10px] text-[#A3A3AA] px-1">...</span>
                    {:else if chk?.state === 'ok'}
                      <span class="font-mono text-[10px] text-[#4ADE80] px-1">OK</span>
                    {:else if chk?.state === 'err'}
                      <span class="font-mono text-[10px] text-[#F87171] px-1" title={chk.error || 'Error'}>ERR</span>
                    {/if}
                    <button
                      type="button"
                      onclick={() => {
                        if (accProxyEditId === acc.id) {
                          accProxyEditId = null;
                          accProxyDraft = '';
                        } else {
                          accProxyEditId = acc.id;
                          accProxyDraft = acc.proxyUrl || '';
                        }
                      }}
                      class="px-2 py-0.5 rounded text-[10px] transition-colors {acc.proxyUrl ? 'bg-[#8B5CF6]/20 text-[#DDD6FE]' : 'bg-white/[0.04] hover:bg-white/[0.08] text-[#A3A3AA]'}"
                      title="Set dedicated proxy for this OAuth account"
                    >
                      Proxy
                    </button>
                    <button
                      type="button"
                      onclick={() => checkAccountLiveness(acc)}
                      class="px-2 py-0.5 rounded bg-white/[0.06] hover:bg-white/[0.12] text-[10px] text-[#D4D4D8] transition-colors"
                      title="Check if account is alive & refresh quota"
                    >
                      Check
                    </button>
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
                {#if accProxyEditId === acc.id}
                  <div class="px-2.5 pb-2 pt-1 border-t border-white/[0.05] flex items-center gap-1.5">
                    <input
                      type="text"
                      bind:value={accProxyDraft}
                      placeholder="Dedicated proxy: socks5h://user:pass@host:port or http://host:port"
                      class="flex-1 h-7 px-2 rounded-[4px] bg-[#18181B] border border-white/[0.08] font-mono text-[10.5px] text-[#E5E5E8] placeholder-[#5C5C64] outline-none"
                      onkeydown={(e) => {
                        if (e.key === 'Enter') {
                          e.preventDefault();
                          saveAccountProxy(acc, accProxyDraft);
                        }
                      }}
                    />
                    <button
                      type="button"
                      onclick={() => saveAccountProxy(acc, accProxyDraft)}
                      class="h-7 px-2.5 rounded-[4px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-[10.5px] font-medium"
                    >
                      Save
                    </button>
                    {#if acc.proxyUrl}
                      <button
                        type="button"
                        onclick={() => saveAccountProxy(acc, null)}
                        class="h-7 px-2 rounded-[4px] bg-white/[0.05] hover:bg-[#EF4444]/20 text-[10.5px] text-[#A3A3AA] hover:text-[#FCA5A5]"
                      >
                        Clear
                      </button>
                    {/if}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {/if}

        {#if linkedProv}
          {#if showForm && editingId === linkedProv.id}
            <div class="pt-2">
              {@render providerForm()}
            </div>
          {:else}
            <div class="pt-2 border-t border-white/[0.06] space-y-1.5">
              <div class="flex items-center justify-between gap-2">
                <span class="text-[10.5px] text-[#73737A]">Enabled Models ({linkedGrouped.length})</span>
                <div class="flex items-center gap-1">
                  <button
                    type="button"
                    class="p-1 rounded-[5px] text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
                    onclick={() => openEditForm(linkedProv)}
                    title="Edit models"
                  >
                    <Icon name="edit" class="w-3 h-3" strokeWidth={1.6} />
                  </button>
                  <button
                    type="button"
                    class="p-1 rounded-[5px] text-[#73737A] hover:text-[#EF4444] hover:bg-white/[0.06] transition-colors"
                    onclick={() => settingsStore.removeProvider(linkedProv.id)}
                    title="Remove synced provider"
                  >
                    <Icon name="trash" class="w-3 h-3" strokeWidth={1.6} />
                  </button>
                </div>
              </div>
              <div class="flex flex-wrap gap-1.5">
                {#each linkedGrouped as grp (grp.displayId)}
                  {@const probeId = grp.rawIds[0]}
                  {@const cap = linkedProv.modelCapabilities?.[probeId]}
                  {@const tStatus = modelTestStatus[`${linkedProv.id}:${probeId}`]}
                  {@const variantLevels = (['OFF', 'LOW', 'MEDIUM', 'HIGH', 'MAX'] as ReasoningLevel[]).filter((l) => Boolean(grp.reasoningVariants[l]))}
                  <div class="flex items-center rounded-[6px] border bg-[#121214] text-[#D4D4D8] border-white/[0.08]">
                    <button
                      type="button"
                      class="pl-2 pr-0.5 py-1 flex items-center justify-center transition-colors"
                      onclick={() => testModel(linkedProv, probeId)}
                      title={tStatus?.state === 'ok' ? `Available (${tStatus.latencyMs}ms)` : tStatus?.state === 'err' ? `Unavailable: ${tStatus.error || 'Error'}` : 'Test model availability'}
                    >
                      <span class="w-2 h-2 rounded-full transition-colors {tStatus?.state === 'ok' ? 'bg-[#4ADE80]' : tStatus?.state === 'err' ? 'bg-[#F87171]' : tStatus?.state === 'testing' ? 'bg-[#EDEDEF] animate-pulse' : 'bg-[#52525B] hover:bg-[#A3A3AA]'}"></span>
                    </button>
                    <div class="pl-1.5 pr-2 py-1 font-mono text-[11px] flex items-center gap-1.5">
                      <span>{grp.displayId}</span>
                      {#if cap}
                        <span class="text-[9.5px] text-[#73737A]">{formatCtx(cap.contextWindow)}</span>
                        {#if variantLevels.length > 0}
                          <span class="text-[9.5px] text-[#8E8E95]">{variantLevels.map((l) => SHORT_REASONING_LABELS[l]).join('·')}</span>
                        {:else if cap.supportsReasoning && !isNonReasoningModelId(probeId)}
                          <span class="text-[9.5px] text-[#8E8E95]">Reasoning</span>
                        {/if}
                        {#if cap.supportsVision}
                          <span class="text-[9.5px] text-[#8E8E95]">Vision</span>
                        {/if}
                      {:else if variantLevels.length > 0}
                        <span class="text-[9.5px] text-[#8E8E95]">{variantLevels.map((l) => SHORT_REASONING_LABELS[l]).join('·')}</span>
                      {/if}
                    </div>
                    <button
                      type="button"
                      class="pr-2 pl-1 text-xs text-[#73737A] hover:text-[#EF4444] transition-colors"
                      onclick={() => {
                        for (const rawId of grp.rawIds) {
                          settingsStore.removeModelFromProvider(linkedProv.id, rawId);
                        }
                      }}
                      title="Remove model"
                    >
                      ×
                    </button>
                  </div>
                {/each}
              </div>
              <div class="flex items-center gap-1.5 pt-0.5">
                <input
                  type="text"
                  bind:value={cardModelInputs[linkedProv.id]}
                  placeholder="Add model ID..."
                  class="flex-1 px-2.5 py-1 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#E5E5E8] placeholder-[#55555C] outline-none"
                  onkeydown={(e) => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      addCustomModelToExisting(linkedProv.id);
                    }
                  }}
                />
                <button
                  type="button"
                  class="w-7 h-7 rounded-[6px] bg-[#121214] hover:bg-[#EDEDEF] hover:text-[#111113] border border-white/[0.08] flex items-center justify-center text-sm text-[#D4D4D8] shrink-0 active:scale-[0.95] transition-all"
                  onclick={() => addCustomModelToExisting(linkedProv.id)}
                  title="Add model"
                >
                  +
                </button>
              </div>
            </div>
          {/if}
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
          {#each apiKeys as entry, idx (entry.id)}
            {@const tStatus = keyTestStatus[entry.id]}
            <div class="flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.06]">
              <div class="flex items-center gap-2 min-w-0 flex-1">
                <span class="font-mono text-[10px] text-[#66666D] shrink-0">#{idx + 1}</span>
                <span class="font-mono text-[11px] text-[#E5E5E8] truncate select-all">
                  {revealedKeys[entry.id] ? entry.value : maskKey(entry.value)}
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
                  onclick={() => testSingleApiKey(entry)}
                  class="px-2 py-0.5 rounded-[4px] bg-white/[0.05] hover:bg-white/[0.1] text-[10.5px] text-[#D4D4D8] transition-colors"
                  title="Test this key"
                >
                  Test
                </button>
                <button
                  type="button"
                  onclick={() => (revealedKeys = { ...revealedKeys, [entry.id]: !revealedKeys[entry.id] })}
                  class="px-2 py-0.5 rounded-[4px] bg-white/[0.05] hover:bg-white/[0.1] text-[10.5px] text-[#A3A3AA] hover:text-white transition-colors"
                  title={revealedKeys[entry.id] ? 'Hide key' : 'Show key'}
                >
                  {revealedKeys[entry.id] ? 'Hide' : 'Show'}
                </button>
                <button
                  type="button"
                  onclick={() => removeApiKeyFromList(entry.id)}
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
      {#if OAUTH_PROTOCOLS.includes(apiProtocol as any)}
        <div class="px-3 py-2 rounded-[6px] bg-[#121214] border border-white/[0.06] font-mono text-[11px] text-[#C4B5FD]">
          OAuth: {apiProtocol} (managed in Subscription OAuth Accounts above)
        </div>
      {:else}
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
      {/if}
    </div>

    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <span class="text-xs text-[#A3A3AA]">Enable Proxy (HTTP / HTTPS / SOCKS5 / SOCKS5h)</span>
        <button
          type="button"
          class="w-9 h-5 rounded-full p-0.5 transition-all duration-200 {proxyEnabled ? 'bg-[#8B5CF6]' : 'bg-[#27272A]'}"
          onclick={() => {
            proxyEnabled = !proxyEnabled;
            if (editingId) {
              settingsStore.updateProvider(editingId, { proxyEnabled, proxyUrl: proxyUrl.trim() });
            }
          }}
          aria-label="Enable Proxy"
        >
          <div class="w-4 h-4 rounded-full transition-transform duration-200 {proxyEnabled ? 'translate-x-4 bg-white shadow-sm' : 'translate-x-0 bg-[#73737A]'}"></div>
        </button>
      </div>
      {#if proxyEnabled}
        <input
          bind:value={proxyUrl}
          oninput={() => {
            if (editingId) {
              settingsStore.updateProvider(editingId, { proxyEnabled, proxyUrl: proxyUrl.trim() });
            }
          }}
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
            value={modelSearch}
            oninput={handleModelSearchInput}
            type="text"
            placeholder="Search discovered models ({fetchedModels.length})..."
            class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none"
          />
          <div class="max-h-48 overflow-y-auto rounded-[6px] bg-[#121214] border border-white/[0.06] divide-y divide-white/[0.04]">
            {#each filteredFetchedModels.slice(0, visibleFetchedLimit) as m (m.id)}
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
            {#if filteredFetchedModels.length > visibleFetchedLimit}
              <button
                type="button"
                onclick={() => (visibleFetchedLimit += 80)}
                class="w-full py-2 text-center font-mono text-[11px] text-[#8E8E95] hover:text-[#EDEDEF] hover:bg-white/[0.03]"
              >
                Show more ({filteredFetchedModels.length - visibleFetchedLimit} remaining)
              </button>
            {/if}
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
            <div class="px-2.5 py-1 rounded-[6px] bg-[#121214] border border-white/[0.08] flex items-center gap-2 text-[11px]">
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
  {:else if regularProvidersList.length === 0 && oauthAccounts.length === 0 && providersList.length === 0}
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

  {#if regularProvidersList.length > 0}
    <div class="space-y-3">
      {#each regularProvidersList as prov (prov.id)}
        {@const provGrouped = groupedModelsByProvider.get(prov.id) || []}
        {#if showForm && editingId === prov.id}
          <div transition:slide={{ duration: 180 }}>
            {@render providerForm()}
          </div>
        {:else}
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
            {#if !isOAuthProvider(prov.apiProtocol)}
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
            {/if}

            <div class="space-y-1.5">
              <span class="text-[11px] text-[#73737A] block">Enabled Models</span>
              <div class="flex flex-wrap gap-1.5">
                {#each provGrouped as grp (grp.displayId)}
                  {@const probeId = grp.rawIds[0]}
                  {@const cap = prov.modelCapabilities?.[probeId]}
                  {@const tStatus = modelTestStatus[`${prov.id}:${probeId}`]}
                  {@const variantLevels = (['OFF', 'LOW', 'MEDIUM', 'HIGH', 'MAX'] as ReasoningLevel[]).filter((l) => Boolean(grp.reasoningVariants[l]))}
                  <div class="flex items-center rounded-[6px] border bg-[#121214] text-[#D4D4D8] border-white/[0.08]">
                    <button
                      type="button"
                      class="pl-2 pr-0.5 py-1 flex items-center justify-center transition-colors"
                      onclick={() => testModel(prov, probeId)}
                      title={tStatus?.state === 'ok' ? `Available (${tStatus.latencyMs}ms)` : tStatus?.state === 'err' ? `Unavailable: ${tStatus.error || 'Error'}` : 'Test model availability'}
                    >
                      <span
                        class="w-2 h-2 rounded-full transition-colors {tStatus?.state === 'ok' ? 'bg-[#4ADE80]' : tStatus?.state === 'err' ? 'bg-[#F87171]' : tStatus?.state === 'testing' ? 'bg-[#EDEDEF] animate-pulse' : 'bg-[#52525B] hover:bg-[#A3A3AA]'}"
                      ></span>
                    </button>
                    <div class="pl-1.5 pr-2 py-1 font-mono text-[11px] flex items-center gap-1.5">
                      <span>{grp.displayId}</span>
                      {#if cap}
                        <span class="text-[9.5px] text-[#73737A]">{formatCtx(cap.contextWindow)}</span>
                        {#if variantLevels.length > 0}
                          <span class="text-[9.5px] text-[#8E8E95]">{variantLevels.map((l) => SHORT_REASONING_LABELS[l]).join('·')}</span>
                        {:else if cap.supportsReasoning && !isNonReasoningModelId(probeId)}
                          <span class="text-[9.5px] text-[#8E8E95]">Reasoning</span>
                        {/if}
                        {#if cap.supportsVision}
                          <span class="text-[9.5px] text-[#8E8E95]">Vision</span>
                        {/if}
                      {:else if variantLevels.length > 0}
                        <span class="text-[9.5px] text-[#8E8E95]">{variantLevels.map((l) => SHORT_REASONING_LABELS[l]).join('·')}</span>
                      {/if}
                    </div>
                    <button
                      type="button"
                      class="pr-2 pl-1 text-xs text-[#73737A] hover:text-[#EF4444] transition-colors"
                      onclick={() => {
                        for (const rawId of grp.rawIds) {
                          settingsStore.removeModelFromProvider(prov.id, rawId);
                        }
                      }}
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
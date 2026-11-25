import { invoke } from '@tauri-apps/api/core';

export type ReasoningLevel = 'OFF' | 'LOW' | 'MEDIUM' | 'HIGH' | 'MAX';

export const REASONING_ORDER: ReasoningLevel[] = ['OFF', 'LOW', 'MEDIUM', 'HIGH', 'MAX'];

const REASONING_SUFFIX_MAP: Record<string, ReasoningLevel> = {
  off: 'OFF',
  none: 'OFF',
  minimal: 'LOW',
  low: 'LOW',
  med: 'MEDIUM',
  medium: 'MEDIUM',
  high: 'HIGH',
  xhigh: 'MAX',
  max: 'MAX'
};

export interface GroupedProviderModel {
  displayId: string;
  rawIds: string[];
  reasoningVariants: Partial<Record<ReasoningLevel, string>>;
}

export function splitReasoningSuffix(modelId: string): { baseId: string; level: ReasoningLevel | null } {
  const lower = modelId.toLowerCase();
  for (const [suffix, level] of Object.entries(REASONING_SUFFIX_MAP)) {
    const tag = `-${suffix}`;
    if (lower.endsWith(tag) && modelId.length > tag.length) {
      return { baseId: modelId.slice(0, modelId.length - tag.length), level };
    }
  }
  return { baseId: modelId, level: null };
}

export function groupProviderModels(availableModels: string[]): GroupedProviderModel[] {
  const groups = new Map<string, GroupedProviderModel>();
  for (const rawId of availableModels) {
    const { baseId, level } = splitReasoningSuffix(rawId);
    let grp = groups.get(baseId);
    if (!grp) {
      grp = { displayId: baseId, rawIds: [], reasoningVariants: {} };
      groups.set(baseId, grp);
    }
    if (!grp.rawIds.includes(rawId)) {
      grp.rawIds.push(rawId);
    }
    if (level !== null && !grp.reasoningVariants[level]) {
      grp.reasoningVariants[level] = rawId;
    }
  }
  return Array.from(groups.values());
}

export function findModelGroup(
  prov: ProviderConfig | null | undefined,
  modelId: string
): GroupedProviderModel | null {
  if (!prov || !modelId) return null;
  const groups = groupProviderModels(prov.availableModels || []);
  return groups.find((g) => g.displayId === modelId || g.rawIds.includes(modelId)) || null;
}

export function resolveValidModelForProvider(
  prov: ProviderConfig | null | undefined,
  modelId: string | undefined
): string {
  if (!prov) return modelId || '';
  const avail = prov.availableModels || [];
  if (avail.length === 0) return prov.defaultModel || modelId || '';
  if (modelId && (avail.includes(modelId) || findModelGroup(prov, modelId) !== null)) {
    return modelId;
  }
  if (prov.defaultModel && (avail.includes(prov.defaultModel) || findModelGroup(prov, prov.defaultModel) !== null)) {
    return prov.defaultModel;
  }
  return avail[0];
}

export function getDisplayModelId(
  prov: ProviderConfig | null | undefined,
  modelId: string
): string {
  if (!modelId) return '';
  const grp = findModelGroup(prov, modelId);
  if (grp) return grp.displayId;
  return splitReasoningSuffix(modelId).baseId;
}

export function resolveModelForReasoning(
  prov: ProviderConfig | null | undefined,
  modelId: string,
  level: ReasoningLevel
): string {
  const grp = findModelGroup(prov, modelId);
  if (!grp) return modelId;
  const variantKeys = Object.keys(grp.reasoningVariants) as ReasoningLevel[];
  if (variantKeys.length === 0) return grp.rawIds[0] || modelId;
  if (grp.reasoningVariants[level]) return grp.reasoningVariants[level]!;
  const ordered = REASONING_ORDER.filter((l) => grp.reasoningVariants[l]);
  const targetIdx = REASONING_ORDER.indexOf(level);
  let best = ordered[0];
  let bestDist = Infinity;
  for (const cand of ordered) {
    const d = Math.abs(REASONING_ORDER.indexOf(cand) - targetIdx);
    if (d < bestDist) {
      bestDist = d;
      best = cand;
    }
  }
  return (best && grp.reasoningVariants[best]) || grp.rawIds[0] || modelId;
}

export function isImageGenerationModel(modelId: string): boolean {
  const m = modelId.toLowerCase();
  return (
    m.includes('-image') ||
    m.endsWith('image') ||
    m.includes('imagine-image') ||
    m.includes('imagen') ||
    m.includes('dall-e') ||
    m.includes('flux')
  );
}

export function isNonReasoningModelId(modelId: string): boolean {
  const m = modelId.toLowerCase();
  return (
    m.includes('-image') ||
    m.endsWith('image') ||
    m.includes('imagine-image') ||
    m.includes('imagine-video') ||
    m.includes('flux') ||
    m.includes('tts') ||
    m.includes('whisper') ||
    m.includes('embedding') ||
    m.includes('dall-e') ||
    m.includes('sora')
  );
}

export function getSupportedReasoningLevels(
  prov: ProviderConfig | null | undefined,
  modelId: string
): ReasoningLevel[] {
  if (!modelId) return [];
  const grp = findModelGroup(prov, modelId);
  if (grp) {
    const variantLevels = REASONING_ORDER.filter((l) => Boolean(grp.reasoningVariants[l]));
    if (variantLevels.length > 0) return variantLevels;
  }
  const probeId = grp?.rawIds[0] || modelId;
  if (isNonReasoningModelId(probeId)) return [];
  const detected = prov?.modelCapabilities?.[probeId] || prov?.modelCapabilities?.[modelId];
  const m = probeId.toLowerCase();
  const proto = (prov?.apiProtocol || '').toLowerCase();
  if (detected && !detected.supportsReasoning) return [];
  if (!detected && prov && !prov.supportsReasoning) {
    const heuristicReasoning =
      m.includes('reason') ||
      m.includes('thinking') ||
      m.includes('r1') ||
      m.includes('o1') ||
      m.includes('o3') ||
      m.includes('o4') ||
      m.includes('gpt-5') ||
      m.includes('gpt-6') ||
      m.includes('gemini-2.5') ||
      m.includes('gemini-3') ||
      m.includes('gemini-4') ||
      m.includes('claude') ||
      m.includes('glm-4.5') ||
      m.includes('glm-4.7') ||
      m.includes('glm-5');
    if (!heuristicReasoning) return [];
  }
  if (m.includes('gemini-3') || m.includes('gemini-4')) {
    return ['OFF', 'LOW', 'MEDIUM', 'HIGH'];
  }
  if (m.includes('claude') && m.includes('thinking')) {
    return ['LOW', 'MEDIUM', 'HIGH', 'MAX'];
  }
  if (m.includes('claude') || proto === 'claude_oauth' || proto === 'zcode_oauth' || m.includes('glm')) {
    return ['OFF', 'LOW', 'MEDIUM', 'HIGH', 'MAX'];
  }
  if (m.startsWith('o1') || m.startsWith('o3') || m.startsWith('o4')) {
    return ['LOW', 'MEDIUM', 'HIGH'];
  }
  if (proto === 'openai_codex' || m.includes('gpt-5') || m.includes('gpt-6')) {
    return ['LOW', 'MEDIUM', 'HIGH', 'MAX'];
  }
  if (m.includes('deepseek') && (m.includes('reason') || m.includes('r1'))) {
    return ['OFF', 'HIGH'];
  }
  return ['OFF', 'LOW', 'MEDIUM', 'HIGH', 'MAX'];
}

export interface DetectedModelInfo {
  id: string;
  name: string;
  contextWindow: number;
  maxOutputTokens: number;
  supportsReasoning: boolean;
  supportsVision: boolean;
  supportsTools: boolean;
  supportsAudio: boolean;
  supportsFiles: boolean;
}

export type ApiProtocol =
  | 'auto'
  | 'chat_completions'
  | 'responses'
  | 'anthropic_messages'
  | 'google_gemini'
  | 'antigravity'
  | 'openai_codex'
  | 'claude_oauth'
  | 'kimi_oauth'
  | 'xai_oauth'
  | 'zcode_oauth'
  | 'perplexity_oauth';

export interface ProviderConfig {
  id: string;
  name: string;
  baseUrl: string;
  apiKey: string;
  apiProtocol?: ApiProtocol;
  proxyEnabled?: boolean;
  proxyUrl?: string;
  defaultModel: string;
  availableModels: string[];
  modelCapabilities?: Record<string, DetectedModelInfo>;
  supportsReasoning: boolean;
  enabled: boolean;
}

export interface AppSettings {
  activeProvider: string;
  activeModel: string;
  reasoningLevel: ReasoningLevel;
  reasoningBudgetTokens: number;
  temperature: number;
  maxTokens: number;
  streamReasoning: boolean;
  systemPrompt: string;
  whisperModel: string;
  whisperLanguage: string;
  audioInputDevice: string;
  telegramBotToken: string;
  telegramPairingCode: string;
  telegramAllowedChats: string[];
  telegramActive: boolean;
  firstRunCompleted: boolean;
  enabledTools: Record<string, boolean>;
  providers: Record<string, ProviderConfig>;
}

const DEFAULT_SETTINGS: AppSettings = {
  activeProvider: '',
  activeModel: '',
  reasoningLevel: 'HIGH',
  reasoningBudgetTokens: 32000,
  temperature: 0.7,
  maxTokens: 8192,
  streamReasoning: true,
  systemPrompt: 'You are OmniHarness, an autonomous engineering intelligence operating directly on Windows.',
  whisperModel: 'ggml-large-v3-turbo',
  whisperLanguage: 'ru',
  audioInputDevice: 'default',
  telegramBotToken: '',
  telegramPairingCode: '',
  telegramAllowedChats: [],
  telegramActive: false,
  firstRunCompleted: true,
  enabledTools: {
    write_file: true,
    edit_file: true,
    read_file: true,
    list_directory: true,
    file_search: true,
    run_command: true,
    pty_spawn: true,
    pty_write: true,
    git_status: true,
    git_diff: true,
    git_commit: true,
    browser_open: true,
    browser_click: true,
    uia_inspect: true,
    uia_click: true,
    memory_save: true,
    memory_search: true,
    rollback_snapshot: true
  },
  providers: {}
};

class SettingsStore {
  data = $state<AppSettings>(JSON.parse(JSON.stringify(DEFAULT_SETTINGS)));
  isLoading = $state<boolean>(false);
  isSettingsOpen = $state<boolean>(false);
  isFirstRunOpen = $state<boolean>(false);
  private diskLoaded = false;
  private mutationVersion = 0;
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private saveChain: Promise<void> = Promise.resolve();
  private unlisteners: Array<() => void> = [];
  private disposed = false;

  currentProvider = $derived.by(() => {
    return this.data.providers[this.data.activeProvider] || null;
  });

  currentModelCapabilities = $derived.by(() => {
    const rawModel = this.data.activeModel || '';
    const model = resolveModelForReasoning(this.currentProvider, rawModel, this.data.reasoningLevel);
    const detected =
      this.currentProvider?.modelCapabilities?.[model] ||
      this.currentProvider?.modelCapabilities?.[rawModel];
    if (detected) {
      const mLow = model.toLowerCase();
      const isFaulty200k =
        detected.contextWindow === 200000 &&
        (mLow.includes('gemini') ||
          mLow.includes('sonnet-4') ||
          mLow.includes('sonnet-5') ||
          mLow.includes('sol') ||
          mLow.includes('astra'));
      const resolvedCtx = isFaulty200k ? (mLow.includes('gemini') ? 1048576 : 1000000) : detected.contextWindow;
      return {
        modelId: model,
        providerId: this.data.activeProvider,
        supportsReasoning: detected.supportsReasoning,
        contextWindow: resolvedCtx,
        supportsVision: detected.supportsVision,
        supportsTools: detected.supportsTools
      };
    }
    const isReasoner =
      model.includes('reason') ||
      model.includes('r1') ||
      model.startsWith('o1') ||
      model.startsWith('o3');

    return {
      modelId: model,
      providerId: this.data.activeProvider,
      supportsReasoning: !isNonReasoningModelId(model) && (isReasoner || Boolean(this.currentProvider?.supportsReasoning)),
      contextWindow: 128000,
      supportsVision: model.includes('4o') || model.includes('claude'),
      supportsTools: true
    };
  });

  constructor() {
    this.hydrateFromLocalStorageSync();
    this.init();
  }

  public destroy(): void {
    this.disposed = true;
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
    for (const fn of this.unlisteners) {
      try {
        fn();
      } catch {}
    }
    this.unlisteners = [];
  }

  private hydrateFromLocalStorageSync(): void {
    if (typeof localStorage === 'undefined') return;
    const saved = localStorage.getItem('omni_settings');
    if (!saved) return;
    try {
      const parsed = JSON.parse(saved);
      if (parsed && typeof parsed === 'object') {
        this.data = {
          ...DEFAULT_SETTINGS,
          ...parsed,
          providers: { ...(parsed.providers || {}) },
          enabledTools: { ...DEFAULT_SETTINGS.enabledTools, ...(parsed.enabledTools || {}) }
        };
      }
    } catch {}
  }

  private async init(): Promise<void> {
    await this.loadSettings();
    if (this.disposed) return;
    import('@tauri-apps/api/event')
      .then(async ({ listen }) => {
        if (this.disposed) return;
        const u1 = await listen<{ providerId?: string | null; modelId?: string | null; reasoningEffort?: string | null }>(
          'telegram-settings-updated',
          (ev) => {
            const p = ev.payload;
            if (!p) return;
            let changed = false;
            if (p.providerId && this.data.providers[p.providerId] && this.data.activeProvider !== p.providerId) {
              this.data.activeProvider = p.providerId;
              changed = true;
            }
            if (p.modelId && this.data.activeModel !== p.modelId) {
              this.data.activeModel = p.modelId;
              changed = true;
            }
            if (p.reasoningEffort) {
              const mapped = p.reasoningEffort.toUpperCase() as ReasoningLevel;
              if (['OFF', 'LOW', 'MEDIUM', 'HIGH', 'MAX'].includes(mapped) && this.data.reasoningLevel !== mapped) {
                this.data.reasoningLevel = mapped;
                changed = true;
              }
            }
            if (changed) {
              this.mutationVersion++;
              this.saveSettings();
            }
          }
        );
        if (this.disposed) {
          u1();
          return;
        }
        this.unlisteners.push(u1);

        const u2 = await listen<any>('telegram-providers-updated', (ev) => {
          const p = ev.payload;
          if (!p) return;
          if (p.action === 'add_provider' && p.provider) {
            const np = p.provider;
            this.data.providers[np.id] = {
              id: np.id,
              name: np.name,
              baseUrl: np.baseUrl || np.base_url || '',
              apiKey: np.apiKey || np.api_key || '',
              apiProtocol: np.apiProtocol || np.api_protocol || 'auto',
              proxyEnabled: Boolean(np.proxyEnabled),
              proxyUrl: np.proxyUrl || '',
              defaultModel: np.defaultModel || '',
              availableModels: np.availableModels || [],
              supportsReasoning: true,
              enabled: true
            };
            this.data.activeProvider = np.id;
            this.mutationVersion++;
            this.saveSettings();
          } else if (p.action === 'edit_provider' && p.provider) {
            const up = p.provider;
            const ex = this.data.providers[up.id];
            if (ex) {
              this.updateProvider(up.id, {
                baseUrl: up.baseUrl || up.base_url || ex.baseUrl,
                apiKey: up.apiKey || up.api_key || ex.apiKey
              });
            }
          } else if (p.action === 'add_models' && p.providerId && Array.isArray(p.models)) {
            const ex = this.data.providers[p.providerId];
            if (ex) {
              const merged = Array.from(new Set([...(ex.availableModels || []), ...p.models]));
              this.updateProvider(p.providerId, {
                availableModels: merged,
                defaultModel: ex.defaultModel || p.models[0] || ''
              });
              if (p.models[0]) {
                this.setActiveModel(p.providerId, p.models[0]);
              }
            }
          }
        });
        if (this.disposed) {
          u2();
          return;
        }
        this.unlisteners.push(u2);
      })
      .catch(() => {});

    this.refreshAllModelCapabilities();
    try {
      const cfg = await invoke<{ firstRunCompleted?: boolean; workspaceDir?: string | null }>('get_bootstrap_config');
      if (!this.disposed && cfg && cfg.firstRunCompleted === false) {
        this.data.firstRunCompleted = false;
        this.isFirstRunOpen = true;
      }
    } catch {}
  }

  public async loadSettings(): Promise<void> {
    const startVer = this.mutationVersion;
    this.isLoading = true;
    const localProviders = { ...$state.snapshot(this.data.providers || {}) };
    try {
      const res = await invoke<AppSettings | null>('get_settings');
      if (this.mutationVersion !== startVer) {
        this.diskLoaded = true;
        this.isLoading = false;
        this.syncProvidersToBackend();
        return;
      }
      if (res && typeof res === 'object') {
        const diskProviders = { ...(res.providers || {}) };
        const mergedProviders =
          Object.keys(diskProviders).length > 0
            ? { ...localProviders, ...diskProviders }
            : localProviders;
        const activeProvider =
          res.activeProvider && mergedProviders[res.activeProvider]
            ? res.activeProvider
            : this.data.activeProvider && mergedProviders[this.data.activeProvider]
              ? this.data.activeProvider
              : Object.values(mergedProviders).find((p) => p.enabled)?.id || '';
        const activeProvObj = activeProvider ? mergedProviders[activeProvider] : null;
        for (const prov of Object.values(mergedProviders)) {
          if (!prov.modelCapabilities) continue;
          for (const [mid, cap] of Object.entries(prov.modelCapabilities)) {
            const mLow = mid.toLowerCase();
            if (cap.contextWindow === 200000) {
              if (mLow.includes('gemini')) {
                cap.contextWindow = 1048576;
              } else if (
                mLow.includes('sonnet-4') ||
                mLow.includes('sonnet-5') ||
                mLow.includes('sol') ||
                mLow.includes('astra')
              ) {
                cap.contextWindow = 1000000;
              }
            }
          }
        }
        const rawActiveModel =
          res.activeModel ||
          this.data.activeModel ||
          activeProvObj?.defaultModel ||
          activeProvObj?.availableModels?.[0] ||
          '';
        const nextData: AppSettings = {
          ...DEFAULT_SETTINGS,
          ...$state.snapshot(this.data),
          ...res,
          activeProvider,
          activeModel: getDisplayModelId(activeProvObj, rawActiveModel),
          providers: mergedProviders,
          enabledTools: { ...DEFAULT_SETTINGS.enabledTools, ...(res.enabledTools || {}) }
        };
        this.data = nextData;
        if (typeof localStorage !== 'undefined') {
          try {
            localStorage.setItem('omni_settings', JSON.stringify(nextData));
          } catch {}
        }
        this.diskLoaded = true;
        this.isLoading = false;
        this.syncProvidersToBackend();
        if (Object.keys(diskProviders).length === 0 && Object.keys(mergedProviders).length > 0) {
          this.enqueueDiskSave();
        }
        return;
      }
    } catch (err) {
      console.error('Failed to load settings from disk:', err);
    }

    this.diskLoaded = true;
    this.isLoading = false;
    this.syncProvidersToBackend();
    if (Object.keys(this.data.providers || {}).length > 0) {
      this.enqueueDiskSave();
    }
  }

  public async refreshAllModelCapabilities(): Promise<void> {
    const startVer = this.mutationVersion;
    const provsSnapshot = Object.values($state.snapshot(this.data.providers || {}));
    const updates: Array<{ providerId: string; modelId: string; info: DetectedModelInfo }> = [];
    for (const prov of provsSnapshot) {
      const models = prov.availableModels || [];
      if (models.length === 0) continue;
      const caps = prov.modelCapabilities || {};
      for (const mid of models) {
        const prev = caps[mid];
        const mLow = mid.toLowerCase();
        const isCorrupted200k =
          prev &&
          prev.contextWindow === 200000 &&
          (mLow.includes('gemini') ||
            mLow.includes('sonnet-4') ||
            mLow.includes('sonnet-5') ||
            mLow.includes('sol') ||
            mLow.includes('astra'));
        if (prev && prev.contextWindow > 0 && !isCorrupted200k) continue;
        try {
          const detected = await invoke<DetectedModelInfo>('detect_model_capabilities', { modelId: mid });
          updates.push({ providerId: prov.id, modelId: mid, info: detected });
        } catch {}
      }
    }
    if (this.disposed || updates.length === 0) return;
    let applied = false;
    for (const u of updates) {
      const liveProv = this.data.providers[u.providerId];
      if (!liveProv) continue;
      if (!liveProv.modelCapabilities) {
        liveProv.modelCapabilities = {};
      }
      if (!liveProv.modelCapabilities[u.modelId] || this.mutationVersion === startVer) {
        liveProv.modelCapabilities[u.modelId] = u.info;
        applied = true;
      }
    }
    if (applied) {
      await this.saveSettings();
    }
  }

  public syncProvidersToBackend(): void {
    const snap = $state.snapshot(this.data);
    invoke('sync_providers_to_backend', {
      providers: Object.values(snap.providers || {}),
      activeProvider: snap.activeProvider || '',
      activeModel: snap.activeModel || ''
    }).catch(() => {});
  }

  private enqueueDiskSave(): Promise<void> {
    const snapshot = $state.snapshot(this.data);
    if (typeof localStorage !== 'undefined') {
      try {
        localStorage.setItem('omni_settings', JSON.stringify(snapshot));
      } catch {}
    }
    this.syncProvidersToBackend();
    this.saveChain = this.saveChain
      .then(async () => {
        const latest = $state.snapshot(this.data);
        await invoke('save_settings', { settings: latest });
      })
      .catch((err) => {
        console.error('Failed to save settings:', err);
      });
    return this.saveChain;
  }

  private pendingSaveResolvers: Array<() => void> = [];

  private flushSaveResolvers(): void {
    const list = this.pendingSaveResolvers.splice(0);
    for (const r of list) r();
  }

  public async saveSettings(immediate = false): Promise<void> {
    this.mutationVersion++;
    if (!this.diskLoaded) return;
    if (typeof localStorage !== 'undefined') {
      try {
        localStorage.setItem('omni_settings', JSON.stringify($state.snapshot(this.data)));
      } catch {}
    }
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
    }
    if (immediate) {
      await this.enqueueDiskSave();
      this.flushSaveResolvers();
      return;
    }
    await new Promise<void>((resolve) => {
      this.pendingSaveResolvers.push(resolve);
      this.saveTimer = setTimeout(() => {
        this.saveTimer = null;
        this.enqueueDiskSave().finally(() => this.flushSaveResolvers());
      }, 220);
    });
  }

  public flushPendingSaves(): void {
    if (!this.diskLoaded) return;
    if (this.saveTimer) {
      clearTimeout(this.saveTimer);
      this.saveTimer = null;
      void this.enqueueDiskSave().finally(() => this.flushSaveResolvers());
    }
  }

  public addProvider(provider: ProviderConfig): void {
    this.data.providers[provider.id] = provider;
    if (!this.data.activeProvider) {
      this.data.activeProvider = provider.id;
      this.data.activeModel = getDisplayModelId(provider, provider.defaultModel || provider.availableModels[0] || '');
    }
    void this.saveSettings();
  }

  public updateProvider(providerId: string, patch: Partial<ProviderConfig>): void {
    const existing = this.data.providers[providerId];
    if (!existing) return;
    Object.assign(existing, patch);
    if (
      this.data.activeProvider === providerId &&
      existing.availableModels.length > 0 &&
      !findModelGroup(existing, this.data.activeModel)
    ) {
      this.data.activeModel = getDisplayModelId(existing, existing.defaultModel || existing.availableModels[0]);
    }
    void this.saveSettings();
  }

  public addModelToProvider(providerId: string, model: DetectedModelInfo): void {
    const prov = this.data.providers[providerId];
    if (!prov) return;
    if (!prov.availableModels.includes(model.id)) {
      prov.availableModels.push(model.id);
    }
    if (!prov.modelCapabilities) {
      prov.modelCapabilities = {};
    }
    prov.modelCapabilities[model.id] = model;
    if (!prov.defaultModel) prov.defaultModel = model.id;
    if (model.supportsReasoning) prov.supportsReasoning = true;
    if (!this.data.activeModel) {
      this.data.activeProvider = providerId;
      this.data.activeModel = getDisplayModelId(prov, model.id);
    }
    void this.saveSettings();
  }

  public removeModelFromProvider(providerId: string, modelId: string): void {
    const prov = this.data.providers[providerId];
    if (!prov) return;
    prov.availableModels = prov.availableModels.filter((m) => m !== modelId);
    if (prov.modelCapabilities) {
      delete prov.modelCapabilities[modelId];
    }
    if (this.data.activeProvider === providerId && !findModelGroup(prov, this.data.activeModel)) {
      this.data.activeModel = getDisplayModelId(prov, prov.availableModels[0] || '');
    }
    void this.saveSettings();
  }

  public removeProvider(providerId: string): void {
    delete this.data.providers[providerId];
    if (this.data.activeProvider === providerId) {
      const first = Object.values(this.data.providers).find((p) => p.enabled);
      this.data.activeProvider = first ? first.id : '';
      this.data.activeModel = first ? getDisplayModelId(first, first.defaultModel || first.availableModels[0] || '') : '';
    }
    void this.saveSettings();
  }

  public setActiveModel(providerId: string, modelId: string): void {
    const prov = this.data.providers[providerId];
    this.data.activeProvider = providerId;
    this.data.activeModel = getDisplayModelId(prov, modelId);
    void this.saveSettings();
  }

  public setReasoningLevel(level: ReasoningLevel): void {
    this.data.reasoningLevel = level;
    switch (level) {
      case 'OFF':
        this.data.reasoningBudgetTokens = 0;
        break;
      case 'LOW':
        this.data.reasoningBudgetTokens = 8000;
        break;
      case 'MEDIUM':
        this.data.reasoningBudgetTokens = 16000;
        break;
      case 'HIGH':
        this.data.reasoningBudgetTokens = 32000;
        break;
      case 'MAX':
        this.data.reasoningBudgetTokens = 64000;
        break;
    }
    void this.saveSettings();
  }

  public setProvider(providerId: string): void {
    if (this.data.providers[providerId]) {
      this.data.activeProvider = providerId;
      const prov = this.data.providers[providerId];
      const grp = findModelGroup(prov, this.data.activeModel);
      if (prov.availableModels.length > 0 && !grp) {
        this.data.activeModel = getDisplayModelId(prov, prov.defaultModel || prov.availableModels[0]);
      }
      void this.saveSettings();
    }
  }

  public setModel(modelId: string): void {
    const prov = this.data.providers[this.data.activeProvider];
    this.data.activeModel = getDisplayModelId(prov, modelId);
    void this.saveSettings();
  }

  public setApiKey(providerId: string, apiKey: string): void {
    if (this.data.providers[providerId]) {
      this.data.providers[providerId].apiKey = apiKey;
      void this.saveSettings();
    }
  }

  public openSettings(): void {
    this.isSettingsOpen = true;
  }

  public closeSettings(): void {
    this.isSettingsOpen = false;
  }

  public openFirstRun(): void {
    this.isFirstRunOpen = true;
  }

  public closeFirstRun(): void {
    this.isFirstRunOpen = false;
  }
}

export const settingsStore = new SettingsStore();

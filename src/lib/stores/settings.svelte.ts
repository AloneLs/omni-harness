import { invoke } from '@tauri-apps/api/core';

export type ReasoningLevel = 'OFF' | 'LOW' | 'MEDIUM' | 'HIGH' | 'MAX';

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

export type ApiProtocol = 'auto' | 'chat_completions' | 'responses' | 'anthropic_messages' | 'google_gemini' | 'antigravity' | 'openai_codex' | 'claude_oauth' | 'kimi_oauth' | 'xai_oauth' | 'zcode_oauth' | 'perplexity_oauth';

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

  currentProvider = $derived.by(() => {
    return this.data.providers[this.data.activeProvider] || null;
  });

  currentModelCapabilities = $derived.by(() => {
    const model = this.data.activeModel || '';
    const detected = this.currentProvider?.modelCapabilities?.[model];
    if (detected) {
      return {
        modelId: model,
        providerId: this.data.activeProvider,
        supportsReasoning: detected.supportsReasoning,
        contextWindow: detected.contextWindow,
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
      supportsReasoning: isReasoner || Boolean(this.currentProvider?.supportsReasoning),
      contextWindow: 128000,
      supportsVision: model.includes('4o') || model.includes('claude'),
      supportsTools: true
    };
  });

  constructor() {
    this.init();
  }

  private async init(): Promise<void> {
    await this.loadSettings();
    import('@tauri-apps/api/event')
      .then(({ listen }) => {
        listen<{ providerId?: string | null; modelId?: string | null; reasoningEffort?: string | null }>(
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
              this.data = { ...this.data };
              if (typeof localStorage !== 'undefined') {
                localStorage.setItem('omni_settings', JSON.stringify(this.data));
              }
              invoke('save_settings', { settings: this.data }).catch(() => {});
            }
          }
        );
      })
      .catch(() => {});
    this.refreshAllModelCapabilities();
    try {
      const cfg = await invoke<{ firstRunCompleted?: boolean; workspaceDir?: string | null }>('get_bootstrap_config');
      if (cfg && cfg.firstRunCompleted === false) {
        this.data.firstRunCompleted = false;
        this.isFirstRunOpen = true;
      }
    } catch {}
  }

  public async loadSettings(): Promise<void> {
    this.isLoading = true;
    try {
      const res = await invoke<AppSettings>('get_settings');
      if (res && typeof res === 'object') {
        this.data = {
          ...DEFAULT_SETTINGS,
          ...res,
          providers: { ...(res.providers || {}) },
          enabledTools: { ...DEFAULT_SETTINGS.enabledTools, ...(res.enabledTools || {}) }
        };
        this.isLoading = false;
        return;
      }
    } catch {}

    if (typeof localStorage !== 'undefined') {
      const saved = localStorage.getItem('omni_settings');
      if (saved) {
        try {
          const parsed = JSON.parse(saved);
          this.data = {
            ...DEFAULT_SETTINGS,
            ...parsed,
            providers: { ...(parsed.providers || {}) },
            enabledTools: { ...DEFAULT_SETTINGS.enabledTools, ...(parsed.enabledTools || {}) }
          };
        } catch {}
      }
    }
    this.isLoading = false;
    this.syncProvidersToBackend();
  }

  public async refreshAllModelCapabilities(): Promise<void> {
    const provs = Object.values(this.data.providers || {});
    let changed = false;
    for (const prov of provs) {
      const models = prov.availableModels || [];
      if (models.length === 0) continue;
      const nextCaps: Record<string, DetectedModelInfo> = { ...(prov.modelCapabilities || {}) };
      for (const mid of models) {
        try {
          const detected = await invoke<DetectedModelInfo>('detect_model_capabilities', { modelId: mid });
          const prev = nextCaps[mid];
          if (
            !prev ||
            prev.contextWindow !== detected.contextWindow ||
            prev.maxOutputTokens !== detected.maxOutputTokens ||
            prev.supportsReasoning !== detected.supportsReasoning ||
            prev.supportsVision !== detected.supportsVision ||
            prev.supportsTools !== detected.supportsTools
          ) {
            nextCaps[mid] = detected;
            changed = true;
          }
        } catch {}
      }
      prov.modelCapabilities = nextCaps;
    }
    if (changed) {
      this.data.providers = { ...this.data.providers };
      await this.saveSettings();
    }
  }

  public syncProvidersToBackend(): void {
    invoke('sync_providers_to_backend', {
      providers: Object.values(this.data.providers || {}),
      activeProvider: this.data.activeProvider || '',
      activeModel: this.data.activeModel || ''
    }).catch(() => {});
  }

  public async saveSettings(): Promise<void> {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('omni_settings', JSON.stringify(this.data));
    }
    this.syncProvidersToBackend();
    try {
      await invoke('save_settings', { settings: this.data });
    } catch {}
  }

  public addProvider(provider: ProviderConfig): void {
    this.data.providers = {
      ...this.data.providers,
      [provider.id]: provider
    };
    if (!this.data.activeProvider) {
      this.data.activeProvider = provider.id;
      this.data.activeModel = provider.defaultModel || provider.availableModels[0] || '';
    }
    this.saveSettings();
  }

  public updateProvider(providerId: string, patch: Partial<ProviderConfig>): void {
    const existing = this.data.providers[providerId];
    if (!existing) return;
    const updated: ProviderConfig = {
      ...existing,
      ...patch
    };
    this.data.providers = {
      ...this.data.providers,
      [providerId]: updated
    };
    if (this.data.activeProvider === providerId && updated.availableModels.length > 0 && !updated.availableModels.includes(this.data.activeModel)) {
      this.data.activeModel = updated.defaultModel || updated.availableModels[0];
    }
    this.saveSettings();
  }

  public addModelToProvider(providerId: string, model: DetectedModelInfo): void {
    const prov = this.data.providers[providerId];
    if (!prov) return;
    if (!prov.availableModels.includes(model.id)) {
      prov.availableModels = [...prov.availableModels, model.id];
    }
    prov.modelCapabilities = {
      ...(prov.modelCapabilities || {}),
      [model.id]: model
    };
    if (!prov.defaultModel) prov.defaultModel = model.id;
    if (model.supportsReasoning) prov.supportsReasoning = true;
    if (!this.data.activeModel) {
      this.data.activeProvider = providerId;
      this.data.activeModel = model.id;
    }
    this.data.providers = { ...this.data.providers };
    this.saveSettings();
  }

  public removeModelFromProvider(providerId: string, modelId: string): void {
    const prov = this.data.providers[providerId];
    if (!prov) return;
    prov.availableModels = prov.availableModels.filter((m) => m !== modelId);
    if (prov.modelCapabilities) {
      const nextCaps = { ...prov.modelCapabilities };
      delete nextCaps[modelId];
      prov.modelCapabilities = nextCaps;
    }
    if (this.data.activeProvider === providerId && this.data.activeModel === modelId) {
      this.data.activeModel = prov.availableModels[0] || '';
    }
    this.data.providers = { ...this.data.providers };
    this.saveSettings();
  }

  public removeProvider(providerId: string): void {
    const next = { ...this.data.providers };
    delete next[providerId];
    this.data.providers = next;
    if (this.data.activeProvider === providerId) {
      const first = Object.values(next).find((p) => p.enabled);
      this.data.activeProvider = first ? first.id : '';
      this.data.activeModel = first ? (first.defaultModel || first.availableModels[0] || '') : '';
    }
    this.saveSettings();
  }

  public setActiveModel(providerId: string, modelId: string): void {
    this.data.activeProvider = providerId;
    this.data.activeModel = modelId;
    this.saveSettings();
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
    this.saveSettings();
  }

  public setProvider(providerId: string): void {
    if (this.data.providers[providerId]) {
      this.data.activeProvider = providerId;
      const prov = this.data.providers[providerId];
      if (prov.availableModels.length > 0 && !prov.availableModels.includes(this.data.activeModel)) {
        this.data.activeModel = prov.defaultModel || prov.availableModels[0];
      }
      this.saveSettings();
    }
  }

  public setModel(modelId: string): void {
    this.data.activeModel = modelId;
    this.saveSettings();
  }

  public setApiKey(providerId: string, apiKey: string): void {
    if (this.data.providers[providerId]) {
      this.data.providers[providerId].apiKey = apiKey;
      this.saveSettings();
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

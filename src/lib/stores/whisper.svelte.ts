import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface BackendWhisperModel {
  modelId: string;
  filename: string;
  sizeBytes: number;
  isDownloaded: boolean;
  localPath?: string | null;
  isDownloading?: boolean;
  downloadProgress?: number | null;
  downloadedBytes?: number | null;
  totalBytes?: number | null;
}

export interface WhisperModelInfo {
  id: string;
  name: string;
  filename: string;
  sizeFormatted: string;
  vramRequired: string;
  status: 'installed' | 'not_installed' | 'downloading';
  downloadProgress?: number;
  downloadedBytes?: number;
  totalBytes?: number;
  downloadSpeed?: string;
}

class WhisperStore {
  models = $state<WhisperModelInfo[]>([
    { id: 'large-v3-turbo', name: 'Whisper Large v3 Turbo', filename: 'ggml-large-v3-turbo.bin', sizeFormatted: '1.62 GB', vramRequired: '1.8 GB', status: 'not_installed' },
    { id: 'large-v3', name: 'Whisper Large v3 Full', filename: 'ggml-large-v3.bin', sizeFormatted: '3.10 GB', vramRequired: '3.5 GB', status: 'not_installed' },
    { id: 'medium', name: 'Whisper Medium', filename: 'ggml-medium.bin', sizeFormatted: '1.53 GB', vramRequired: '1.8 GB', status: 'not_installed' },
    { id: 'base', name: 'Whisper Base Fast', filename: 'ggml-base.bin', sizeFormatted: '148 MB', vramRequired: '500 MB', status: 'not_installed' }
  ]);

  statusError = $state<string | null>(null);
  justInstalledId = $state<string | null>(null);

  get hasAnyInstalled(): boolean {
    return this.models.some((m) => m.status === 'installed');
  }

  private speedTracker: Record<string, { lastBytes: number; lastTs: number; speedMb: number }> = {};
  private listenersInitialized = false;

  constructor() {
    this.initGlobalListeners();
    this.refresh();
  }

  private initGlobalListeners() {
    if (this.listenersInitialized || typeof window === 'undefined') return;
    this.listenersInitialized = true;

    window.addEventListener('focus', () => {
      this.refresh();
    });

    listen<{ modelId: string; downloaded: number; total: number; percent: number }>(
      'whisper-download-progress',
      (ev) => {
        const { modelId, downloaded, total, percent } = ev.payload || {};
        if (!modelId) return;

        const now = Date.now();
        const tracker = this.speedTracker[modelId] || { lastBytes: downloaded, lastTs: now, speedMb: 0 };
        const dt = Math.max(0.15, (now - tracker.lastTs) / 1000);
        const speedMb = Math.max(0, (downloaded - tracker.lastBytes) / (1024 * 1024) / dt);

        this.speedTracker[modelId] = { lastBytes: downloaded, lastTs: now, speedMb };

        const speedText = speedMb > 0 ? `${speedMb.toFixed(1)} MB/s` : 'Downloading...';

        this.models = this.models.map((m) =>
          m.id === modelId
            ? {
                ...m,
                status: 'downloading',
                downloadProgress: Math.min(100, Math.max(0, percent)),
                downloadedBytes: downloaded,
                totalBytes: total,
                downloadSpeed: speedText
              }
            : m
        );
      }
    ).catch(() => {});

    listen<{ modelId: string; status: string; error?: string }>(
      'whisper-download-finished',
      (ev) => {
        const { modelId, status, error } = ev.payload || {};
        if (!modelId) return;

        delete this.speedTracker[modelId];

        if (status === 'installed') {
          this.models = this.models.map((m) =>
            m.id === modelId
              ? { ...m, status: 'installed', downloadProgress: undefined, downloadSpeed: undefined }
              : m
          );
          this.justInstalledId = modelId;
          setTimeout(() => {
            if (this.justInstalledId === modelId) this.justInstalledId = null;
          }, 1200);
          window.dispatchEvent(new CustomEvent('omni-whisper-updated'));
        } else if (status === 'error') {
          this.models = this.models.map((m) =>
            m.id === modelId
              ? { ...m, status: 'not_installed', downloadProgress: undefined, downloadSpeed: undefined }
              : m
          );
          this.statusError = error || 'Download failed';
        }
        this.refresh();
      }
    ).catch(() => {});
  }

  async refresh() {
    try {
      const res = await invoke<BackendWhisperModel[]>('get_speech_status');
      if (Array.isArray(res)) {
        const statusMap = new Map<string, BackendWhisperModel>();
        for (const item of res) {
          const id = item.modelId || '';
          if (id) statusMap.set(id, item);
        }

        this.models = this.models.map((m) => {
          const backend = statusMap.get(m.id);
          if (!backend) return m;

          const isInstalled = Boolean(backend.isDownloaded);
          const isDownloading = Boolean(backend.isDownloading);

          if (isDownloading) {
            const pct = backend.downloadProgress ?? m.downloadProgress ?? 0;
            return {
              ...m,
              status: 'downloading',
              downloadProgress: pct,
              downloadedBytes: backend.downloadedBytes ?? m.downloadedBytes,
              totalBytes: backend.totalBytes ?? m.totalBytes,
              downloadSpeed: m.downloadSpeed || 'Downloading...'
            };
          }

          if (m.status === 'downloading' && !isInstalled) {
            return m;
          }

          return {
            ...m,
            status: isInstalled ? 'installed' : 'not_installed',
            downloadProgress: undefined,
            downloadSpeed: undefined
          };
        });
        window.dispatchEvent(new CustomEvent('omni-whisper-updated'));
      }
    } catch (err: any) {
      this.statusError = String(err || 'Failed to read Whisper status');
    }
  }

  async download(modelId: string) {
    this.statusError = null;
    this.models = this.models.map((m) =>
      m.id === modelId
        ? { ...m, status: 'downloading', downloadProgress: 0, downloadSpeed: 'Starting...' }
        : m
    );

    try {
      await invoke('download_whisper_model', { modelId });
      this.models = this.models.map((m) =>
        m.id === modelId
          ? { ...m, status: 'installed', downloadProgress: undefined, downloadSpeed: undefined }
          : m
      );
      this.justInstalledId = modelId;
      setTimeout(() => {
        if (this.justInstalledId === modelId) this.justInstalledId = null;
      }, 1200);
      await this.refresh();
      window.dispatchEvent(new CustomEvent('omni-whisper-updated'));
    } catch (err: any) {
      this.models = this.models.map((m) =>
        m.id === modelId
          ? { ...m, status: 'not_installed', downloadProgress: undefined, downloadSpeed: undefined }
          : m
      );
      this.statusError = String(err || 'Download failed');
    }
  }

  async delete(modelId: string) {
    this.statusError = null;
    try {
      await invoke('delete_whisper_model', { modelId });
      this.models = this.models.map((m) =>
        m.id === modelId ? { ...m, status: 'not_installed' } : m
      );
      await this.refresh();
      window.dispatchEvent(new CustomEvent('omni-whisper-updated'));
    } catch (err: any) {
      this.statusError = String(err || 'Delete failed');
    }
  }
}

export const whisperStore = new WhisperStore();
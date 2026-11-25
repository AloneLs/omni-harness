import { invoke } from '@tauri-apps/api/core';

export interface WorkspaceFile {
  path: string;
  name: string;
  isDirectory: boolean;
  size?: number;
  extension?: string;
  children?: WorkspaceFile[];
}

export interface SearchResult {
  path: string;
  name: string;
  matchLine?: number;
  matchSnippet?: string;
  isDirectory: boolean;
}

class WorkspaceStore {
  currentPath = $state<string>('');
  recentWorkspaces = $state<string[]>([]);
  files = $state.raw<WorkspaceFile[]>([]);
  activeFilePath = $state<string | null>(null);
  searchQuery = $state<string>('');
  searchResults = $state.raw<SearchResult[]>([]);
  isLoading = $state<boolean>(false);

  private refreshReqId = 0;
  private searchReqId = 0;
  private searchTimer: ReturnType<typeof setTimeout> | null = null;

  workspaceName = $derived.by(() => {
    if (!this.currentPath) {
      return '';
    }
    const normalized = this.currentPath.replace(/[\\/]+$/, '');
    const parts = normalized.split(/[\\/]/);
    return parts[parts.length - 1] || normalized;
  });

  totalFiles = $derived.by(() => {
    let count = 0;
    const countRec = (list: WorkspaceFile[]) => {
      for (const f of list) {
        if (!f.isDirectory) count++;
        if (f.children && f.children.length > 0) countRec(f.children);
      }
    };
    countRec(this.files);
    return count;
  });

  constructor() {
    this.initFromLocalStorage();
  }

  private initFromLocalStorage(): void {
    if (typeof localStorage === 'undefined') return;
    const saved = localStorage.getItem('omni_recent_workspaces');
    if (saved) {
      try {
        const parsed = JSON.parse(saved);
        if (Array.isArray(parsed)) {
          this.recentWorkspaces = parsed;
        }
      } catch {}
    }
    const active = localStorage.getItem('omni_active_workspace');
    if (active) {
      this.currentPath = active;
      void this.refreshFiles();
    }
    this.syncWorkspacesToBackend();
  }

  public syncWorkspacesToBackend(): void {
    const payload = this.recentWorkspaces.map((p) => ({
      name: p.split(/[\\/]/).filter(Boolean).pop() || p,
      path: p
    }));
    invoke('sync_workspaces_to_backend', { workspaces: payload }).catch(() => {});
  }

  public async setWorkspaceDir(path: string): Promise<void> {
    this.currentPath = path;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('omni_active_workspace', path);
    }
    if (path) {
      this.addRecentWorkspace(path);
    }
    try {
      await invoke('set_workspace_dir', { path });
    } catch {}
    if (path) {
      await this.refreshFiles();
    } else {
      this.refreshReqId++;
      this.files = [];
      this.isLoading = false;
    }
  }

  public async clearWorkspace(): Promise<void> {
    await this.setWorkspaceDir('');
  }

  public addRecentWorkspace(path: string): void {
    const norm = path.replace(/[\\/]+$/, '').toLowerCase();
    if (this.recentWorkspaces.some((p) => p.replace(/[\\/]+$/, '').toLowerCase() === norm)) {
      return;
    }
    const list = [path, ...this.recentWorkspaces].slice(0, 30);
    this.recentWorkspaces = list;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('omni_recent_workspaces', JSON.stringify(list));
    }
    this.syncWorkspacesToBackend();
  }

  public removeRecentWorkspace(path: string): void {
    const norm = path.replace(/[\\/]+$/, '').toLowerCase();
    this.recentWorkspaces = this.recentWorkspaces.filter(
      (p) => p.replace(/[\\/]+$/, '').toLowerCase() !== norm
    );
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('omni_recent_workspaces', JSON.stringify(this.recentWorkspaces));
    }
    if (this.currentPath.replace(/[\\/]+$/, '').toLowerCase() === norm) {
      this.clearWorkspace().catch(() => {});
    }
    this.syncWorkspacesToBackend();
  }

  public async openDirectoryPicker(): Promise<string | null> {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select Project Directory'
      });
      if (typeof selected === 'string' && selected) {
        await this.setWorkspaceDir(selected);
        return selected;
      }
    } catch {
      try {
        const selected = await invoke<string | null>('open_directory_dialog');
        if (selected) {
          await this.setWorkspaceDir(selected);
          return selected;
        }
      } catch {
        const promptPath = window.prompt('Enter project directory path:', this.currentPath || '');
        if (promptPath) {
          await this.setWorkspaceDir(promptPath);
          return promptPath;
        }
      }
    }
    return null;
  }

  public async refreshFiles(): Promise<void> {
    const targetPath = this.currentPath;
    if (!targetPath) {
      this.refreshReqId++;
      this.files = [];
      this.isLoading = false;
      return;
    }
    const reqId = ++this.refreshReqId;
    this.isLoading = true;
    try {
      const result = await invoke<WorkspaceFile[]>('list_workspace_files', { path: targetPath });
      if (reqId !== this.refreshReqId || this.currentPath !== targetPath) return;
      if (Array.isArray(result)) {
        this.files = result;
      }
    } catch {
      if (reqId !== this.refreshReqId || this.currentPath !== targetPath) return;
      this.files = [];
    } finally {
      if (reqId === this.refreshReqId) {
        this.isLoading = false;
      }
    }
  }

  public searchFilesDebounced(query: string, delayMs = 180): void {
    this.searchQuery = query;
    if (this.searchTimer) {
      clearTimeout(this.searchTimer);
      this.searchTimer = null;
    }
    const trimmed = query.trim();
    if (!trimmed || !this.currentPath) {
      this.searchReqId++;
      this.searchResults = [];
      return;
    }
    this.searchTimer = setTimeout(() => {
      this.searchTimer = null;
      void this.searchFiles(query);
    }, delayMs);
  }

  public async searchFiles(query: string): Promise<SearchResult[]> {
    this.searchQuery = query;
    const trimmed = query.trim();
    const targetPath = this.currentPath;
    const reqId = ++this.searchReqId;
    if (!trimmed || !targetPath) {
      this.searchResults = [];
      return [];
    }

    try {
      const results = await invoke<SearchResult[]>('search_workspace_files', {
        path: targetPath,
        query: trimmed
      });
      if (reqId !== this.searchReqId || this.currentPath !== targetPath) {
        return this.searchResults;
      }
      if (Array.isArray(results)) {
        this.searchResults = results;
        return results;
      }
    } catch {}

    if (reqId !== this.searchReqId || this.currentPath !== targetPath) {
      return this.searchResults;
    }

    const q = trimmed.toLowerCase();
    const matches: SearchResult[] = [];
    const searchRec = (list: WorkspaceFile[]) => {
      for (const f of list) {
        if (f.name.toLowerCase().includes(q) || f.path.toLowerCase().includes(q)) {
          matches.push({
            path: f.path,
            name: f.name,
            isDirectory: f.isDirectory,
            matchSnippet: f.path
          });
        }
        if (f.children && f.children.length > 0) searchRec(f.children);
      }
    };

    searchRec(this.files);
    this.searchResults = matches;
    return matches;
  }

  public selectFile(path: string | null): void {
    this.activeFilePath = path;
  }
}

export const workspaceStore = new WorkspaceStore();

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface OmniTheme {
  id: string;
  name: string;
  description: string;
  author: string;
  version: string;
  css: string;
  iconOverrides: Record<string, string>;
  textOverrides: Record<string, string>;
  assets: Record<string, string>;
  createdAt: number;
  updatedAt: number;
}

export function createEmptyTheme(name = 'New Custom Theme'): OmniTheme {
  const now = Date.now();
  return {
    id: `theme-${now.toString(36)}`,
    name,
    description: '',
    author: 'User',
    version: '1.0.0',
    css: `/* OmniHarness Theme: ${name} */
/* Selectors: [data-omni-app], [data-omni-sidebar], [data-omni-chat-view],
   [data-omni-message="user"], [data-omni-message="assistant"],
   [data-omni-timeline], [data-omni-composer], [data-omni-button="send"] */

[data-omni-app] {
  --color-bg-base: #0a0a0f;
  --color-bg-sidebar: #0e0e15;
  --color-accent-primary: #8b5cf6;
}
`,
    iconOverrides: {},
    textOverrides: {},
    assets: {},
    createdAt: now,
    updatedAt: now
  };
}

export function compileThemeCss(theme: OmniTheme | null, scopeSelector: string): string {
  if (!theme || !theme.css.trim()) return '';
  let css = theme.css;

  for (const [assetName, dataUri] of Object.entries(theme.assets || {})) {
    if (!assetName || !dataUri) continue;
    const token = `omni-asset://${assetName}`;
    css = css.split(token).join(dataUri);
  }

  css = css.replace(/\b(html|body|:root)\b/g, scopeSelector);

  const atRules: string[] = [];
  const stripped = css.replace(/@(font-face|keyframes|charset|import)\b[\s\S]*?(?:\{[\s\S]*?\}\s*\}|\{[^}]*\}|;)/gi, (match) => {
    atRules.push(match);
    return '';
  });

  return `${atRules.join('\n')}\n@scope (${scopeSelector}) to ([data-omni-studio-ui], [data-omni-preview-bar]) {\n${stripped}\n}`;
}

class ThemeStore {
  themes = $state<OmniTheme[]>([]);
  activeThemeId = $state<string | null>(null);
  editingTheme = $state<OmniTheme | null>(null);
  isStudioOpen = $state<boolean>(false);
  isFullscreenPreview = $state<boolean>(false);

  activeTheme = $derived.by(() => {
    if (!this.activeThemeId) return null;
    return this.themes.find((t) => t.id === this.activeThemeId) || null;
  });

  effectiveAppTheme = $derived.by(() => {
    if (this.isFullscreenPreview && this.editingTheme) {
      return this.editingTheme;
    }
    return this.activeTheme;
  });

  constructor() {
    this.init();
  }

  private async init(): Promise<void> {
    await this.reload();
    listen<OmniTheme>('theme-studio-patch', (ev) => {
      const updated = ev.payload;
      if (!updated || !updated.id) return;
      const idx = this.themes.findIndex((t) => t.id === updated.id);
      if (idx >= 0) {
        this.themes[idx] = updated;
        this.themes = [...this.themes];
      } else {
        this.themes = [updated, ...this.themes];
      }
      if (this.editingTheme?.id === updated.id) {
        this.editingTheme = updated;
      }
    }).catch(() => {});

    if (typeof window !== 'undefined') {
      window.addEventListener('keydown', (e) => {
        if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'r') {
          e.preventDefault();
          this.isFullscreenPreview = false;
          this.setActiveTheme(null);
        }
      });
    }
  }

  public async reload(): Promise<void> {
    try {
      const [list, activeId] = await Promise.all([
        invoke<OmniTheme[]>('list_themes'),
        invoke<string | null>('get_active_theme_id')
      ]);
      if (Array.isArray(list)) {
        this.themes = list;
      }
      this.activeThemeId = activeId || null;
    } catch {}
  }

  public async saveTheme(theme: OmniTheme): Promise<OmniTheme> {
    const saved = await invoke<OmniTheme>('save_theme', { theme });
    const idx = this.themes.findIndex((t) => t.id === saved.id);
    if (idx >= 0) {
      this.themes[idx] = saved;
      this.themes = [...this.themes];
    } else {
      this.themes = [saved, ...this.themes];
    }
    if (this.editingTheme?.id === saved.id) {
      this.editingTheme = saved;
    }
    return saved;
  }

  public async deleteTheme(id: string): Promise<void> {
    await invoke('delete_theme', { id });
    this.themes = this.themes.filter((t) => t.id !== id);
    if (this.activeThemeId === id) {
      this.activeThemeId = null;
    }
    if (this.editingTheme?.id === id) {
      this.editingTheme = this.themes[0] || null;
    }
  }

  public async setActiveTheme(id: string | null): Promise<void> {
    this.activeThemeId = id;
    try {
      await invoke('set_active_theme_id', { id });
    } catch {}
  }

  public openStudio(themeId?: string | null): void {
    const target = themeId
      ? this.themes.find((t) => t.id === themeId) || null
      : this.activeTheme || this.themes[0] || null;
    this.editingTheme = target ? JSON.parse(JSON.stringify(target)) : createEmptyTheme('Midnight Glass');
    this.isStudioOpen = true;
    this.isFullscreenPreview = false;
  }

  public closeStudio(): void {
    this.isFullscreenPreview = false;
    this.isStudioOpen = false;
  }

  public t(key: string, fallback: string, previewOnly = false): string {
    const src = previewOnly ? this.editingTheme : this.effectiveAppTheme;
    const val = src?.textOverrides?.[key];
    return val && val.trim() ? val : fallback;
  }

  public resolveIconOverride(name: string, previewOnly = false): string | null {
    const src = previewOnly ? this.editingTheme : this.effectiveAppTheme;
    const raw = src?.iconOverrides?.[name];
    return raw && raw.trim() ? raw.trim() : null;
  }

  public exportThemeJson(theme: OmniTheme): string {
    return JSON.stringify(theme, null, 2);
  }

  public async importThemeJson(rawJson: string): Promise<OmniTheme> {
    const parsed = JSON.parse(rawJson);
    if (!parsed || typeof parsed !== 'object' || typeof parsed.name !== 'string') {
      throw new Error('Invalid theme JSON format');
    }
    const now = Date.now();
    const theme: OmniTheme = {
      id: typeof parsed.id === 'string' && parsed.id.trim() ? parsed.id.trim() : `theme-${now.toString(36)}`,
      name: parsed.name.trim() || 'Imported Theme',
      description: String(parsed.description || ''),
      author: String(parsed.author || 'Imported'),
      version: String(parsed.version || '1.0.0'),
      css: String(parsed.css || ''),
      iconOverrides: parsed.iconOverrides && typeof parsed.iconOverrides === 'object' ? parsed.iconOverrides : {},
      textOverrides: parsed.textOverrides && typeof parsed.textOverrides === 'object' ? parsed.textOverrides : {},
      assets: parsed.assets && typeof parsed.assets === 'object' ? parsed.assets : {},
      createdAt: Number(parsed.createdAt) || now,
      updatedAt: now
    };
    return await this.saveTheme(theme);
  }
}

export const themeStore = new ThemeStore();

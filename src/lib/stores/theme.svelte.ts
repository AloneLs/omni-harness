import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { omniBridge } from '../runtime/omniBridge';

export interface OmniTheme {
  id: string;
  name: string;
  description: string;
  author: string;
  version: string;
  css: string;
  rawCss?: string;
  customJs?: string;
  iconOverrides: Record<string, string>;
  textOverrides: Record<string, string>;
  assets: Record<string, string>;
  createdAt: number;
  updatedAt: number;
}

export const DEFAULT_OMNI_CSS = `[data-omni-app] {
  --font-sans: "Inter", "Segoe UI", -apple-system, BlinkMacSystemFont, Roboto, sans-serif;
  --font-mono: "JetBrains Mono", "Cascadia Code", "Consolas", monospace;
  --color-bg-base: #101012;
  --color-bg-sidebar: #171719;
  --color-bg-surface: #1B1B1E;
  --color-bg-card: #1E1E22;
  --color-bg-elevated: #242428;
  --color-bg-hover: #28282D;
  --color-border-subtle: rgba(255, 255, 255, 0.06);
  --color-border-default: rgba(255, 255, 255, 0.08);
  --color-text-primary: #E5E5E8;
  --color-text-secondary: #A3A3AA;
  --color-text-muted: #66666D;
  background-color: var(--color-bg-base);
  color: var(--color-text-primary);
  font-family: var(--font-sans);
}

[data-omni-titlebar] {
  background-color: var(--color-bg-base);
  border-bottom: 1px solid var(--color-border-subtle);
}

[data-omni-sidebar] {
  background-color: var(--color-bg-sidebar);
  border-color: var(--color-border-subtle);
}

[data-omni-sidebar-header] {
  border-color: var(--color-border-subtle);
}

[data-omni-chat-list] {
  scrollbar-color: rgba(255, 255, 255, 0.1) transparent;
}

[data-omni-chat-item] {
  border-radius: 6px;
}

[data-omni-chat-view] {
  background-color: var(--color-bg-base);
}

[data-omni-message="user"] [data-omni-message-bubble] {
  background-color: var(--color-bg-card);
  border-color: rgba(255, 255, 255, 0.07);
}

[data-omni-timeline] {
  border-color: var(--color-border-subtle);
}

[data-omni-composer-box] {
  background-color: rgba(28, 28, 31, 0.95);
  border-color: var(--color-border-default);
}

[data-omni-button="send"] {
  border-radius: 4px;
}
`;

export function createEmptyTheme(name = 'New Theme'): OmniTheme {
  const now = Date.now();
  return {
    id: `theme-${now.toString(36)}`,
    name,
    description: '',
    author: 'User',
    version: '1.0.0',
    css: DEFAULT_OMNI_CSS,
    rawCss: '',
    customJs: '',
    iconOverrides: {},
    textOverrides: {},
    assets: {},
    createdAt: now,
    updatedAt: now
  };
}

function extractGlobalAtRules(css: string): { atRules: string[]; remaining: string } {
  const atRules: string[] = [];
  let out = '';
  let i = 0;
  const len = css.length;

  while (i < len) {
    if (css[i] === '/' && css[i + 1] === '*') {
      const endComment = css.indexOf('*/', i + 2);
      if (endComment === -1) break;
      i = endComment + 2;
      continue;
    }
    if (css[i] === '@') {
      const rest = css.slice(i);
      const m = rest.match(/^@(?:-(?:webkit|moz)-)?(keyframes|font-face|import|charset)\b/i);
      if (m) {
        const ruleKind = m[1].toLowerCase();
        if (ruleKind === 'import' || ruleKind === 'charset') {
          const semi = css.indexOf(';', i);
          if (semi !== -1) {
            atRules.push(css.slice(i, semi + 1));
            i = semi + 1;
            continue;
          }
        } else {
          const openBrace = css.indexOf('{', i);
          if (openBrace !== -1) {
            let depth = 0;
            let j = openBrace;
            let inQuote: string | null = null;
            while (j < len) {
              const ch = css[j];
              if (inQuote) {
                if (ch === '\\') {
                  j += 2;
                  continue;
                }
                if (ch === inQuote) inQuote = null;
              } else if (ch === '"' || ch === "'") {
                inQuote = ch;
              } else if (ch === '{') {
                depth++;
              } else if (ch === '}') {
                depth--;
                if (depth === 0) {
                  atRules.push(css.slice(i, j + 1));
                  i = j + 1;
                  break;
                }
              }
              j++;
            }
            if (depth === 0) continue;
          }
        }
      }
    }
    out += css[i];
    i++;
  }

  return { atRules, remaining: out };
}

export function compileThemeCss(theme: OmniTheme | null, scopeSelector: string): string {
  if (!theme || !theme.css.trim()) return '';
  let css = theme.css;

  for (const [assetName, dataUri] of Object.entries(theme.assets || {})) {
    if (!assetName || !dataUri) continue;
    css = css.split(`omni-asset://${assetName}`).join(dataUri);
    css = css.split(`omni-asset:///${assetName}`).join(dataUri);
  }

  const { atRules, remaining } = extractGlobalAtRules(css);

  let scopedBody = remaining
    .replace(/<\/style/gi, '')
    .replace(/:root(?![\w-])/gi, ':scope')
    .replace(/(^|[\s,>+~])(html|body)(?![\w-])/gi, '$1:scope')
    .replace(/(^|[\s,>+~])\[data-omni-app\]/gi, '$1:scope');

  let depth = 0;
  let inQuote: string | null = null;
  let inComment = false;
  for (let i = 0; i < scopedBody.length; i++) {
    const ch = scopedBody[i];
    if (inComment) {
      if (ch === '*' && scopedBody[i + 1] === '/') {
        inComment = false;
        i++;
      }
      continue;
    }
    if (inQuote) {
      if (ch === '\\') {
        i++;
        continue;
      }
      if (ch === inQuote) inQuote = null;
      continue;
    }
    if (ch === '/' && scopedBody[i + 1] === '*') {
      inComment = true;
      i++;
      continue;
    }
    if (ch === '"' || ch === "'") {
      inQuote = ch;
      continue;
    }
    if (ch === '{') {
      depth++;
    } else if (ch === '}') {
      depth--;
      if (depth < 0) return '';
    }
  }
  if (depth !== 0 || inComment || inQuote !== null) return '';

  return `${atRules.join('\n')}\n@scope (${scopeSelector}) to ([data-omni-studio-ui], [data-omni-preview-bar], [data-omni-window-bar]) {\n${scopedBody}\n}`;
}

class ThemeStore {
  themes = $state<OmniTheme[]>([]);
  activeThemeId = $state<string | null>(null);
  editingTheme = $state<OmniTheme | null>(null);
  isStudioOpen = $state<boolean>(false);
  isFullscreenPreview = $state<boolean>(false);

  private unlistenPatch: (() => void) | null = null;
  private keydownHandler: ((e: KeyboardEvent) => void) | null = null;
  private disposed = false;

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

  public destroy(): void {
    this.disposed = true;
    if (this.unlistenPatch) {
      this.unlistenPatch();
      this.unlistenPatch = null;
    }
    if (this.keydownHandler && typeof window !== 'undefined') {
      window.removeEventListener('keydown', this.keydownHandler);
      this.keydownHandler = null;
    }
  }

  public syncDomStylesheet(cssText: string): void {
    if (typeof document === 'undefined') return;
    let el = document.getElementById('omni-active-theme-style') as HTMLStyleElement | null;
    if (!cssText.trim()) {
      if (el) el.remove();
      return;
    }
    if (!el) {
      el = document.createElement('style');
      el.id = 'omni-active-theme-style';
      document.head.appendChild(el);
    }
    if (el.textContent !== cssText) {
      el.textContent = cssText;
    }
  }

  private async init(): Promise<void> {
    await this.reload();
    if (this.disposed) return;
    listen<OmniTheme>('theme-studio-patch', (ev) => {
      const updated = ev.payload;
      if (!updated || !updated.id) return;
      const idx = this.themes.findIndex((t) => t.id === updated.id);
      if (idx >= 0) {
        this.themes[idx] = updated;
      } else {
        this.themes.unshift(updated);
      }
      if (this.editingTheme?.id === updated.id) {
        this.editingTheme = updated;
      }
    })
      .then((fn) => {
        if (this.disposed) fn();
        else this.unlistenPatch = fn;
      })
      .catch(() => {});

    if (typeof window !== 'undefined') {
      this.keydownHandler = (e: KeyboardEvent) => {
        if (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === 'r') {
          e.preventDefault();
          this.isFullscreenPreview = false;
          void this.setActiveTheme(null);
        }
      };
      window.addEventListener('keydown', this.keydownHandler);
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
      this.syncCustomRuntime();
    } catch {}
  }

  public syncCustomRuntime(): void {
    if (typeof window === 'undefined') return;
    const current = this.effectiveAppTheme;
    const rawCss = current?.rawCss !== undefined ? current.rawCss : (localStorage.getItem('omni_raw_css') || '');
    const customJs = current?.customJs !== undefined ? current.customJs : (localStorage.getItem('omni_custom_js') || '');
    omniBridge.injectRawCssToDom(rawCss);
    omniBridge.executeCustomJs(customJs);
  }

  public async saveTheme(theme: OmniTheme): Promise<OmniTheme> {
    const saved = await invoke<OmniTheme>('save_theme', { theme });
    const idx = this.themes.findIndex((t) => t.id === saved.id);
    if (idx >= 0) {
      this.themes[idx] = saved;
    } else {
      this.themes.unshift(saved);
    }
    if (this.editingTheme?.id === saved.id) {
      this.editingTheme = saved;
    }
    return saved;
  }

  public async deleteTheme(id: string): Promise<void> {
    await invoke('delete_theme', { id });
    const idx = this.themes.findIndex((t) => t.id === id);
    if (idx >= 0) this.themes.splice(idx, 1);
    if (this.activeThemeId === id) {
      this.activeThemeId = null;
      await invoke('set_active_theme_id', { id: null }).catch(() => {});
      this.syncCustomRuntime();
    }
    if (this.editingTheme?.id === id) {
      this.editingTheme = this.themes[0] || null;
    }
  }

  public async setActiveTheme(id: string | null): Promise<void> {
    const prev = this.activeThemeId;
    this.activeThemeId = id;
    try {
      await invoke('set_active_theme_id', { id });
      this.syncCustomRuntime();
    } catch {
      this.activeThemeId = prev;
    }
  }

  public openStudio(themeId?: string | null): void {
    const target = themeId
      ? this.themes.find((t) => t.id === themeId) || null
      : null;
    this.editingTheme = target ? JSON.parse(JSON.stringify(target)) : null;
    this.isStudioOpen = true;
    this.isFullscreenPreview = false;
  }

  public closeStudio(): void {
    this.isFullscreenPreview = false;
    this.isStudioOpen = false;
  }

  public t(key: string, fallback: string, previewOnly = false): string {
    const src = previewOnly || this.isFullscreenPreview ? this.editingTheme || this.effectiveAppTheme : this.effectiveAppTheme;
    const val = src?.textOverrides?.[key];
    return val && val.trim() ? val : fallback;
  }

  public resolveIconOverride(name: string, previewOnly = false): string | null {
    const src = previewOnly || this.isFullscreenPreview ? this.editingTheme || this.effectiveAppTheme : this.effectiveAppTheme;
    const overrides = src?.iconOverrides;
    if (!overrides) return null;
    let direct = overrides[name];
    if (!direct?.trim()) {
      if (name === 'enter' && overrides.send?.trim()) direct = overrides.send;
      else if (name === 'send' && overrides.enter?.trim()) direct = overrides.enter;
    }
    if (!direct || !direct.trim()) return null;
    let resolved = direct.trim();
    if (resolved.startsWith('omni-asset://')) {
      const assetKey = resolved.replace(/^omni-asset:\/\/\/?/, '');
      if (src?.assets?.[assetKey]) {
        return src.assets[assetKey];
      }
    }
    return resolved;
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

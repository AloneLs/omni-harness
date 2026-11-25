import { chatStore } from '../stores/chat.svelte';
import { settingsStore } from '../stores/settings.svelte';
import { themeStore } from '../stores/theme.svelte';
import { workspaceStore } from '../stores/workspace.svelte';

export interface OmniErrorLog {
  id: string;
  timestamp: number;
  source: 'css' | 'js' | 'widget';
  message: string;
  stack?: string;
}

export type OmniEventListener = (payload: any) => void;
export type OmniWidgetRenderFn = (container: HTMLElement, omni: typeof omniBridge) => (() => void) | void;

interface OmniMountedWidget {
  id: string;
  slotId: string;
  container: HTMLElement;
  cleanup?: () => void;
}

class OmniBridge {
  private slots = new Map<string, HTMLElement>();
  private widgets = new Map<string, OmniMountedWidget>();
  private listeners = new Map<string, Set<OmniEventListener>>();
  private scriptCleanupFns: Array<() => void> = [];

  safeMode = $state<boolean>(false);
  errorLogs = $state<OmniErrorLog[]>([]);
  consecutiveCrashCount = $state<number>(0);

  constructor() {
    if (typeof window !== 'undefined') {
      try {
        const savedSafe = localStorage.getItem('omni_safe_mode');
        if (savedSafe === 'true') {
          this.safeMode = true;
        }
      } catch (_) {}

      window.addEventListener('keydown', this.handleGlobalKeydown, { capture: true });
      window.addEventListener('error', this.handleWindowError);
      window.addEventListener('unhandledrejection', this.handleUnhandledRejection);
    }
  }

  private handleGlobalKeydown = (e: KeyboardEvent) => {
    if (e.ctrlKey && e.altKey && e.shiftKey && (e.key === 'Escape' || e.code === 'Escape')) {
      e.preventDefault();
      e.stopPropagation();
      this.toggleSafeMode();
    }
  };

  private handleWindowError = (e: ErrorEvent) => {
    if (this.safeMode) return;
    const msg = e.message || String(e.error || 'Unknown error');
    if (msg.includes('omni-custom-script') || e.filename?.includes('omni-custom')) {
      this.recordError('js', msg, e.error?.stack);
    }
  };

  private handleUnhandledRejection = (e: PromiseRejectionEvent) => {
    if (this.safeMode) return;
    const reason = e.reason;
    const msg = reason?.message || String(reason || 'Unhandled promise rejection');
    if (String(reason?.stack || '').includes('omni-custom')) {
      this.recordError('js', msg, reason?.stack);
    }
  };

  toggleSafeMode(force?: boolean) {
    const next = force !== undefined ? force : !this.safeMode;
    this.safeMode = next;
    try {
      localStorage.setItem('omni_safe_mode', String(next));
    } catch (_) {}

    if (next) {
      this.cleanupCustomScripts();
      this.removeRawCssFromDom();
    } else {
      this.consecutiveCrashCount = 0;
      this.injectRawCssToDom();
      this.executeCustomJs();
    }

    this.emit('safemode:changed', { safeMode: next });
  }

  recordError(source: 'css' | 'js' | 'widget', message: string, stack?: string) {
    const log: OmniErrorLog = {
      id: `err-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
      timestamp: Date.now(),
      source,
      message,
      stack
    };

    this.errorLogs = [log, ...this.errorLogs.slice(0, 49)];
    this.consecutiveCrashCount++;

    if (this.consecutiveCrashCount >= 3 && !this.safeMode) {
      console.warn('[OmniSafeMode] 3 consecutive errors detected. Automatically entering Safe Mode to prevent crash loop.');
      this.toggleSafeMode(true);
    }
  }

  clearErrors() {
    this.errorLogs = [];
    this.consecutiveCrashCount = 0;
  }

  get state() {
    return {
      get activeChatId() {
        return chatStore.activeChatId;
      },
      get activeChat() {
        return chatStore.activeSession;
      },
      get tasks() {
        return chatStore.activeTodos;
      },
      get activeModel() {
        return settingsStore.data.activeModel;
      },
      get workspace() {
        return workspaceStore.currentPath;
      },
      get isStreaming() {
        return chatStore.isStreaming;
      },
      get activeTheme() {
        return themeStore.activeTheme;
      },
      get safeMode() {
        return omniBridge.safeMode;
      }
    };
  }

  on(event: string, listener: OmniEventListener): () => void {
    if (!this.listeners.has(event)) {
      this.listeners.set(event, new Set());
    }
    this.listeners.get(event)!.add(listener);

    return () => {
      const set = this.listeners.get(event);
      if (set) {
        set.delete(listener);
        if (set.size === 0) this.listeners.delete(event);
      }
    };
  }

  emit(event: string, payload: any) {
    const set = this.listeners.get(event);
    if (!set) return;
    for (const listener of set) {
      try {
        listener(payload);
      } catch (err: any) {
        this.recordError('js', `Listener for ${event} threw: ${err?.message || err}`, err?.stack);
      }
    }
  }

  registerSlot(slotId: string, el: HTMLElement): () => void {
    this.slots.set(slotId, el);
    this.emit('slot:registered', { slotId, el });

    for (const widget of this.widgets.values()) {
      if (widget.slotId === slotId && !el.contains(widget.container)) {
        el.appendChild(widget.container);
      }
    }

    return () => {
      if (this.slots.get(slotId) === el) {
        this.slots.delete(slotId);
        this.emit('slot:unregistered', { slotId });
      }
    };
  }

  getSlot(slotId: string): HTMLElement | null {
    return this.slots.get(slotId) || null;
  }

  mountWidget(slotId: string, renderFn: OmniWidgetRenderFn, widgetId?: string): string {
    const id = widgetId || `widget-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
    this.unmountWidget(id);

    const container = document.createElement('div');
    container.setAttribute('data-omni-custom-widget', id);
    container.className = 'omni-custom-widget';

    let cleanup: (() => void) | undefined;
    try {
      const res = renderFn(container, this);
      if (typeof res === 'function') {
        cleanup = res;
      }
    } catch (err: any) {
      this.recordError('widget', `Failed to render widget "${id}": ${err?.message || err}`, err?.stack);
      return id;
    }

    this.widgets.set(id, { id, slotId, container, cleanup });

    const slotEl = this.slots.get(slotId);
    if (slotEl) {
      slotEl.appendChild(container);
    }

    return id;
  }

  unmountWidget(widgetId: string) {
    const widget = this.widgets.get(widgetId);
    if (!widget) return;

    if (widget.cleanup) {
      try {
        widget.cleanup();
      } catch (err: any) {
        console.error(`Error in widget cleanup (${widgetId}):`, err);
      }
    }

    if (widget.container.parentNode) {
      widget.container.parentNode.removeChild(widget.container);
    }

    this.widgets.delete(widgetId);
  }

  registerCleanup(fn: () => void) {
    if (typeof fn === 'function') {
      this.scriptCleanupFns.push(fn);
    }
  }

  cleanupCustomScripts() {
    while (this.scriptCleanupFns.length > 0) {
      const fn = this.scriptCleanupFns.pop();
      try {
        fn?.();
      } catch (_) {}
    }

    for (const [id] of Array.from(this.widgets.entries())) {
      this.unmountWidget(id);
    }
  }

  injectRawCssToDom(rawCssOverride?: string) {
    if (typeof document === 'undefined') return;

    let styleEl = document.getElementById('omni-custom-raw-css') as HTMLStyleElement | null;
    if (!styleEl) {
      styleEl = document.createElement('style');
      styleEl.id = 'omni-custom-raw-css';
      document.head.appendChild(styleEl);
    }

    this.injectProtectedStyles();

    if (this.safeMode) {
      styleEl.textContent = '';
      return;
    }

    const cssToInject = rawCssOverride !== undefined
      ? rawCssOverride
      : (themeStore.activeTheme?.rawCss || localStorage.getItem('omni_raw_css') || '');

    styleEl.textContent = cssToInject;
  }

  removeRawCssFromDom() {
    if (typeof document === 'undefined') return;
    const styleEl = document.getElementById('omni-custom-raw-css');
    if (styleEl) {
      styleEl.textContent = '';
    }
  }

  injectProtectedStyles() {
    if (typeof document === 'undefined') return;
    let protEl = document.getElementById('omni-protected-css') as HTMLStyleElement | null;
    if (!protEl) {
      protEl = document.createElement('style');
      protEl.id = 'omni-protected-css';
      document.head.appendChild(protEl);
    }

    protEl.textContent = `
      [data-omni-emergency-shield],
      [data-omni-emergency-banner],
      [data-omni-emergency-btn] {
        display: flex !important;
        visibility: visible !important;
        pointer-events: auto !important;
        z-index: 2147483647 !important;
        opacity: 1 !important;
      }
      [data-omni-titlebar] {
        display: flex !important;
        visibility: visible !important;
        opacity: 1 !important;
        pointer-events: auto !important;
      }
      [data-omni-settings-modal] {
        display: flex !important;
        visibility: visible !important;
        opacity: 1 !important;
        pointer-events: auto !important;
      }
    `;
  }

  executeCustomJs(customJsOverride?: string) {
    this.cleanupCustomScripts();

    if (this.safeMode || typeof window === 'undefined') return;

    const scriptCode = customJsOverride !== undefined
      ? customJsOverride
      : (themeStore.activeTheme?.customJs || localStorage.getItem('omni_custom_js') || '');

    if (!scriptCode.trim()) return;

    try {
      const runner = new Function(
        'omni',
        'window',
        'document',
        `"use strict";
        try {
          ${scriptCode}
        } catch (err) {
          omni.recordError('js', err?.message || String(err), err?.stack);
        }`
      );

      runner(this, window, document);
    } catch (err: any) {
      this.recordError('js', `Compilation error in Custom JS: ${err?.message || err}`, err?.stack);
    }
  }
}

export const omniBridge = new OmniBridge();

if (typeof window !== 'undefined') {
  (window as any).omni = omniBridge;
}

<script lang="ts">
import { onMount } from 'svelte';
import { fly, fade } from 'svelte/transition';
import { invoke } from '@tauri-apps/api/core';
import { Spring } from 'svelte/motion';
import Icon from './components/Icon.svelte';
import FilePreviewDrawer from './components/FilePreviewDrawer.svelte';
import DotGridCanvas from './components/DotGridCanvas.svelte';
import Sidebar from './components/Sidebar.svelte';
import ChatView from './components/ChatView.svelte';
import DiffReviewPanel from './components/DiffReviewPanel.svelte';
import TodoContractPanel from './components/TodoContractPanel.svelte';
import FirstRunModal from './components/FirstRunModal.svelte';
import ProvidersTab from './components/settings/ProvidersTab.svelte';
import SkillsTab from './components/settings/SkillsTab.svelte';
import SubagentsTab from './components/settings/SubagentsTab.svelte';
import McpGatewayTab from './components/settings/McpGatewayTab.svelte';
import TelegramTab from './components/settings/TelegramTab.svelte';
import SpeechTab from './components/settings/SpeechTab.svelte';
import MemorySystemTab from './components/settings/MemorySystemTab.svelte';
import AppearanceTab from './components/settings/AppearanceTab.svelte';
import ComputerUseTab from './components/settings/ComputerUseTab.svelte';
import ThemeStudio from './components/themes/ThemeStudio.svelte';
import MockHarnessPreview from './components/themes/MockHarnessPreview.svelte';
import { themeStore, compileThemeCss } from './lib/stores/theme.svelte';
import { settingsStore } from './lib/stores/settings.svelte';
import { chatStore } from './lib/stores/chat.svelte';
import { omniBridge } from './lib/runtime/omniBridge.svelte';
import { SPRING_LAUNCH, SPRING_PANEL, SPRING_POPUP, attachPressScale } from './lib/motion';

interface UpdateInfo {
  hasUpdate: boolean;
  currentVersion: string;
  latestVersion: string;
  releaseName: string;
  releaseUrl: string;
  releaseNotes: string;
  assetName?: string | null;
  assetUrl?: string | null;
  publishedAt: string;
}

let hoverTip = $state<{ text: string; x: number; y: number; above: boolean } | null>(null);
let hoverTipEl = $state<HTMLDivElement | null>(null);
let hoverEl: HTMLElement | null = null;
let hoverTimer: ReturnType<typeof setTimeout> | null = null;
let hoverRafId: number | null = null;

$effect(() => {
  if (!hoverTip || !hoverTipEl) return;
  const rect = hoverTipEl.getBoundingClientRect();
  const margin = 8;
  let nx = hoverTip.x;
  if (rect.right > window.innerWidth - margin) {
    nx = Math.max(margin, Math.round(window.innerWidth - rect.width - margin));
  }
  if (nx < margin) nx = margin;
  if (nx !== hoverTip.x) {
    hoverTip = { ...hoverTip, x: nx };
  }
});

function handleGlobalMouseOver(e: MouseEvent) {
  const target = e.target as HTMLElement | null;
  if (!target) return;
  if (hoverEl && (target === hoverEl || hoverEl.contains(target))) return;
  if (hoverRafId !== null) cancelAnimationFrame(hoverRafId);
  hoverRafId = requestAnimationFrame(() => {
    hoverRafId = null;
    const el = target.closest?.('[data-omni-tip]') as HTMLElement | null;
    if (!el) {
      if (hoverTimer) {
        clearTimeout(hoverTimer);
        hoverTimer = null;
      }
      hoverTip = null;
      hoverEl = null;
      return;
    }
    const tipText = el.getAttribute('data-omni-tip') || '';
    if (!tipText.trim()) return;
    hoverEl = el;
    if (hoverTimer) clearTimeout(hoverTimer);
    hoverTimer = setTimeout(() => {
      hoverTimer = null;
      if (!hoverEl || !hoverEl.isConnected) return;
      const rect = hoverEl.getBoundingClientRect();
      const estW = Math.min(280, Math.max(64, Math.ceil(tipText.length * 6.8) + 22));
      const margin = 8;
      const rawLeft = rect.left + rect.width / 2 - estW / 2;
      const x = Math.round(Math.min(Math.max(margin, rawLeft), Math.max(margin, window.innerWidth - estW - margin)));
      const above = rect.top > 44;
      const y = Math.round(above ? rect.top - 8 : rect.bottom + 8);
      hoverTip = { text: tipText, x, y, above };
    }, 320);
  });
}

function handleGlobalMouseOut(e: MouseEvent) {
  const rel = e.relatedTarget as HTMLElement | null;
  if (hoverEl && rel && hoverEl.contains(rel)) return;
  if (hoverRafId !== null) {
    cancelAnimationFrame(hoverRafId);
    hoverRafId = null;
  }
  if (hoverTimer) {
    clearTimeout(hoverTimer);
    hoverTimer = null;
  }
  hoverTip = null;
  hoverEl = null;
}

let updateInfo = $state<UpdateInfo | null>(null);
let updateDismissed = $state<boolean>(false);
let updateInstalling = $state<boolean>(false);
let updateStatusText = $state<string>('');
let updatePercent = $state<number>(0);

async function handleInstallUpdate() {
  if (!updateInfo?.assetUrl || !updateInfo?.assetName || updateInstalling) return;
  updateInstalling = true;
  updateStatusText = 'Downloading...';
  let unlisten: (() => void) | null = null;
  try {
    const { listen } = await import('@tauri-apps/api/event');
    unlisten = await listen<{ status?: string; percent?: number }>('update-download-progress', (ev) => {
      if (typeof ev.payload?.percent === 'number') updatePercent = ev.payload.percent;
      if (ev.payload?.status) updateStatusText = ev.payload.status;
    });
    await invoke('install_github_update', {
      assetUrl: updateInfo.assetUrl,
      assetName: updateInfo.assetName
    });
  } catch (err: any) {
    updateStatusText = err instanceof Error ? err.message : String(err);
    updateInstalling = false;
  } finally {
    if (unlisten) unlisten();
  }
}

let sidebarOpen = $state<boolean>(true);
let contractsPanelOpen = $state<boolean>(false);
let activeSettingsTab = $state<
  'providers' | 'appearance' | 'computer_use' | 'skills' | 'subagents' | 'mcp' | 'telegram' | 'speech' | 'memory'
>('providers');

const launchSpring = new Spring({ opacity: 0, scale: 0.96, y: 10 }, SPRING_LAUNCH);
const sidebarWidth = new Spring(240, SPRING_PANEL);
const contractsWidth = new Spring(0, SPRING_PANEL);
const settingsModalSpring = new Spring({ opacity: 0, scale: 0.96, y: 8 }, SPRING_POPUP);
const navIndicator = new Spring({ y: 0, height: 34 }, { stiffness: 0.28, damping: 0.8 });

const SETTINGS_TABS: { id: typeof activeSettingsTab; label: string; icon: string }[] = [
  { id: 'providers', label: 'Models & Providers', icon: 'M13 10V3L4 14h7v7l9-11h-7z' },
  { id: 'appearance', label: 'Appearance & CSS', icon: 'M12 2L2 7l10 5 10-5-10-5z M2 17l10 5 10-5 M2 12l10 5 10-5' },
  { id: 'computer_use', label: 'Computer Use', icon: 'M2 4a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V4z M8 21h8 M12 17v4' },
  { id: 'skills', label: 'Skills', icon: 'M4 19.5A2.5 2.5 0 0 1 6.5 17H20 M4 4.5A2.5 2.5 0 0 1 6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15z' },
  { id: 'subagents', label: 'Subagents', icon: 'M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2 M9 7a4 4 0 1 0 0-8 4 4 0 0 0 0 8z M23 21v-2a4 4 0 0 0-3-3.87 M16 3.13a4 4 0 0 1 0 7.75' },
  { id: 'mcp', label: 'MCP Servers', icon: 'M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z M6 12h4 M14 12h4 M10 8v8' },
  { id: 'telegram', label: 'Telegram', icon: 'M22 2L11 13 M22 2l-7 20-4-9-9-4 20-7z' },
  { id: 'speech', label: 'Speech', icon: 'M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z M19 10v2a7 7 0 0 1-14 0v-2' },
  { id: 'memory', label: 'Memory', icon: 'M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z' }
];

onMount(() => {
  invoke('window_show').catch(() => {});
  launchSpring.target = { opacity: 1, scale: 1, y: 0 };
  const updateTimer = setTimeout(() => {
    invoke<UpdateInfo>('check_github_update')
      .then((info) => {
        if (info?.hasUpdate) {
          updateInfo = info;
        }
      })
      .catch(() => {});
  }, 3000);

  const captureKeydown = (e: KeyboardEvent) => {
    if (e.key === 'Alt' || e.code === 'AltLeft' || e.code === 'AltRight') {
      e.preventDefault();
      return;
    }
    if ((e.key === 'Escape' || e.code === 'Escape') && ctxMenu.open) {
      closeCtxMenu();
    }
    handleGlobalKeydown(e);
  };
  const blockAltKeyup = (e: KeyboardEvent) => {
    if (e.key === 'Alt' || e.code === 'AltLeft' || e.code === 'AltRight') {
      e.preventDefault();
    }
  };
  window.addEventListener('keydown', captureKeydown, { capture: true });
  window.addEventListener('keyup', blockAltKeyup, { capture: true });
  return () => {
    clearTimeout(updateTimer);
    if (hoverTimer) clearTimeout(hoverTimer);
    if (hoverRafId !== null) cancelAnimationFrame(hoverRafId);
    window.removeEventListener('keydown', captureKeydown, { capture: true });
    window.removeEventListener('keyup', blockAltKeyup, { capture: true });
  };
});

$effect(() => {
  sidebarWidth.target = sidebarOpen ? 240 : 0;
});

$effect(() => {
  contractsWidth.target = contractsPanelOpen ? 320 : 0;
});

$effect(() => {
  settingsModalSpring.target = settingsStore.isSettingsOpen
    ? { opacity: 1, scale: 1, y: 0 }
    : { opacity: 0, scale: 0.96, y: 8 };
});

$effect(() => {
  const idx = Math.max(0, SETTINGS_TABS.findIndex((t) => t.id === activeSettingsTab));
  navIndicator.target = { y: idx * 36, height: 34 };
});

function toggleSidebar() {
  sidebarOpen = !sidebarOpen;
}

function toggleContractsPanel() {
  contractsPanelOpen = !contractsPanelOpen;
}

function handleMinimize() {
  invoke('window_minimize').catch(() => {});
}

function handleToggleMaximize() {
  invoke('window_toggle_maximize').catch(() => {});
}

function handleClose() {
  chatStore.flushAllBeforeClose();
  settingsStore.flushPendingSaves();
  invoke('window_close').catch(() => {});
}

function handleGlobalKeydown(e: KeyboardEvent) {
  const code = e.code || '';
  const key = (e.key || '').toLowerCase();
  if (key === 'alt') {
    e.preventDefault();
    return;
  }
  if (key === 'f11' || code === 'F11') {
    e.preventDefault();
    invoke('window_toggle_fullscreen').catch(() => {});
    return;
  }
  if ((key === 'escape' || code === 'Escape') && settingsStore.isSettingsOpen) {
    settingsStore.closeSettings();
    return;
  }
  if ((e.ctrlKey || e.metaKey) && !e.altKey) {
    if (code === 'KeyB' || key === 'b') {
      e.preventDefault();
      e.stopPropagation();
      toggleSidebar();
    } else if (code === 'KeyN' || key === 'n') {
      e.preventDefault();
      e.stopPropagation();
      chatStore.createSession();
    } else if (code === 'Comma' || key === ',') {
      e.preventDefault();
      e.stopPropagation();
      settingsStore.openSettings();
    } else if (code === 'KeyJ' || key === 'j') {
      e.preventDefault();
      e.stopPropagation();
      toggleContractsPanel();
    }
  }
}

let ctxMenu = $state<{
  open: boolean;
  x: number;
  y: number;
  selectedText: string;
  isEditable: boolean;
  targetEl: HTMLElement | null;
}>({ open: false, x: 0, y: 0, selectedText: '', isEditable: false, targetEl: null });

function handleGlobalContextMenu(e: MouseEvent) {
  e.preventDefault();
  const target = e.target as HTMLElement | null;
  const isEditable = Boolean(
    target &&
      (target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable ||
        target.closest('.cm-editor'))
  );
  const selectedText = window.getSelection()?.toString() || '';
  const menuW = 200;
  const menuH = 210;
  const x = Math.min(e.clientX, Math.max(8, window.innerWidth - menuW - 8));
  const y = Math.min(e.clientY, Math.max(8, window.innerHeight - menuH - 8));
  ctxMenu = { open: true, x, y, selectedText, isEditable, targetEl: target };
}

function closeCtxMenu() {
  if (ctxMenu.open || ctxMenu.targetEl) {
    ctxMenu = { open: false, x: 0, y: 0, selectedText: '', isEditable: false, targetEl: null };
  }
}

async function ctxAction(action: string) {
  const { selectedText, isEditable, targetEl } = ctxMenu;
  closeCtxMenu();
  if (action === 'copy') {
    if (selectedText) {
      await navigator.clipboard.writeText(selectedText).catch(() => {});
    } else if (isEditable) {
      document.execCommand('copy');
    }
  } else if (action === 'cut') {
    document.execCommand('cut');
  } else if (action === 'paste') {
    try {
      const clip = await navigator.clipboard.readText();
      if (clip && targetEl && (targetEl instanceof HTMLInputElement || targetEl instanceof HTMLTextAreaElement)) {
        const start = targetEl.selectionStart ?? targetEl.value.length;
        const end = targetEl.selectionEnd ?? targetEl.value.length;
        targetEl.setRangeText(clip, start, end, 'end');
        targetEl.dispatchEvent(new Event('input', { bubbles: true }));
      } else {
        document.execCommand('paste');
      }
    } catch {
      document.execCommand('paste');
    }
  } else if (action === 'select_all') {
    if (targetEl && (targetEl instanceof HTMLInputElement || targetEl instanceof HTMLTextAreaElement)) {
      targetEl.select();
    } else {
      document.execCommand('selectAll');
    }
  } else if (action === 'ask_omni' && selectedText.trim()) {
    await chatStore.sendMessage(selectedText.trim());
  } else if (action === 'new_chat') {
    await chatStore.createSession();
  } else if (action === 'toggle_sidebar') {
    toggleSidebar();
  } else if (action === 'open_settings') {
    settingsStore.openSettings();
  } else if (action === 'reload') {
    chatStore.flushAllBeforeClose();
    window.location.reload();
  }
}

function handleGlobalKeyup(e: KeyboardEvent) {
  if (e.key === 'Alt') {
    e.preventDefault();
  }
}

$effect(() => {
  const css = compileThemeCss(themeStore.effectiveAppTheme, '[data-omni-app]');
  themeStore.syncDomStylesheet(css);
});
</script>

<svelte:window
  onkeyup={handleGlobalKeyup}
  oncontextmenu={handleGlobalContextMenu}
  onclick={closeCtxMenu}
  onmouseover={handleGlobalMouseOver}
  onmouseout={handleGlobalMouseOut}
/>

{#if omniBridge.safeMode}
  <div
    id="omni-emergency-banner"
    data-omni-emergency-shield=""
    class="fixed top-2 left-1/2 -translate-x-1/2 z-[9999999] px-3.5 py-1.5 rounded-full bg-[#F59E0B] text-black shadow-2xl flex items-center gap-2.5 text-xs font-medium select-none animate-pulse"
  >
    <span>⚠️ Safe Mode Active (Custom CSS &amp; Scripts disabled)</span>
    <span class="font-mono text-[11px] opacity-80">[Ctrl+Alt+Shift+Esc]</span>
    <button
      type="button"
      data-omni-emergency-btn=""
      onclick={() => omniBridge.toggleSafeMode(false)}
      class="px-2 py-0.5 rounded bg-black/20 hover:bg-black/30 font-semibold transition-colors"
    >
      Restore
    </button>
    <button
      type="button"
      data-omni-emergency-btn=""
      onclick={() => {
        settingsStore.openSettings();
        activeSettingsTab = 'appearance';
      }}
      class="px-2 py-0.5 rounded bg-black/20 hover:bg-black/30 font-semibold transition-colors"
    >
      Settings
    </button>
  </div>
{/if}

<div data-omni-theme-root="" class="w-screen h-screen overflow-hidden">
<div
  data-omni-app=""
  class="w-full h-full flex bg-[#101012] text-[#E5E5E8] overflow-hidden select-none font-sans"
  style="opacity: {launchSpring.current.opacity}; transform: translateY({launchSpring.current.y}px) scale({launchSpring.current.scale});"
>
  {#if sidebarWidth.current > 0.5}
    <div class="h-full shrink-0 overflow-hidden" style="width: {sidebarWidth.current}px;">
      <Sidebar
        onToggleSidebar={toggleSidebar}
        onOpenSettings={() => settingsStore.openSettings()}
      />
    </div>
  {/if}

  <section class="flex-1 flex flex-col h-full min-w-0 relative">
    <DotGridCanvas active={chatStore.isStreaming} />

    <div
      data-omni-titlebar=""
      data-tauri-drag-region
      role="presentation"
      class="h-11 w-full flex items-center justify-between pl-3 pr-2 shrink-0 z-40 relative"
    >
      <div>
        {#if !sidebarOpen && sidebarWidth.current < 16}
          <button
            type="button"
            transition:fade={{ duration: 110 }}
            data-tauri-drag-region="false"
            class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
            onclick={toggleSidebar}
            title="Expand Sidebar (Ctrl+B)"
            use:attachPressScale
          >
            <Icon name="sidebar" class="w-4 h-4" strokeWidth={1.5} />
          </button>
        {/if}
      </div>

      <div
        data-tauri-drag-region="false"
        class="flex items-center gap-1"
      >
        <button
          type="button"
          class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
          onclick={handleMinimize}
          title="Minimize"
          use:attachPressScale
        >
          <Icon name="minimize" class="w-3.5 h-3.5" strokeWidth={1.6} />
        </button>
        <button
          type="button"
          class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
          onclick={handleToggleMaximize}
          title="Maximize"
          use:attachPressScale
        >
          <Icon name="maximize" class="w-3.5 h-3.5" strokeWidth={1.6} />
        </button>
        <button
          type="button"
          class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-white hover:bg-[#EF4444]/85 transition-colors"
          onclick={handleClose}
          title="Close"
          use:attachPressScale
        >
          <Icon name="close" class="w-3.5 h-3.5" strokeWidth={1.6} />
        </button>
      </div>
    </div>

    <div data-omni-main="" class="flex-1 flex min-h-0 overflow-hidden relative z-10">
      <ChatView onToggleContracts={toggleContractsPanel} />
      <DiffReviewPanel />

      {#if contractsWidth.current > 0.5}
        <div
          class="h-full shrink-0 border-l border-white/[0.06] bg-[#161618] overflow-hidden flex flex-col"
          style="width: {contractsWidth.current}px;"
        >
          <div class="h-10 px-4 border-b border-white/[0.06] flex items-center justify-between shrink-0">
            <span class="font-mono text-[11px] text-[#A3A3AA]">CONTRACTS</span>
            <button
              type="button"
              class="p-1 rounded-[4px] text-[#66666D] hover:text-[#E5E5E8]"
              onclick={toggleContractsPanel}
              title="Close"
              use:attachPressScale
            >
              <Icon name="close" class="w-3.5 h-3.5" strokeWidth={1.6} />
            </button>
          </div>
          <div class="flex-1 overflow-y-auto">
            <TodoContractPanel />
          </div>
        </div>
      {/if}
    </div>
  </section>

  <FilePreviewDrawer />
</div>
</div>

{#if settingsStore.isSettingsOpen || settingsModalSpring.current.opacity > 0.02}
  <div
    class="fixed inset-x-0 top-11 bottom-0 bg-black/65 backdrop-blur-[6px] z-[200] flex items-center justify-center select-none transition-[padding] duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] {themeStore.isStudioOpen ? 'p-0' : 'p-6'} {settingsStore.isSettingsOpen ? 'pointer-events-auto' : 'pointer-events-none'}"
    style="opacity: {settingsModalSpring.current.opacity};"
    onclick={() => { if (!themeStore.isStudioOpen) settingsStore.closeSettings(); }}
    role="presentation"
  >
    <div
      data-omni-settings-modal=""
      class="bg-[#161618] border border-white/[0.08] flex flex-col overflow-hidden transition-all duration-300 ease-[cubic-bezier(0.22,1,0.36,1)] {themeStore.isStudioOpen ? 'w-full h-full max-w-none rounded-none border-0 border-t border-white/[0.06]' : 'w-full max-w-[920px] h-[630px] rounded-[6px]'}"
      style="transform: translateY({themeStore.isStudioOpen ? 0 : settingsModalSpring.current.y}px) scale({themeStore.isStudioOpen ? 1 : settingsModalSpring.current.scale});"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => { if (e.key === 'Escape') { if (themeStore.isStudioOpen) themeStore.closeStudio(); else settingsStore.closeSettings(); } }}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      {#if themeStore.isStudioOpen}
        <ThemeStudio />
      {:else}
      <div class="flex-1 flex overflow-hidden">
        <aside data-omni-settings-tabs="" class="w-52 bg-[#121214] border-r border-white/[0.06] p-2.5 overflow-y-auto shrink-0 text-xs">
          <div class="relative">
            <div
              class="absolute left-0 right-0 rounded-[6px] bg-[#1C1C1F] border-t border-white/[0.1] pointer-events-none flex items-center"
              style="height: {navIndicator.current.height}px; transform: translateY({navIndicator.current.y}px);"
            >
              <div class="w-[2px] h-4 bg-[#EDEDEF] rounded-r"></div>
            </div>

            {#each SETTINGS_TABS as tab}
              <button
                type="button"
                class="relative z-10 w-full h-[34px] mb-[2px] flex items-center gap-2.5 px-3 rounded-[6px] text-left transition-colors {activeSettingsTab === tab.id ? 'text-[#EDEDEF] font-medium' : 'text-[#8E8E95] hover:text-[#D4D4D8]'}"
                onclick={() => (activeSettingsTab = tab.id)}
              >
                <svg class="w-3.5 h-3.5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
                  <path d={tab.icon}></path>
                </svg>
                <span class="truncate">{tab.label}</span>
              </button>
            {/each}
          </div>
        </aside>

        <section data-omni-settings-content="" class="flex-1 bg-[#161618] p-6 overflow-y-auto settings-pane">
          {#key activeSettingsTab}
            <div in:fly={{ y: 6, duration: 150 }}>
              {#if activeSettingsTab === 'providers'}
                <ProvidersTab />
              {:else if activeSettingsTab === 'skills'}
                <SkillsTab />
              {:else if activeSettingsTab === 'subagents'}
                <SubagentsTab />
              {:else if activeSettingsTab === 'mcp'}
                <McpGatewayTab />
              {:else if activeSettingsTab === 'telegram'}
                <TelegramTab />
              {:else if activeSettingsTab === 'speech'}
                <SpeechTab />
              {:else if activeSettingsTab === 'appearance'}
                <AppearanceTab />
              {:else if activeSettingsTab === 'computer_use'}
                <ComputerUseTab />
              {:else if activeSettingsTab === 'memory'}
                <MemorySystemTab />
              {/if}
            </div>
          {/key}
        </section>
      </div>
      {/if}
    </div>
  </div>
{/if}

{#if themeStore.isFullscreenPreview}
  <div class="fixed inset-x-0 top-11 bottom-0 z-[300] bg-[#101012] border-t border-white/[0.06]">
    <MockHarnessPreview theme={themeStore.editingTheme} fullscreen={true} />
    <div
      data-omni-preview-bar=""
      class="fixed top-14 left-1/2 -translate-x-1/2 z-[350] flex items-center gap-2 px-3 py-1 rounded-[6px] bg-[#161618]/95 backdrop-blur-md border border-white/[0.12] text-xs text-[#EDEDEF] select-none"
    >
      <span class="font-mono text-[11px] text-[#A3A3AA]">Preview: {themeStore.editingTheme?.name || 'Custom'}</span>
      <button
        type="button"
        onclick={() => (themeStore.isFullscreenPreview = false)}
        class="px-2 py-0.5 rounded-[4px] bg-[#242428] hover:bg-[#2C2C32] text-[11px] text-[#EDEDEF] transition-colors"
      >
        Exit Preview
      </button>
      <button
        type="button"
        onclick={async () => {
          if (themeStore.editingTheme) {
            const saved = await themeStore.saveTheme(themeStore.editingTheme);
            await themeStore.setActiveTheme(saved.id);
          }
          themeStore.isFullscreenPreview = false;
        }}
        class="px-2 py-0.5 rounded-[4px] bg-[#EDEDEF] hover:bg-white text-[11px] text-[#111113] font-medium transition-colors"
      >
        Apply
      </button>
      <button
        type="button"
        onclick={() => {
          themeStore.setActiveTheme(null);
          themeStore.isFullscreenPreview = false;
        }}
        class="px-2 py-0.5 rounded-[4px] bg-white/[0.06] hover:bg-[#EF4444]/25 text-[11px] text-[#A3A3AA] hover:text-white transition-colors"
      >
        Off
      </button>
    </div>
  </div>
{/if}

{#if settingsStore.isSettingsOpen || themeStore.isStudioOpen || themeStore.isFullscreenPreview}
  <div
    data-omni-window-bar=""
    data-tauri-drag-region
    role="presentation"
    class="fixed top-0 left-0 right-0 h-11 z-[500] bg-[#101012] border-b border-white/[0.06] flex items-center justify-between pl-4 pr-2 select-none"
  >
    <span class="font-mono text-[11px] font-medium tracking-wider text-[#A3A3AA] pointer-events-none leading-none">
      {themeStore.isFullscreenPreview ? 'OMNIHARNESS // PREVIEW' : themeStore.isStudioOpen ? 'OMNIHARNESS // THEME STUDIO' : 'OMNIHARNESS // SETTINGS'}
    </span>
    <div data-tauri-drag-region="false" class="flex items-center gap-1">
      <button
        type="button"
        class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
        onclick={handleMinimize}
        title="Minimize"
      >
        <Icon name="minimize" class="w-3.5 h-3.5" strokeWidth={1.6} />
      </button>
      <button
        type="button"
        class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
        onclick={handleToggleMaximize}
        title="Maximize"
      >
        <Icon name="maximize" class="w-3.5 h-3.5" strokeWidth={1.6} />
      </button>
      <button
        type="button"
        class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-white hover:bg-[#EF4444]/85 transition-colors"
        onclick={handleClose}
        title="Close"
      >
        <Icon name="close" class="w-3.5 h-3.5" strokeWidth={1.6} />
      </button>
    </div>
  </div>
{/if}

<FirstRunModal
  isOpen={settingsStore.isFirstRunOpen}
  onComplete={() => settingsStore.closeFirstRun()}
/>

{#if ctxMenu.open}
  <div
    data-omni-context-menu=""
    role="menu"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => { if (e.key === 'Escape') closeCtxMenu(); }}
    class="fixed z-[900] w-[196px] py-1 rounded-[8px] bg-[#161618]/98 backdrop-blur-md border border-white/[0.1] shadow-2xl text-xs text-[#E5E5E8] select-none"
    style="left: {ctxMenu.x}px; top: {ctxMenu.y}px;"
  >
    {#if ctxMenu.isEditable}
      <button type="button" onclick={() => ctxAction('cut')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] flex items-center justify-between">
        <span>Cut</span><span class="font-mono text-[10px] text-[#66666D]">Ctrl+X</span>
      </button>
      <button type="button" onclick={() => ctxAction('copy')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] flex items-center justify-between">
        <span>Copy</span><span class="font-mono text-[10px] text-[#66666D]">Ctrl+C</span>
      </button>
      <button type="button" onclick={() => ctxAction('paste')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] flex items-center justify-between">
        <span>Paste</span><span class="font-mono text-[10px] text-[#66666D]">Ctrl+V</span>
      </button>
      <button type="button" onclick={() => ctxAction('select_all')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] flex items-center justify-between">
        <span>Select All</span><span class="font-mono text-[10px] text-[#66666D]">Ctrl+A</span>
      </button>
      <div class="my-1 border-t border-white/[0.06]"></div>
    {:else if ctxMenu.selectedText}
      <button type="button" onclick={() => ctxAction('copy')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] flex items-center justify-between">
        <span>Copy</span><span class="font-mono text-[10px] text-[#66666D]">Ctrl+C</span>
      </button>
      <button type="button" onclick={() => ctxAction('ask_omni')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] text-[#EDEDEF]">
        Ask OmniHarness
      </button>
      <div class="my-1 border-t border-white/[0.06]"></div>
    {/if}
    <button type="button" onclick={() => ctxAction('new_chat')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] flex items-center justify-between">
      <span>New Chat</span><span class="font-mono text-[10px] text-[#66666D]">Ctrl+N</span>
    </button>
    <button type="button" onclick={() => ctxAction('toggle_sidebar')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] flex items-center justify-between">
      <span>Toggle Sidebar</span><span class="font-mono text-[10px] text-[#66666D]">Ctrl+B</span>
    </button>
    <button type="button" onclick={() => ctxAction('open_settings')} class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07] flex items-center justify-between">
      <span>Settings</span><span class="font-mono text-[10px] text-[#66666D]">Ctrl+,</span>
    </button>
    <div class="my-1 border-t border-white/[0.06]"></div>
    <button type="button" onclick={() => ctxAction('reload')} class="w-full px-3 py-1.5 text-left text-[#A3A3AA] hover:text-white hover:bg-white/[0.07]">
      Reload Window
    </button>
  </div>
{/if}

{#if updateInfo?.hasUpdate && !updateDismissed}
  <div
    transition:fly={{ y: 12, duration: 200 }}
    class="fixed bottom-5 right-5 z-[450] w-[320px] rounded-[10px] bg-[#161618]/95 backdrop-blur-md border border-white/[0.12] p-3.5 shadow-2xl text-xs text-[#E5E5E8] select-none space-y-2.5"
  >
    <div class="flex items-start justify-between gap-2">
      <div class="min-w-0">
        <div class="font-medium text-[#EDEDEF]">Доступно обновление v{updateInfo.latestVersion}</div>
        <div class="text-[11px] text-[#73737A] mt-0.5">Текущая версия: v{updateInfo.currentVersion}</div>
      </div>
      {#if !updateInstalling}
        <button
          type="button"
          onclick={() => (updateDismissed = true)}
          class="p-1 rounded-[4px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.06]"
          title="Позже"
        >
          <Icon name="close" class="w-3.5 h-3.5" strokeWidth={1.6} />
        </button>
      {/if}
    </div>
    {#if updateInstalling}
      <div class="space-y-1">
        <div class="flex justify-between font-mono text-[10.5px] text-[#A3A3AA]">
          <span>{updateStatusText || 'Updating...'}</span>
          <span>{updatePercent}%</span>
        </div>
        <div class="h-1.5 rounded-full bg-white/[0.08] overflow-hidden">
          <div class="h-full bg-[#EDEDEF] transition-all duration-200" style="width: {updatePercent}%"></div>
        </div>
      </div>
    {:else}
      <div class="flex items-center justify-end gap-2">
        <button
          type="button"
          onclick={() => (updateDismissed = true)}
          class="h-7 px-2.5 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#A3A3AA]"
        >
          Позже
        </button>
        {#if updateInfo.assetUrl}
          <button
            type="button"
            onclick={handleInstallUpdate}
            class="h-7 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[11px] font-medium text-[#111113] transition-colors"
          >
            Обновить
          </button>
        {/if}
      </div>
    {/if}
  </div>
{/if}

{#if hoverTip}
  <div
    bind:this={hoverTipEl}
    class="fixed z-[950] pointer-events-none px-2.5 py-1 rounded-[6px] bg-[#18181B]/96 backdrop-blur-md border border-white/[0.1] shadow-xl font-sans text-[11px] text-[#EDEDEF] max-w-[280px] truncate {hoverTip.above ? '-translate-y-full' : ''}"
    style="left: {hoverTip.x}px; top: {hoverTip.y}px;"
  >
    {hoverTip.text}
  </div>
{/if}

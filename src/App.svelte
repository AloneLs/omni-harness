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
import TodoContractPanel from './components/TodoContractPanel.svelte';
import FirstRunModal from './components/FirstRunModal.svelte';
import ProvidersTab from './components/settings/ProvidersTab.svelte';
import SkillsTab from './components/settings/SkillsTab.svelte';
import McpGatewayTab from './components/settings/McpGatewayTab.svelte';
import TelegramTab from './components/settings/TelegramTab.svelte';
import SpeechTab from './components/settings/SpeechTab.svelte';
import MemorySystemTab from './components/settings/MemorySystemTab.svelte';
import { settingsStore } from './lib/stores/settings.svelte';
import { chatStore } from './lib/stores/chat.svelte';
import { SPRING_LAUNCH, SPRING_PANEL, SPRING_POPUP, attachPressScale } from './lib/motion';

let sidebarOpen = $state<boolean>(true);
let contractsPanelOpen = $state<boolean>(false);
let activeSettingsTab = $state<
  'providers' | 'skills' | 'mcp' | 'telegram' | 'speech' | 'memory'
>('providers');

const launchSpring = new Spring({ opacity: 0, scale: 0.96, y: 10 }, SPRING_LAUNCH);
const sidebarWidth = new Spring(240, SPRING_PANEL);
const contractsWidth = new Spring(0, SPRING_PANEL);
const settingsModalSpring = new Spring({ opacity: 0, scale: 0.96, y: 8 }, SPRING_POPUP);
const navIndicator = new Spring({ y: 0, height: 34 }, { stiffness: 0.28, damping: 0.8 });

const SETTINGS_TABS: { id: typeof activeSettingsTab; label: string; icon: string }[] = [
  { id: 'providers', label: 'Models & Providers', icon: 'M13 10V3L4 14h7v7l9-11h-7z' },
  { id: 'skills', label: 'Skills', icon: 'M4 19.5A2.5 2.5 0 0 1 6.5 17H20 M4 4.5A2.5 2.5 0 0 1 6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15z' },
  { id: 'mcp', label: 'MCP Servers', icon: 'M4 4h16c1.1 0 2 .9 2 2v12c0 1.1-.9 2-2 2H4c-1.1 0-2-.9-2-2V6c0-1.1.9-2 2-2z M6 12h4 M14 12h4 M10 8v8' },
  { id: 'telegram', label: 'Telegram', icon: 'M22 2L11 13 M22 2l-7 20-4-9-9-4 20-7z' },
  { id: 'speech', label: 'Speech', icon: 'M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z M19 10v2a7 7 0 0 1-14 0v-2' },
  { id: 'memory', label: 'Memory', icon: 'M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z' }
];

onMount(() => {
  requestAnimationFrame(() => {
    invoke('window_show').catch(() => {});
    launchSpring.target = { opacity: 1, scale: 1, y: 0 };
  });
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

function handleDragMouseDown(e: MouseEvent) {
  if (e.buttons !== 1) return;
  if ((e.target as HTMLElement).closest('button')) return;
  invoke('window_start_dragging').catch(() => {});
}

function handleMinimize() {
  invoke('window_minimize').catch(() => {});
}

function handleToggleMaximize() {
  invoke('window_toggle_maximize').catch(() => {});
}

function handleClose() {
  invoke('window_close').catch(() => {});
}

function handleGlobalKeydown(e: KeyboardEvent) {
  if (e.key === 'Escape' && settingsStore.isSettingsOpen) {
    settingsStore.closeSettings();
    return;
  }
  if (e.ctrlKey || e.metaKey) {
    if (e.key.toLowerCase() === 'b') {
      e.preventDefault();
      toggleSidebar();
    } else if (e.key.toLowerCase() === 'n') {
      e.preventDefault();
      chatStore.createSession();
    } else if (e.key === ',') {
      e.preventDefault();
      settingsStore.openSettings();
    } else if (e.key.toLowerCase() === 'j') {
      e.preventDefault();
      toggleContractsPanel();
    }
  }
}
</script>

<svelte:window onkeydown={handleGlobalKeydown} />

<div
  class="w-screen h-screen flex bg-[#101012] text-[#E5E5E8] overflow-hidden select-none font-sans"
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

  <section class="flex-1 flex flex-col h-full min-w-0 relative bg-[#101012]">
    <DotGridCanvas active={chatStore.isStreaming} />

    <div
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

    <div class="flex-1 flex min-h-0 overflow-hidden relative z-10">
      <ChatView onToggleContracts={toggleContractsPanel} />

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
            <TodoContractPanel onRollback={(path: string) => chatStore.rollbackSnapshot(path)} />
          </div>
        </div>
      {/if}
    </div>
  </section>

  <FilePreviewDrawer />
</div>

{#if settingsStore.isSettingsOpen || settingsModalSpring.current.opacity > 0.02}
  <div
    class="fixed inset-0 bg-black/65 backdrop-blur-[6px] z-[200] flex items-center justify-center p-8 select-none {settingsStore.isSettingsOpen ? 'pointer-events-auto' : 'pointer-events-none'}"
    style="opacity: {settingsModalSpring.current.opacity};"
    onclick={() => settingsStore.closeSettings()}
    role="presentation"
  >
    <div
      class="w-full max-w-[920px] h-[650px] bg-[#161618] border border-white/[0.08] rounded-[6px] flex flex-col overflow-hidden"
      style="transform: translateY({settingsModalSpring.current.y}px) scale({settingsModalSpring.current.scale});"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => { if (e.key === 'Escape') settingsStore.closeSettings(); }}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
    >
      <div class="h-11 px-5 bg-[#121214] border-b border-white/[0.06] flex items-center justify-between shrink-0">
        <span class="font-mono text-xs text-[#D4D4D8] tracking-wide">OMNIHARNESS // SETTINGS</span>
        <button
          type="button"
          class="p-1.5 rounded-[4px] text-[#73737A] hover:text-[#E5E5E8] hover:bg-white/[0.06]"
          onclick={() => settingsStore.closeSettings()}
          title="Close (Escape)"
          use:attachPressScale
        >
          <Icon name="close" class="w-4 h-4" strokeWidth={1.6} />
        </button>
      </div>

      <div class="flex-1 flex overflow-hidden">
        <aside class="w-52 bg-[#121214] border-r border-white/[0.06] p-2.5 overflow-y-auto shrink-0 text-xs">
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

        <section class="flex-1 bg-[#161618] p-6 overflow-y-auto settings-pane">
          {#key activeSettingsTab}
            <div in:fly={{ y: 6, duration: 150 }}>
              {#if activeSettingsTab === 'providers'}
                <ProvidersTab />
              {:else if activeSettingsTab === 'skills'}
                <SkillsTab />
              {:else if activeSettingsTab === 'mcp'}
                <McpGatewayTab />
              {:else if activeSettingsTab === 'telegram'}
                <TelegramTab />
              {:else if activeSettingsTab === 'speech'}
                <SpeechTab />
              {:else if activeSettingsTab === 'memory'}
                <MemorySystemTab />
              {/if}
            </div>
          {/key}
        </section>
      </div>
    </div>
  </div>
{/if}

<FirstRunModal
  isOpen={settingsStore.isFirstRunOpen}
  onComplete={() => settingsStore.closeFirstRun()}
/>

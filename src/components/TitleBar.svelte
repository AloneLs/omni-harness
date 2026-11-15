<script lang="ts">
import { getCurrentWindow } from '@tauri-apps/api/window';
import { workspaceStore } from '../lib/stores/workspace.svelte';
import { chatStore } from '../lib/stores/chat.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';
import { attachPressScale } from '../lib/motion';

let { onToggleSidebar, onToggleContracts }: {
  onToggleSidebar?: () => void;
  onToggleContracts?: () => void;
} = $props();

let searchModalOpen = $state(false);
let searchQuery = $state('');
let searchInputEl = $state<HTMLInputElement | null>(null);

let matchingChats = $derived.by(() => {
  if (!searchQuery.trim()) {
    return [];
  }
  const q = searchQuery.toLowerCase();
  return chatStore.sessions.filter(
    (s) => s.title.toLowerCase().includes(q) || s.preview.toLowerCase().includes(q)
  );
});

let isMaximized = $state(false);

async function handleMinimize() {
  try {
    const appWindow = getCurrentWindow();
    await appWindow.minimize();
  } catch {}
}

async function handleToggleMaximize() {
  try {
    const appWindow = getCurrentWindow();
    await appWindow.toggleMaximize();
    isMaximized = await appWindow.isMaximized();
  } catch {}
}

async function handleClose() {
  try {
    const appWindow = getCurrentWindow();
    await appWindow.close();
  } catch {}
}

function openSearch() {
  searchModalOpen = true;
  setTimeout(() => {
    searchInputEl?.focus();
  }, 50);
}

function closeSearch() {
  searchModalOpen = false;
  searchQuery = '';
}

function handleSearchKeydown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    closeSearch();
  }
}

function selectChatResult(id: string) {
  chatStore.selectSession(id);
  closeSearch();
}

function selectFileResult(path: string) {
  workspaceStore.selectFile(path);
  closeSearch();
}
</script>

<svelte:window onkeydown={(e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    if (searchModalOpen) {
      closeSearch();
    } else {
      openSearch();
    }
  }
}} />

<header
  data-tauri-drag-region
  class="h-10 w-full flex items-center justify-between px-3 bg-[#0E0F12] border-b border-[#2B2E36] select-none text-xs z-50 shrink-0"
>
  <div class="flex items-center gap-2" data-tauri-drag-region>
    {#if onToggleSidebar}
      <button
        type="button"
        class="p-1 rounded text-[#9BA1B0] hover:text-[#F0F3F8] hover:bg-[#1E222B] transition-colors"
        onclick={onToggleSidebar}
        title="Toggle Sidebar (Ctrl+B)"
        use:attachPressScale
      >
        <svg class="w-4 h-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <rect x="3" y="3" width="18" height="18" rx="2" ry="2"></rect>
          <line x1="9" y1="3" x2="9" y2="21"></line>
        </svg>
      </button>
    {/if}

    <div class="flex items-center gap-1.5 font-semibold text-[#F0F3F8] tracking-tight mr-2" data-tauri-drag-region>
      <div class="w-2 h-2 rounded-full bg-[#3E7BFA] animate-pulse"></div>
      <span>OmniHarness</span>
    </div>

    <button
      type="button"
      class="flex items-center gap-1.5 px-2 py-1 rounded bg-[#131416] hover:bg-[#1E222B] border border-[#2B2E36] text-[#9BA1B0] hover:text-[#F0F3F8] max-w-[220px] transition-colors"
      onclick={() => workspaceStore.openDirectoryPicker()}
      title={workspaceStore.currentPath || 'Open Directory'}
      use:attachPressScale
    >
      <svg class="w-3.5 h-3.5 text-[#3E7BFA] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
      </svg>
      <span class="truncate font-mono">{workspaceStore.workspaceName}</span>
    </button>

    {#if chatStore.pinnedSessions.length > 0}
      <div class="hidden md:flex items-center gap-1 border-l border-[#2B2E36] pl-2">
        {#each chatStore.pinnedSessions.slice(0, 3) as pinned}
          <button
            type="button"
            class="px-2 py-0.5 rounded text-[11px] font-medium border transition-colors truncate max-w-[130px] {chatStore.activeChatId === pinned.id ? 'bg-[#3E7BFA]/20 border-[#3E7BFA] text-[#F0F3F8]' : 'bg-[#131416] border-[#2B2E36] text-[#9BA1B0] hover:text-[#F0F3F8]'}"
            onclick={() => chatStore.selectSession(pinned.id)}
            title={pinned.title}
          >
            ★ {pinned.title}
          </button>
        {/each}
      </div>
    {/if}
  </div>

  <div class="flex-1 max-w-[320px] mx-4" data-tauri-drag-region>
    <button
      type="button"
      class="w-full flex items-center justify-between px-2.5 py-1 rounded bg-[#131416] hover:bg-[#181A20] border border-[#2B2E36] text-[#5F6677] hover:text-[#9BA1B0] transition-colors text-left"
      onclick={openSearch}
    >
      <div class="flex items-center gap-2 truncate">
        <svg class="w-3.5 h-3.5 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="8"></circle>
          <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
        </svg>
        <span class="truncate">Search chats & files...</span>
      </div>
      <kbd class="px-1.5 py-0.5 rounded text-[10px] bg-[#1E222B] border border-[#2B2E36] text-[#9BA1B0] font-mono">Ctrl+K</kbd>
    </button>
  </div>

  <div class="flex items-center gap-1.5" data-tauri-drag-region>
    <div class="hidden sm:flex items-center gap-1.5 mr-2">
      <span class="px-2 py-0.5 rounded bg-[#181A20] border border-[#2B2E36] text-[11px] text-[#9BA1B0] font-mono">
        {settingsStore.data.activeModel}
      </span>
      <span class="px-1.5 py-0.5 rounded bg-[#3E7BFA]/20 text-[#3E7BFA] text-[10px] font-bold">
        {settingsStore.data.reasoningLevel}
      </span>
    </div>

    {#if onToggleContracts}
      <button
        type="button"
        class="px-2 py-1 rounded bg-[#131416] hover:bg-[#1E222B] border border-[#2B2E36] text-[#9BA1B0] hover:text-[#F0F3F8] transition-colors mr-1 flex items-center gap-1"
        onclick={onToggleContracts}
        title="Goals & Acceptance Contracts"
        use:attachPressScale
      >
        <svg class="w-3.5 h-3.5 text-[#10B981]" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M9 11l3 3L22 4"></path>
          <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11"></path>
        </svg>
        <span class="hidden lg:inline text-[11px]">Contracts</span>
      </button>
    {/if}

    <div class="flex items-center">
      <button
        type="button"
        class="w-7 h-7 flex items-center justify-center text-[#9BA1B0] hover:text-[#F0F3F8] hover:bg-[#1E222B] rounded transition-colors"
        onclick={handleMinimize}
        title="Minimize"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" fill="none">
          <line x1="5" y1="12" x2="19" y2="12"></line>
        </svg>
      </button>
      <button
        type="button"
        class="w-7 h-7 flex items-center justify-center text-[#9BA1B0] hover:text-[#F0F3F8] hover:bg-[#1E222B] rounded transition-colors"
        onclick={handleToggleMaximize}
        title="Maximize"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" fill="none">
          <rect x="5" y="5" width="14" height="14" rx="1"></rect>
        </svg>
      </button>
      <button
        type="button"
        class="w-7 h-7 flex items-center justify-center text-[#9BA1B0] hover:text-white hover:bg-[#EF4444] rounded transition-colors"
        onclick={handleClose}
        title="Close"
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" stroke="currentColor" stroke-width="2" fill="none">
          <line x1="18" y1="6" x2="6" y2="18"></line>
          <line x1="6" y1="6" x2="18" y2="18"></line>
        </svg>
      </button>
    </div>
  </div>
</header>

{#if searchModalOpen}
  <div
    class="fixed inset-0 bg-black/60 backdrop-blur-sm z-[100] flex items-start justify-center pt-20 px-4"
    onclick={closeSearch}
    role="button"
    tabindex="0"
    onkeydown={(e) => { if (e.key === 'Escape') closeSearch(); }}
  >
    <div
      class="w-full max-w-xl bg-[#131416] border border-[#2B2E36] rounded-xl shadow-2xl overflow-hidden flex flex-col max-h-[70vh] animate-spring-in"
      onclick={(e) => e.stopPropagation()}
      role="region"
      tabindex="-1"
    >
      <div class="p-3 border-b border-[#2B2E36] flex items-center gap-2 bg-[#181A20]">
        <svg class="w-4 h-4 text-[#3E7BFA] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="8"></circle>
          <line x1="21" y1="21" x2="16.65" y2="16.65"></line>
        </svg>
        <input
          bind:this={searchInputEl}
          bind:value={searchQuery}
          oninput={() => workspaceStore.searchFiles(searchQuery)}
          onkeydown={handleSearchKeydown}
          type="text"
          placeholder="Search conversations, files, code snippets..."
          class="w-full bg-transparent text-sm text-[#F0F3F8] placeholder-[#5F6677] focus:outline-none"
        />
        <button
          type="button"
          class="text-xs text-[#5F6677] hover:text-[#9BA1B0] px-1.5 py-0.5 rounded bg-[#1E222B] border border-[#2B2E36]"
          onclick={closeSearch}
        >
          ESC
        </button>
      </div>

      <div class="overflow-y-auto p-2 space-y-3 flex-1 text-xs">
        {#if searchQuery.trim()}
          {#if matchingChats.length > 0}
            <div>
              <div class="px-2 py-1 text-[11px] font-bold text-[#5F6677] uppercase tracking-wider">Conversations</div>
              <div class="space-y-1">
                {#each matchingChats as chat}
                  <button
                    type="button"
                    class="w-full text-left p-2 rounded-lg hover:bg-[#1E222B] transition-colors flex flex-col gap-0.5"
                    onclick={() => selectChatResult(chat.id)}
                  >
                    <div class="font-medium text-[#F0F3F8] flex items-center gap-1.5">
                      <span class="text-[#3E7BFA]">{chat.pinned ? '★' : '💬'}</span>
                      <span>{chat.title}</span>
                    </div>
                    <div class="text-[#9BA1B0] truncate text-[11px]">{chat.preview}</div>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          {#if workspaceStore.searchResults.length > 0}
            <div>
              <div class="px-2 py-1 text-[11px] font-bold text-[#5F6677] uppercase tracking-wider">Files & Code</div>
              <div class="space-y-1">
                {#each workspaceStore.searchResults.slice(0, 10) as item}
                  <button
                    type="button"
                    class="w-full text-left p-2 rounded-lg hover:bg-[#1E222B] transition-colors flex items-center justify-between"
                    onclick={() => selectFileResult(item.path)}
                  >
                    <div class="flex items-center gap-2 truncate">
                      <svg class="w-3.5 h-3.5 text-[#10B981] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
                        <polyline points="14 2 14 8 20 8"></polyline>
                      </svg>
                      <span class="font-mono text-[#F0F3F8] truncate">{item.name}</span>
                    </div>
                    <span class="text-[#5F6677] text-[10px] font-mono truncate max-w-[200px]">{item.path}</span>
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          {#if matchingChats.length === 0 && workspaceStore.searchResults.length === 0}
            <div class="text-center py-8 text-[#5F6677]">
              No matches found for "{searchQuery}"
            </div>
          {/if}
        {:else}
          <div class="text-center py-8 text-[#5F6677]">
            Type a query to search chat titles, previews, and workspace files.
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

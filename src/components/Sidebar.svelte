<script lang="ts">
import { slide, crossfade } from 'svelte/transition';
import { cubicOut } from 'svelte/easing';
import { flip } from 'svelte/animate';
import { invoke } from '@tauri-apps/api/core';
import { Spring } from 'svelte/motion';
import Icon from './Icon.svelte';
import { themeStore } from '../lib/stores/theme.svelte';
import { chatStore } from '../lib/stores/chat.svelte';
import { workspaceStore } from '../lib/stores/workspace.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';
import { SPRING_POPUP, attachPressScale } from '../lib/motion';

let { onOpenSettings, onToggleSidebar }: {
  onOpenSettings?: () => void;
  onToggleSidebar?: () => void;
} = $props();

let editingChatId = $state<string | null>(null);
let editTitleText = $state('');
let showSearchInput = $state(false);
let showAllSessions = $state(false);

const searchBoxSpring = new Spring(0, SPRING_POPUP);

$effect(() => {
  searchBoxSpring.target = showSearchInput ? 38 : 0;
});

const INITIAL_LIMIT = 8;
const [pinSend, pinReceive] = crossfade({ duration: 220, easing: cubicOut });

let visibleRecent = $derived.by(() => {
  if (showAllSessions || chatStore.filterSearchQuery.trim().length > 0 || chatStore.showArchived) {
    return chatStore.recentSessions;
  }
  return chatStore.recentSessions.slice(0, INITIAL_LIMIT);
});

function normProj(p?: string): string {
  return (p || '').replace(/[\\/]+$/, '').toLowerCase();
}

let uniqueProjectPaths = $derived.by(() => {
  const map = new Map<string, string>();
  if (workspaceStore.currentPath) {
    map.set(normProj(workspaceStore.currentPath), workspaceStore.currentPath);
  }
  for (const c of visibleRecent) {
    if (c.projectPath) {
      const k = normProj(c.projectPath);
      if (!map.has(k)) map.set(k, c.projectPath);
    }
  }
  return Array.from(map.values());
});

let projectChatsByNorm = $derived.by(() => {
  const grouped = new Map<string, typeof visibleRecent>();
  for (const c of visibleRecent) {
    if (!c.projectPath) continue;
    const k = normProj(c.projectPath);
    const list = grouped.get(k);
    if (list) list.push(c);
    else grouped.set(k, [c]);
  }
  return grouped;
});

let standaloneChats = $derived(visibleRecent.filter((c) => !c.projectPath));

let hasMoreSessions = $derived(
  !showAllSessions &&
  !chatStore.showArchived &&
  chatStore.filterSearchQuery.trim().length === 0 &&
  chatStore.recentSessions.length > INITIAL_LIMIT
);

function handleDragMouseDown(e: MouseEvent) {
  if (e.buttons !== 1) return;
  if ((e.target as HTMLElement).closest('button')) return;
  invoke('window_start_dragging').catch(() => {});
}

function handleNewChat() {
  chatStore.createSession();
}

function handleStartRename(id: string, currentTitle: string) {
  editingChatId = id;
  editTitleText = currentTitle;
}

function handleSaveRename(id: string) {
  if (editingChatId === id && editTitleText.trim()) {
    chatStore.renameSession(id, editTitleText.trim());
  }
  editingChatId = null;
}

function handleCancelRename() {
  editingChatId = null;
}

function formatRelativeAge(timestamp: number): string {
  const diffMs = Math.max(0, Date.now() - timestamp);
  const mins = Math.floor(diffMs / 60000);
  if (mins < 60) return `${Math.max(1, mins)}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  return `${days}d`;
}
</script>

<aside data-omni-sidebar="" class="w-[240px] h-full bg-[#171719] border-r border-white/[0.06] flex flex-col select-none shrink-0 overflow-hidden">
  <div
    data-omni-sidebar-header=""
    data-tauri-drag-region
    role="presentation"
    class="h-11 px-3 border-b border-white/[0.06] flex items-center justify-between shrink-0"
  >
    <div class="flex items-center gap-1.5">
      {#if onToggleSidebar}
        <button
          type="button"
          data-tauri-drag-region="false"
          class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
          onclick={onToggleSidebar}
          title="Collapse Sidebar (Ctrl+B)"
          use:attachPressScale
        >
          <Icon name="sidebar" class="w-4 h-4" strokeWidth={1.5} />
        </button>
      {/if}
      <button
        type="button"
        data-omni-button="new-chat"
        data-tauri-drag-region="false"
        class="flex items-center gap-2 text-[13px] font-sans text-[#D4D4D8] hover:text-white transition-colors"
        onclick={handleNewChat}
        use:attachPressScale
      >
        <Icon name="plus-circle" class="w-4 h-4 text-[#8A8A92] shrink-0" strokeWidth={1.5} />
        <span>{themeStore.t('new_task', 'New task')}</span>
      </button>
    </div>
    <span class="font-mono text-[11px] text-[#5C5C62] shrink-0 pl-2 pointer-events-none">Ctrl+N</span>
  </div>

  <div class="px-4 pt-3.5 pb-2 flex items-center justify-between shrink-0">
    <span class="text-[12px] font-sans font-medium text-[#A3A3AA]">
      {chatStore.showArchived ? 'Archived' : themeStore.t('task_list', 'Task list')}
    </span>
    <div class="flex items-center gap-1 shrink-0">
      <button
        type="button"
        class="p-1 rounded-[4px] transition-colors {showSearchInput ? 'text-[#E5E5E8] bg-white/[0.08]' : 'text-[#66666D] hover:text-[#D4D4D8] hover:bg-white/[0.05]'}"
        onclick={() => {
          showSearchInput = !showSearchInput;
          if (!showSearchInput) chatStore.filterSearchQuery = '';
        }}
        title="Search tasks"
        use:attachPressScale
      >
        <Icon name="search" class="w-3.5 h-3.5" strokeWidth={1.6} />
      </button>
      <button
        type="button"
        class="p-1 rounded-[4px] transition-colors {chatStore.showArchived ? 'text-[#EDEDEF] bg-white/[0.1]' : 'text-[#66666D] hover:text-[#D4D4D8] hover:bg-white/[0.05]'}"
        onclick={() => (chatStore.showArchived = !chatStore.showArchived)}
        title={chatStore.showArchived ? 'Back to tasks' : `Archived (${chatStore.archivedCount})`}
        use:attachPressScale
      >
        <Icon name="archive" class="w-3.5 h-3.5" strokeWidth={1.6} />
      </button>
    </div>
  </div>

  {#if searchBoxSpring.current > 0.5}
    <div class="px-4 overflow-hidden shrink-0" style="height: {searchBoxSpring.current}px;">
      <input
        bind:value={chatStore.filterSearchQuery}
        type="text"
        placeholder="Filter..."
        class="w-full px-2.5 py-1.5 rounded-[4px] bg-[#101012] border border-white/[0.08] text-[12px] text-[#E5E5E8] placeholder-[#5C5C62] outline-none"
      />
    </div>
  {/if}

  <div data-omni-chat-list="" class="flex-1 overflow-y-auto px-2.5 pb-3 space-y-2">
    {#if !chatStore.showArchived && chatStore.pinnedSessions.length > 0}
      <div class="pb-2 border-b border-white/[0.06] space-y-0.5">
        {#each chatStore.pinnedSessions as chat (chat.id)}
          <div
            data-omni-chat-item=""
            animate:flip={{ duration: 170 }}
            in:pinReceive={{ key: chat.id }}
            out:pinSend={{ key: chat.id }}
            class="group flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[6px] cursor-pointer transition-colors duration-150 {chatStore.activeChatId === chat.id ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#B3B3BA] hover:bg-white/[0.04] hover:text-[#E5E5E8]'}"
            onclick={() => chatStore.selectSession(chat.id)}
            role="button"
            tabindex="0"
            onkeydown={(e) => { if (e.key === 'Enter') chatStore.selectSession(chat.id); }}
          >
            <div class="flex items-center gap-2 min-w-0 flex-1 overflow-hidden">
              {#if chatStore.isChatStreaming(chat.id)}
                <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#EDEDEF] animate-pulse"></span>
              {:else if chat.hasError || chat.hasUnreadCompletion}
                <span class="w-1.5 h-1.5 rounded-full shrink-0 {chat.hasError ? 'bg-[#EF4444]' : 'bg-[#22C55E]'}"></span>
              {/if}

              {#if editingChatId === chat.id}
                <input
                  bind:value={editTitleText}
                  type="text"
                  class="bg-[#101012] border border-white/[0.2] rounded-[4px] px-1.5 py-0.5 text-[12px] text-[#E5E5E8] w-full outline-none"
                  onclick={(e) => e.stopPropagation()}
                  onkeydown={(e) => {
                    if (e.key === 'Enter') handleSaveRename(chat.id);
                    if (e.key === 'Escape') handleCancelRename();
                  }}
                  onblur={() => handleSaveRename(chat.id)}
                />
              {:else}
                <span class="truncate text-[12.5px] font-sans leading-snug transition-all duration-150">{chat.title}</span>
              {/if}
            </div>

            <div class="shrink-0 relative flex items-center justify-end min-w-[68px]">
              <span class="font-mono text-[11px] text-[#606068] opacity-100 group-hover:opacity-0 transition-opacity duration-150 pointer-events-none">
                {formatRelativeAge(chat.updatedAt)}
              </span>
              <div class="absolute right-0 flex items-center gap-0.5 opacity-0 scale-95 group-hover:opacity-100 group-hover:scale-100 transition-all duration-150 pointer-events-none group-hover:pointer-events-auto">
                <button
                  type="button"
                  class="p-0.5 rounded-[3px] text-[#EDEDEF] hover:bg-white/[0.08]"
                  title="Unpin"
                  onclick={(e) => { e.stopPropagation(); chatStore.togglePinSession(chat.id); }}
                >
                  <Icon name="pin" class="w-3 h-3" filled={true} strokeWidth={1.6} />
                </button>
                <button
                  type="button"
                  class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.08]"
                  title="Rename"
                  onclick={(e) => { e.stopPropagation(); handleStartRename(chat.id, chat.title); }}
                >
                  <Icon name="edit" class="w-3 h-3" strokeWidth={1.6} />
                </button>
                <button
                  type="button"
                  class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.08]"
                  title="Archive"
                  onclick={(e) => { e.stopPropagation(); chatStore.toggleArchiveSession(chat.id); }}
                >
                  <Icon name="archive" class="w-3 h-3" strokeWidth={1.6} />
                </button>
                <button
                  type="button"
                  class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#EF4444] hover:bg-white/[0.08]"
                  title="Delete"
                  onclick={(e) => { e.stopPropagation(); chatStore.deleteSession(chat.id); }}
                >
                  <Icon name="trash" class="w-3 h-3" strokeWidth={1.6} />
                </button>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}

    <div>


      {#if !chatStore.showArchived && uniqueProjectPaths.length > 0}
        <div class="px-2 py-1 flex items-center justify-between">
          <span class="text-[11px] font-sans text-[#66666D]">Projects</span>
          <button
            type="button"
            class="text-[11px] text-[#73737A] hover:text-[#E5E5E8] transition-colors"
            onclick={() => workspaceStore.openDirectoryPicker()}
            title="Open another project folder"
          >
            + Folder
          </button>
        </div>
        {#each uniqueProjectPaths as projPath (normProj(projPath))}
          {@const projName = projPath.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || projPath}
          {@const thisProjChats = projectChatsByNorm.get(normProj(projPath)) || []}
          <div class="mb-2">
            <div class="group/proj flex items-center gap-1 mb-0.5 rounded-[4px] hover:bg-white/[0.04] transition-colors pr-1">
              <button
                type="button"
                class="flex-1 min-w-0 px-2 py-1.5 flex items-center gap-2 text-left {normProj(workspaceStore.currentPath) === normProj(projPath) ? 'text-[#EDEDEF]' : 'text-[#A3A3AA] hover:text-white'} transition-colors"
                onclick={() => workspaceStore.setWorkspaceDir(projPath)}
                title={projPath}
              >
                <Icon name="folder" class="w-3.5 h-3.5 text-[#73737A] shrink-0" strokeWidth={1.6} />
                <span class="font-mono text-[12px] truncate">{projName}</span>
              </button>
              <button
                type="button"
                class="w-6 h-6 rounded-[4px] flex items-center justify-center text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.08] transition-colors shrink-0"
                onclick={() => { workspaceStore.setWorkspaceDir(projPath); chatStore.createSession('New task', projPath); }}
                title="New task in {projName}"
                use:attachPressScale
              >
                <Icon name="plus" class="w-3.5 h-3.5" strokeWidth={1.8} />
              </button>
            </div>
            <div class="pl-2 space-y-0.5">
              {#each thisProjChats as chat (chat.id)}
                <div
                  data-omni-chat-item=""
                  animate:flip={{ duration: 170 }}
                  in:pinReceive={{ key: chat.id }}
                  out:pinSend={{ key: chat.id }}
                  class="group flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[6px] cursor-pointer transition-colors duration-150 {chatStore.activeChatId === chat.id ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#B3B3BA] hover:bg-white/[0.04] hover:text-[#E5E5E8]'}"
                  onclick={() => chatStore.selectSession(chat.id)}
                  role="button"
                  tabindex="0"
                  onkeydown={(e) => { if (e.key === 'Enter') chatStore.selectSession(chat.id); }}
                >
                  <div class="flex items-center gap-2 min-w-0 flex-1 overflow-hidden">
                    {#if chatStore.isChatStreaming(chat.id)}
                      <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#EDEDEF] animate-pulse"></span>
                    {:else if chat.hasError || chat.hasUnreadCompletion}
                      <span class="w-1.5 h-1.5 rounded-full shrink-0 {chat.hasError ? 'bg-[#EF4444]' : 'bg-[#22C55E]'}"></span>
                    {/if}
                    {#if editingChatId === chat.id}
                      <input
                        bind:value={editTitleText}
                        type="text"
                        class="bg-[#101012] border border-white/[0.2] rounded-[4px] px-1.5 py-0.5 text-[12px] text-[#E5E5E8] w-full outline-none"
                        onclick={(e) => e.stopPropagation()}
                        onkeydown={(e) => {
                          if (e.key === 'Enter') handleSaveRename(chat.id);
                          if (e.key === 'Escape') handleCancelRename();
                        }}
                        onblur={() => handleSaveRename(chat.id)}
                      />
                    {:else}
                      <span class="truncate text-[12.5px] font-sans leading-snug transition-all duration-150">{chat.title}</span>
                    {/if}
                  </div>
                  <div class="shrink-0 relative flex items-center justify-end min-w-[68px]">
                    <span class="font-mono text-[11px] text-[#606068] opacity-100 group-hover:opacity-0 transition-opacity duration-150 pointer-events-none">
                      {formatRelativeAge(chat.updatedAt)}
                    </span>
                    <div class="absolute right-0 flex items-center gap-0.5 opacity-0 scale-95 group-hover:opacity-100 group-hover:scale-100 transition-all duration-150 pointer-events-none group-hover:pointer-events-auto">
                      <button
                        type="button"
                        class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#EDEDEF] hover:bg-white/[0.08]"
                        title="Pin"
                        onclick={(e) => { e.stopPropagation(); chatStore.togglePinSession(chat.id); }}
                      >
                        <Icon name="pin" class="w-3 h-3" strokeWidth={1.6} />
                      </button>
                      <button
                        type="button"
                        class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.08]"
                        title="Rename"
                        onclick={(e) => { e.stopPropagation(); handleStartRename(chat.id, chat.title); }}
                      >
                        <Icon name="edit" class="w-3 h-3" strokeWidth={1.6} />
                      </button>
                      <button
                        type="button"
                        class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.08]"
                        title="Archive"
                        onclick={(e) => { e.stopPropagation(); chatStore.toggleArchiveSession(chat.id); }}
                      >
                        <Icon name="archive" class="w-3 h-3" strokeWidth={1.6} />
                      </button>
                      <button
                        type="button"
                        class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#EF4444] hover:bg-white/[0.08]"
                        title="Delete"
                        onclick={(e) => { e.stopPropagation(); chatStore.deleteSession(chat.id); }}
                      >
                        <Icon name="trash" class="w-3 h-3" strokeWidth={1.6} />
                      </button>
                    </div>
                  </div>
                </div>
              {/each}
            </div>
          </div>
        {/each}
      {/if}

      {#if uniqueProjectPaths.length > 0 && standaloneChats.length > 0 && !chatStore.showArchived}
        <div class="my-2 border-t border-white/[0.07]"></div>
      {/if}
      <div class="space-y-0.5">
        {#each (chatStore.showArchived ? visibleRecent : standaloneChats) as chat (chat.id)}
          <div
            data-omni-chat-item=""
            animate:flip={{ duration: 170 }}
            in:pinReceive={{ key: chat.id }}
            out:pinSend={{ key: chat.id }}
            class="group flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[6px] cursor-pointer transition-colors duration-150 {chatStore.activeChatId === chat.id ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#B3B3BA] hover:bg-white/[0.04] hover:text-[#E5E5E8]'}"
            onclick={() => chatStore.selectSession(chat.id)}
            role="button"
            tabindex="0"
            onkeydown={(e) => { if (e.key === 'Enter') chatStore.selectSession(chat.id); }}
          >
            <div class="flex items-center gap-2 min-w-0 flex-1 overflow-hidden">
              {#if chatStore.isChatStreaming(chat.id)}
                <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#EDEDEF] animate-pulse"></span>
              {:else if chat.hasError || chat.hasUnreadCompletion}
                <span class="w-1.5 h-1.5 rounded-full shrink-0 {chat.hasError ? 'bg-[#EF4444]' : 'bg-[#22C55E]'}"></span>
              {/if}

              {#if editingChatId === chat.id}
                <input
                  bind:value={editTitleText}
                  type="text"
                  class="bg-[#101012] border border-white/[0.2] rounded-[4px] px-1.5 py-0.5 text-[12px] text-[#E5E5E8] w-full outline-none"
                  onclick={(e) => e.stopPropagation()}
                  onkeydown={(e) => {
                    if (e.key === 'Enter') handleSaveRename(chat.id);
                    if (e.key === 'Escape') handleCancelRename();
                  }}
                  onblur={() => handleSaveRename(chat.id)}
                />
              {:else}
                <span class="truncate text-[12.5px] font-sans leading-snug transition-all duration-150">{chat.title}</span>
              {/if}
            </div>

            <div class="shrink-0 relative flex items-center justify-end min-w-[68px]">
              <span class="font-mono text-[11px] text-[#606068] opacity-100 group-hover:opacity-0 transition-opacity duration-150 pointer-events-none">
                {formatRelativeAge(chat.updatedAt)}
              </span>
              <div class="absolute right-0 flex items-center gap-0.5 opacity-0 scale-95 group-hover:opacity-100 group-hover:scale-100 transition-all duration-150 pointer-events-none group-hover:pointer-events-auto">
                {#if !chat.archived}
                  <button
                    type="button"
                    class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#EDEDEF] hover:bg-white/[0.08]"
                    title="Pin"
                    onclick={(e) => { e.stopPropagation(); chatStore.togglePinSession(chat.id); }}
                  >
                    <Icon name="pin" class="w-3 h-3" strokeWidth={1.6} />
                  </button>
                {/if}
                <button
                  type="button"
                  class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.08]"
                  title="Rename"
                  onclick={(e) => { e.stopPropagation(); handleStartRename(chat.id, chat.title); }}
                >
                  <Icon name="edit" class="w-3 h-3" strokeWidth={1.6} />
                </button>
                <button
                  type="button"
                  class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.08]"
                  title={chat.archived ? 'Unarchive' : 'Archive'}
                  onclick={(e) => { e.stopPropagation(); chatStore.toggleArchiveSession(chat.id); }}
                >
                  <Icon name="archive" class="w-3 h-3" strokeWidth={1.6} />
                </button>
                <button
                  type="button"
                  class="p-0.5 rounded-[3px] text-[#66666D] hover:text-[#EF4444] hover:bg-white/[0.08]"
                  title="Delete"
                  onclick={(e) => { e.stopPropagation(); chatStore.deleteSession(chat.id); }}
                >
                  <Icon name="trash" class="w-3 h-3" strokeWidth={1.6} />
                </button>
              </div>
            </div>
          </div>
        {/each}
      </div>

      {#if hasMoreSessions}
        <button
          type="button"
          class="mt-1.5 pl-7 pr-2 py-1 text-[11.5px] font-sans text-[#6B6B72] hover:text-[#D4D4D8] transition-colors"
          onclick={() => (showAllSessions = true)}
        >
          Show more
        </button>
      {:else if showAllSessions && chatStore.recentSessions.length > INITIAL_LIMIT}
        <button
          type="button"
          class="mt-1.5 pl-7 pr-2 py-1 text-[11.5px] font-sans text-[#6B6B72] hover:text-[#D4D4D8] transition-colors"
          onclick={() => (showAllSessions = false)}
        >
          Show less
        </button>
      {/if}
    </div>
  </div>

  <div class="px-4 py-3 flex items-center justify-between shrink-0">
    <button
      type="button"
      data-omni-button="settings"
      class="p-1.5 rounded-[4px] text-[#73737A] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
      onclick={onOpenSettings || (() => settingsStore.openSettings())}
      title="Settings (Ctrl+,)"
      use:attachPressScale
    >
      <Icon name="settings" class="w-4 h-4" strokeWidth={1.6} />
    </button>
  </div>
</aside>

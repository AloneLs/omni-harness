<script lang="ts">
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

let projCtxMenu = $state<{ open: boolean; x: number; y: number; path: string } | null>(null);

function openProjCtxMenu(e: MouseEvent, path: string) {
  e.preventDefault();
  e.stopPropagation();
  const x = Math.min(e.clientX, Math.max(8, window.innerWidth - 196));
  const y = Math.min(e.clientY, Math.max(8, window.innerHeight - 180));
  projCtxMenu = { open: true, x, y, path };
}

let editingChatId = $state<string | null>(null);
let editTitleText = $state('');
let showSearchInput = $state(false);
let localSearch = $state(chatStore.filterSearchQuery);
let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null;
let visibleStandaloneLimit = $state(24);
let visibleProjectLimits = $state<Record<string, number>>({});

const searchBoxSpring = new Spring(0, SPRING_POPUP);

$effect(() => {
  searchBoxSpring.target = showSearchInput ? 38 : 0;
});

function handleSearchInput(val: string) {
  localSearch = val;
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  searchDebounceTimer = setTimeout(() => {
    searchDebounceTimer = null;
    chatStore.filterSearchQuery = val;
  }, 160);
}

$effect(() => {
  return () => {
    if (searchDebounceTimer) clearTimeout(searchDebounceTimer);
  };
});

let collapsedProjects = $state<Record<string, boolean>>((() => {
  try {
    const raw = typeof localStorage !== 'undefined' ? localStorage.getItem('omni_collapsed_projects') : null;
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
})());

function toggleProjectCollapsed(projPath: string) {
  const k = normProj(projPath);
  collapsedProjects = { ...collapsedProjects, [k]: !collapsedProjects[k] };
  try {
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('omni_collapsed_projects', JSON.stringify(collapsedProjects));
    }
  } catch {}
}

function normProj(p?: string): string {
  return (p || '').replace(/[\\/]+$/, '').toLowerCase();
}

let projectChatsByNorm = $derived.by(() => {
  const grouped = new Map<string, typeof chatStore.recentSessions>();
  for (const c of chatStore.recentSessions) {
    if (!c.projectPath) continue;
    const k = normProj(c.projectPath);
    const list = grouped.get(k);
    if (list) list.push(c);
    else grouped.set(k, [c]);
  }
  for (const list of grouped.values()) {
    list.sort((a, b) => Number(Boolean(b.pinned)) - Number(Boolean(a.pinned)) || b.updatedAt - a.updatedAt);
  }
  return grouped;
});

let uniqueProjectPaths = $derived.by(() => {
  const map = new Map<string, string>();
  for (const rp of workspaceStore.recentWorkspaces) {
    if (rp) {
      const k = normProj(rp);
      if (!map.has(k)) map.set(k, rp);
    }
  }
  for (const c of chatStore.recentSessions) {
    if (c.projectPath) {
      const k = normProj(c.projectPath);
      if (!map.has(k)) map.set(k, c.projectPath);
    }
  }
  if (workspaceStore.currentPath) {
    const k = normProj(workspaceStore.currentPath);
    if (!map.has(k)) map.set(k, workspaceStore.currentPath);
  }
  return Array.from(map.values());
});

let allStandaloneChats = $derived(
  chatStore.showArchived ? chatStore.recentSessions : chatStore.recentSessions.filter((c) => !c.projectPath)
);

let visibleStandaloneChats = $derived.by(() => {
  if (chatStore.filterSearchQuery.trim().length > 0) {
    return allStandaloneChats.slice(0, 60);
  }
  return allStandaloneChats.slice(0, visibleStandaloneLimit);
});

let hasMoreSessions = $derived(
  chatStore.filterSearchQuery.trim().length === 0 && allStandaloneChats.length > visibleStandaloneLimit
);

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
          if (!showSearchInput) {
            localSearch = '';
            chatStore.filterSearchQuery = '';
          }
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
        value={localSearch}
        oninput={(e) => handleSearchInput((e.currentTarget as HTMLInputElement).value)}
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
                <span class="truncate text-[12.5px] font-sans leading-snug">{chat.title}</span>
              {/if}
            </div>

            <div class="shrink-0 relative flex items-center justify-end min-w-[68px]">
              <span class="font-mono text-[11px] text-[#606068] opacity-100 group-hover:opacity-0 transition-opacity duration-150 pointer-events-none">
                {formatRelativeAge(chat.updatedAt)}
              </span>
              <div class="absolute right-0 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-150 pointer-events-none group-hover:pointer-events-auto">
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
        <div class="px-2 pt-1 pb-1.5 flex items-center justify-between">
          <span class="text-[12px] font-sans font-medium text-[#8E8E96]">Projects</span>
          <div class="flex items-center gap-1">
            <button
              type="button"
              class="p-1 rounded-[4px] transition-colors {showSearchInput ? 'text-[#E5E5E8] bg-white/[0.08]' : 'text-[#66666D] hover:text-[#D4D4D8] hover:bg-white/[0.05]'}"
              onclick={() => {
                showSearchInput = !showSearchInput;
                if (!showSearchInput) {
                  localSearch = '';
                  chatStore.filterSearchQuery = '';
                }
              }}
              title="Filter projects and chats"
              use:attachPressScale
            >
              <Icon name="filter" class="w-3.5 h-3.5" strokeWidth={1.6} />
            </button>
            <button
              type="button"
              class="p-1 rounded-[4px] text-[#66666D] hover:text-[#D4D4D8] hover:bg-white/[0.05] transition-colors"
              onclick={() => workspaceStore.openDirectoryPicker()}
              title="Open project folder"
              use:attachPressScale
            >
              <Icon name="folder-plus" class="w-3.5 h-3.5" strokeWidth={1.6} />
            </button>
          </div>
        </div>
        {#each uniqueProjectPaths as projPath (normProj(projPath))}
          {@const projNorm = normProj(projPath)}
          {@const projName = projPath.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || projPath}
          {@const thisProjChats = projectChatsByNorm.get(projNorm) || []}
          {@const defaultLimit = 8}
          {@const isExpandedAll = Boolean(visibleProjectLimits[projNorm])}
          {@const visibleProjChats = isExpandedAll ? thisProjChats : thisProjChats.slice(0, defaultLimit)}
          {@const isProjCollapsed = Boolean(collapsedProjects[projNorm])}
          <div data-omni-project-group="" class="mb-2.5">
            <div
              data-omni-project-header=""
              class="group/proj flex items-center gap-1 px-2 py-1.5 rounded-[7px] hover:bg-white/[0.04] transition-colors"
              oncontextmenu={(e) => openProjCtxMenu(e, projPath)}
              role="presentation"
            >
              <button
                type="button"
                class="flex-1 min-w-0 flex items-center gap-2 text-left {normProj(workspaceStore.currentPath) === projNorm ? 'text-[#D4D4D8]' : 'text-[#8E8E96] hover:text-[#E5E5E8]'} transition-colors"
                onclick={() => {
                  toggleProjectCollapsed(projPath);
                  workspaceStore.setWorkspaceDir(projPath);
                }}
                title={projPath}
              >
                <Icon name="folder" class="w-3.5 h-3.5 text-[#72727A] shrink-0" strokeWidth={1.5} />
                <span class="font-sans text-[13px] truncate">{projName}</span>
              </button>
              <button
                type="button"
                class="w-5 h-5 rounded-[4px] flex items-center justify-center text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.08] opacity-0 group-hover/proj:opacity-100 transition-all shrink-0"
                onclick={(e) => { e.stopPropagation(); workspaceStore.setWorkspaceDir(projPath); chatStore.createSession('', projPath); }}
                title="New task in {projName}"
                use:attachPressScale
              >
                <Icon name="plus" class="w-3.5 h-3.5" strokeWidth={1.8} />
              </button>
            </div>
            {#if !isProjCollapsed}
            <div data-omni-project-chats="" class="mt-0.5 space-y-0.5">
              {#if thisProjChats.length === 0}
                <div class="pl-7 pr-2 py-1.5 text-[12.5px] font-sans text-[#686870]">No conversations yet</div>
              {:else}
                {#each visibleProjChats as chat (chat.id)}
                  <div
                    data-omni-chat-item=""
                    class="group flex items-center justify-between gap-2 pl-7 pr-2.5 py-1.5 rounded-[8px] border cursor-pointer transition-colors duration-150 {chatStore.activeChatId === chat.id ? 'bg-white/[0.06] border-white/[0.12] text-[#F5F5F7] font-medium' : 'border-transparent text-[#C8C8CE] hover:bg-white/[0.04] hover:text-[#F5F5F7]'}"
                    onclick={() => chatStore.selectSession(chat.id)}
                    role="button"
                    tabindex="0"
                    onkeydown={(e) => { if (e.key === 'Enter') chatStore.selectSession(chat.id); }}
                  >
                    <div class="flex items-center gap-1.5 min-w-0 flex-1 overflow-hidden">
                      {#if chatStore.isChatStreaming(chat.id)}
                        <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#EDEDEF] animate-pulse"></span>
                      {:else if chat.hasError || chat.hasUnreadCompletion}
                        <span class="w-1.5 h-1.5 rounded-full shrink-0 {chat.hasError ? 'bg-[#EF4444]' : 'bg-[#22C55E]'}"></span>
                      {/if}
                      {#if editingChatId === chat.id}
                        <input
                          bind:value={editTitleText}
                          type="text"
                          class="bg-[#101012] border border-white/[0.2] rounded-[4px] px-1.5 py-0.5 text-[12.5px] text-[#E5E5E8] w-full outline-none"
                          onclick={(e) => e.stopPropagation()}
                          onkeydown={(e) => {
                            if (e.key === 'Enter') handleSaveRename(chat.id);
                            if (e.key === 'Escape') handleCancelRename();
                          }}
                          onblur={() => handleSaveRename(chat.id)}
                        />
                      {:else}
                        <span class="truncate text-[13px] font-sans leading-snug">{chat.title}</span>
                      {/if}
                    </div>
                    <div class="shrink-0 relative flex items-center justify-end min-w-[28px]">
                      <span class="font-sans text-[12px] text-[#7A7A82] tabular-nums opacity-100 group-hover:opacity-0 transition-opacity duration-150 pointer-events-none">
                        {formatRelativeAge(chat.updatedAt)}
                      </span>
                      <div class="absolute right-0 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-150 pointer-events-none group-hover:pointer-events-auto bg-[#171719]/90 rounded-[4px] pl-1">
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
                {#if thisProjChats.length > defaultLimit}
                  <button
                    type="button"
                    class="pl-7 py-1.5 text-[12.5px] font-sans text-[#787880] hover:text-[#D4D4D8] transition-colors"
                    onclick={() => (visibleProjectLimits = { ...visibleProjectLimits, [projNorm]: isExpandedAll ? 0 : thisProjChats.length })}
                  >
                    {isExpandedAll ? 'See less' : 'See more'}
                  </button>
                {/if}
              {/if}
            </div>
            {/if}
          </div>
        {/each}
      {/if}

      {#if visibleStandaloneChats.length > 0 && !chatStore.showArchived}
        {#if uniqueProjectPaths.length > 0}
          <div class="my-2 border-t border-white/[0.07]"></div>
        {/if}
        <div data-omni-standalone-header="" class="px-2 py-1 text-[11px] font-sans text-[#66666D]">Без проекта</div>
      {/if}
      <div class="space-y-0.5">
        {#each visibleStandaloneChats as chat (chat.id)}
          <div
            data-omni-chat-item=""
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
                <span class="truncate text-[12.5px] font-sans leading-snug">{chat.title}</span>
              {/if}
            </div>

            <div class="shrink-0 relative flex items-center justify-end min-w-[68px]">
              <span class="font-mono text-[11px] text-[#606068] opacity-100 group-hover:opacity-0 transition-opacity duration-150 pointer-events-none">
                {formatRelativeAge(chat.updatedAt)}
              </span>
              <div class="absolute right-0 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-150 pointer-events-none group-hover:pointer-events-auto">
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
          onclick={() => (visibleStandaloneLimit += 40)}
        >
          Show more ({allStandaloneChats.length - visibleStandaloneLimit})
        </button>
      {:else if visibleStandaloneLimit > 24 && allStandaloneChats.length > 24}
        <button
          type="button"
          class="mt-1.5 pl-7 pr-2 py-1 text-[11.5px] font-sans text-[#6B6B72] hover:text-[#D4D4D8] transition-colors"
          onclick={() => (visibleStandaloneLimit = 24)}
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

<svelte:window onclick={() => { if (projCtxMenu?.open) projCtxMenu = null; }} />

{#if projCtxMenu?.open}
  <div
    data-omni-context-menu=""
    role="menu"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => { if (e.key === 'Escape') projCtxMenu = null; }}
    class="fixed z-[910] w-[192px] py-1 rounded-[8px] bg-[#161618]/98 backdrop-blur-md border border-white/[0.1] shadow-2xl text-xs text-[#E5E5E8] select-none"
    style="left: {projCtxMenu.x}px; top: {projCtxMenu.y}px;"
  >
    <button
      type="button"
      onclick={() => { if (projCtxMenu) workspaceStore.setWorkspaceDir(projCtxMenu.path); projCtxMenu = null; }}
      class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07]"
    >
      Выбрать проект
    </button>
    <button
      type="button"
      onclick={() => { if (projCtxMenu) { workspaceStore.setWorkspaceDir(projCtxMenu.path); chatStore.createSession('', projCtxMenu.path); } projCtxMenu = null; }}
      class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07]"
    >
      Новый чат в проекте
    </button>
    <button
      type="button"
      onclick={() => { if (projCtxMenu) invoke('open_path_in_explorer', { path: projCtxMenu.path }).catch(() => {}); projCtxMenu = null; }}
      class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07]"
    >
      Открыть в проводнике
    </button>
    <button
      type="button"
      onclick={() => { if (projCtxMenu) navigator.clipboard.writeText(projCtxMenu.path).catch(() => {}); projCtxMenu = null; }}
      class="w-full px-3 py-1.5 text-left hover:bg-white/[0.07]"
    >
      Копировать путь
    </button>
    <div class="my-1 border-t border-white/[0.06]"></div>
    <button
      type="button"
      onclick={() => { if (projCtxMenu) workspaceStore.removeRecentWorkspace(projCtxMenu.path); projCtxMenu = null; }}
      class="w-full px-3 py-1.5 text-left text-[#F87171] hover:bg-[#EF4444]/15"
    >
      Удалить из списка
    </button>
  </div>
{/if}

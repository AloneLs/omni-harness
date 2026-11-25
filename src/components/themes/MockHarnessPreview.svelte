<script lang="ts">
import { setContext } from 'svelte';
import Icon from '../Icon.svelte';
import MessageItem from '../MessageItem.svelte';
import { compileThemeCss, type OmniTheme } from '../../lib/stores/theme.svelte';
import { settingsStore } from '../../lib/stores/settings.svelte';
import { formatToolExecution } from '../../lib/toolMapper';
import type { ChatMessage } from '../../lib/stores/chat.svelte';

let {
  theme,
  fullscreen = false
}: {
  theme: OmniTheme | null;
  fullscreen?: boolean;
} = $props();

setContext('omni-preview-only', true);

interface MockSession {
  id: string;
  title: string;
  time: string;
  pinned?: boolean;
  status?: 'streaming' | 'success' | 'error';
}

const INITIAL_MOCK_CHATS: MockSession[] = [
  { id: 'm1', title: 'Refactor shader pipeline', time: '2m', pinned: true, status: 'streaming' },
  { id: 'm2', title: 'Optimize Netty packet codec', time: '18m', status: 'success' },
  { id: 'm3', title: 'Fix memory leak in texture pool', time: '1h', status: 'error' },
  { id: 'm4', title: 'Antigravity OAuth refresh jitter', time: '3h' }
];

let mockChats = $state<MockSession[]>(INITIAL_MOCK_CHATS);
let selectedMockChat = $state('m1');
let mockSidebarOpen = $state(true);
let showSearch = $state(false);
let searchQuery = $state('');
let mockPlusOpen = $state(false);
let mockModelOpen = $state(false);
let mockInput = $state('');

const MOCK_READ_REC = {
  ...formatToolExecution({
    id: 'tc1',
    name: 'read_file',
    arguments: '{"path":"src/render/frame_timer.rs"}',
    status: 'success',
    startedAt: Date.now() - 18000,
    finishedAt: Date.now() - 16000,
    output: 'fn present_frame(&mut self, vsync: bool) { ... }'
  }),
  elapsedMs: 12
};

const MOCK_EDIT_REC = {
  ...formatToolExecution({
    id: 'tc2',
    name: 'edit_file',
    arguments: '{"path":"src/render/frame_timer.rs"}',
    status: 'success',
    startedAt: Date.now() - 16000,
    finishedAt: Date.now() - 14000,
    output: JSON.stringify({ path: 'src/render/frame_timer.rs', snapshotId: 'mock-snap-1', addedLines: 4, removedLines: 2 })
  }),
  diff: {
    filePath: 'src/render/frame_timer.rs',
    changes: [
      { type: 'del' as const, text: 'while timer.elapsed() < target_dt { std::hint::spin_loop(); }' },
      { type: 'add' as const, text: 'timer.wait_for_vblank(target_dt);' }
    ]
  },
  elapsedMs: 19
};

let mockMessages = $state<ChatMessage[]>([
  {
    id: 'mock-u1',
    chatId: 'mock',
    role: 'user',
    content: 'Check the render loop and show me the diff for frame pacing.',
    createdAt: Date.now() - 25000
  },
  {
    id: 'mock-a1',
    chatId: 'mock',
    role: 'assistant',
    content: 'Updated the swapchain pacing threshold and removed the redundant spin-wait loop in `src/render/frame_timer.rs`.\n\n```rust\nfn present_frame(&mut self, vsync: bool) {\n    self.swapchain.acquire_next();\n    timer.wait_for_vblank(target_dt);\n}\n```\n\n- Frame time variance dropped from `3.4ms` to `0.3ms`.\n- VSync frame delivery jitter is eliminated.',
    reasoningContent: 'Inspecting frame_timer.rs and comparing swapchain present interval against target monitor refresh rate.',
    startedAt: Date.now() - 22000,
    finishedAt: Date.now() - 8000,
    createdAt: Date.now() - 8000,
    steps: [
      { kind: 'thought', id: 'th-1', text: 'Inspecting frame_timer.rs and comparing swapchain present interval against target monitor refresh rate.', startedAt: Date.now() - 22000, finishedAt: Date.now() - 18000 },
      { kind: 'tool', record: MOCK_READ_REC },
      { kind: 'tool', record: MOCK_EDIT_REC },
      {
        kind: 'message',
        id: 'msg-final-1',
        text: 'Updated the swapchain pacing threshold and removed the redundant spin-wait loop in `src/render/frame_timer.rs`.\n\n```rust\nfn present_frame(&mut self, vsync: bool) {\n    self.swapchain.acquire_next();\n    timer.wait_for_vblank(target_dt);\n}\n```\n\n- Frame time variance dropped from `3.4ms` to `0.3ms`.\n- VSync frame delivery jitter is eliminated.'
      }
    ],
    toolCalls: [MOCK_READ_REC, MOCK_EDIT_REC]
  }
]);

let scopeAttr = $derived(fullscreen ? 'data-omni-fullscreen-preview-root' : 'data-omni-preview-root');
let scopedCss = $derived(compileThemeCss(theme, `[${scopeAttr}]`));

function t(key: string, fallback: string): string {
  const val = theme?.textOverrides?.[key];
  return val && val.trim() ? val : fallback;
}

let pinnedChats = $derived(mockChats.filter((c) => c.pinned));
let recentChats = $derived(
  mockChats
    .filter((c) => !c.pinned)
    .filter((c) => !searchQuery.trim() || c.title.toLowerCase().includes(searchQuery.toLowerCase()))
);

function togglePin(id: string) {
  mockChats = mockChats.map((c) => (c.id === id ? { ...c, pinned: !c.pinned } : c));
}

function removeChat(id: string) {
  mockChats = mockChats.filter((c) => c.id !== id);
}

function sendMock() {
  const txt = mockInput.trim();
  if (!txt) return;
  mockInput = '';
  const now = Date.now();
  mockMessages = [
    ...mockMessages,
    { id: `mock-u-${now}`, chatId: 'mock', role: 'user', content: txt, createdAt: now },
    {
      id: `mock-a-${now + 1}`,
      chatId: 'mock',
      role: 'assistant',
      content: `Theme preview execution completed for: "${txt}". All tokens and styles applied.`,
      reasoningContent: 'Verified CSS scoping, selector tree and reactive variables.',
      startedAt: now,
      finishedAt: now + 800,
      createdAt: now + 800,
      steps: [
        { kind: 'thought', id: `th-${now}`, text: 'Verified CSS scoping, selector tree and reactive variables.', startedAt: now, finishedAt: now + 800 },
        { kind: 'message', id: `m-${now}`, text: `Theme preview execution completed for: "${txt}". All tokens and styles applied.` }
      ]
    }
  ];
}
</script>

{#if scopedCss}
  {@html `<style>${scopedCss.replace(/<\/style/gi, '<\\/style')}</style>`}
{/if}

<div
  data-omni-preview-root={!fullscreen ? '' : undefined}
  data-omni-fullscreen-preview-root={fullscreen ? '' : undefined}
  data-omni-app=""
  class="w-full h-full flex bg-[#101012] text-[#E5E5E8] overflow-hidden select-none font-sans"
>
  {#if mockSidebarOpen}
    <aside data-omni-sidebar="" class="w-[240px] h-full bg-[#171719] border-r border-white/[0.06] flex flex-col select-none shrink-0 overflow-hidden">
      <div data-omni-sidebar-header="" class="h-11 px-3 border-b border-white/[0.06] flex items-center justify-between shrink-0">
        <div class="flex items-center gap-1.5">
          <button
            type="button"
            onclick={() => (mockSidebarOpen = false)}
            class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
            title="Collapse Sidebar"
          >
            <Icon name="sidebar" class="w-4 h-4" strokeWidth={1.5} />
          </button>
          <button
            type="button"
            data-omni-button="new-chat"
            class="flex items-center gap-2 text-[13px] font-sans text-[#D4D4D8] hover:text-white transition-colors"
            onclick={() => {
              const now = Date.now();
              const newId = `m-${now}`;
              mockChats = [{ id: newId, title: `New Task ${mockChats.length + 1}`, time: '1m' }, ...mockChats];
              selectedMockChat = newId;
            }}
          >
            <Icon name="plus-circle" class="w-4 h-4 text-[#8A8A92] shrink-0" strokeWidth={1.5} />
            <span>{t('new_task', 'New task')}</span>
          </button>
        </div>
        <span class="font-mono text-[11px] text-[#5C5C62] shrink-0 pl-2 pointer-events-none">Ctrl+N</span>
      </div>

      <div class="px-4 pt-3.5 pb-2 flex items-center justify-between shrink-0">
        <span class="text-[12px] font-sans font-medium text-[#A3A3AA]">{t('task_list', 'Task list')}</span>
        <div class="flex items-center gap-1 shrink-0">
          <button
            type="button"
            class="p-1 rounded-[4px] transition-colors {showSearch ? 'text-[#E5E5E8] bg-white/[0.08]' : 'text-[#66666D] hover:text-[#D4D4D8] hover:bg-white/[0.05]'}"
            onclick={() => (showSearch = !showSearch)}
            title="Search tasks"
          >
            <Icon name="search" class="w-3.5 h-3.5" strokeWidth={1.6} />
          </button>
          <button
            type="button"
            class="p-1 rounded-[4px] text-[#66666D] hover:text-[#D4D4D8] hover:bg-white/[0.05] transition-colors"
            title="Archive"
          >
            <Icon name="archive" class="w-3.5 h-3.5" strokeWidth={1.6} />
          </button>
        </div>
      </div>

      {#if showSearch}
        <div data-omni-search-box="" class="px-4 pb-2 shrink-0">
          <input
            bind:value={searchQuery}
            type="text"
            placeholder={t('search_chats', 'Search chats...')}
            class="w-full px-2.5 py-1 rounded-[4px] bg-[#101012] border border-white/[0.08] text-[12px] text-[#E5E5E8] placeholder-[#5C5C62] outline-none"
          />
        </div>
      {/if}

      <div data-omni-chat-list="" class="flex-1 overflow-y-auto px-2.5 pb-3 space-y-2">
        {#if pinnedChats.length > 0}
          <div data-omni-pinned-list="" class="pb-2 border-b border-white/[0.06] space-y-0.5">
            {#each pinnedChats as chat (chat.id)}
              <div
                data-omni-chat-item=""
                class="group flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[6px] cursor-pointer transition-colors duration-150 {selectedMockChat === chat.id ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#B3B3BA] hover:bg-white/[0.04] hover:text-[#E5E5E8]'}"
                onclick={() => (selectedMockChat = chat.id)}
                role="button"
                tabindex="0"
                onkeydown={(e) => { if (e.key === 'Enter') selectedMockChat = chat.id; }}
              >
                <div class="flex items-center gap-2 min-w-0 flex-1 overflow-hidden">
                  {#if chat.status === 'streaming'}
                    <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#EDEDEF] animate-pulse"></span>
                  {:else if chat.status === 'error'}
                    <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#EF4444]"></span>
                  {:else if chat.status === 'success'}
                    <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#22C55E]"></span>
                  {/if}
                  <span class="truncate text-[12.5px] font-sans leading-snug">{chat.title}</span>
                </div>
                <div class="shrink-0 relative flex items-center justify-end min-w-[50px]">
                  <span class="font-mono text-[11px] text-[#606068] group-hover:opacity-0 transition-opacity duration-150">{chat.time}</span>
                  <div class="absolute right-0 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-150">
                    <button
                      type="button"
                      class="p-0.5 rounded-[3px] text-[#EDEDEF] hover:bg-white/[0.08]"
                      onclick={(e) => { e.stopPropagation(); togglePin(chat.id); }}
                      title="Unpin"
                    >
                      <Icon name="pin" class="w-3 h-3" filled={true} strokeWidth={1.6} />
                    </button>
                    <button
                      type="button"
                      class="p-0.5 rounded-[3px] text-[#71717A] hover:text-[#EF4444] hover:bg-white/[0.08]"
                      onclick={(e) => { e.stopPropagation(); removeChat(chat.id); }}
                      title="Delete"
                    >
                      <Icon name="trash" class="w-3 h-3" strokeWidth={1.6} />
                    </button>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        {/if}

        <div class="space-y-0.5">
          {#each recentChats as chat (chat.id)}
            <div
              data-omni-chat-item=""
              class="group flex items-center justify-between gap-2 px-2.5 py-1.5 rounded-[6px] cursor-pointer transition-colors duration-150 {selectedMockChat === chat.id ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#B3B3BA] hover:bg-white/[0.04] hover:text-[#E5E5E8]'}"
              onclick={() => (selectedMockChat = chat.id)}
              role="button"
              tabindex="0"
              onkeydown={(e) => { if (e.key === 'Enter') selectedMockChat = chat.id; }}
            >
              <div class="flex items-center gap-2 min-w-0 flex-1 overflow-hidden">
                {#if chat.status === 'streaming'}
                  <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#EDEDEF] animate-pulse"></span>
                {:else if chat.status === 'error'}
                  <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#EF4444]"></span>
                {:else if chat.status === 'success'}
                  <span class="w-1.5 h-1.5 rounded-full shrink-0 bg-[#22C55E]"></span>
                {/if}
                <span class="truncate text-[12.5px] font-sans leading-snug">{chat.title}</span>
              </div>
              <div class="shrink-0 relative flex items-center justify-end min-w-[50px]">
                <span class="font-mono text-[11px] text-[#606068] group-hover:opacity-0 transition-opacity duration-150">{chat.time}</span>
                <div class="absolute right-0 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-150">
                  <button
                    type="button"
                    class="p-0.5 rounded-[3px] text-[#71717A] hover:text-white hover:bg-white/[0.08]"
                    onclick={(e) => { e.stopPropagation(); togglePin(chat.id); }}
                    title="Pin"
                  >
                    <Icon name="pin" class="w-3 h-3" strokeWidth={1.6} />
                  </button>
                  <button
                    type="button"
                    class="p-0.5 rounded-[3px] text-[#71717A] hover:text-[#EF4444] hover:bg-white/[0.08]"
                    onclick={(e) => { e.stopPropagation(); removeChat(chat.id); }}
                    title="Delete"
                  >
                    <Icon name="trash" class="w-3 h-3" strokeWidth={1.6} />
                  </button>
                </div>
              </div>
            </div>
          {/each}
        </div>
      </div>

      <div class="p-2.5 border-t border-white/[0.06] shrink-0">
        <button
          type="button"
          data-omni-button="settings"
          class="w-full flex items-center gap-2 px-2.5 py-1.5 rounded-[6px] text-xs text-[#A3A3AA] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors"
        >
          <Icon name="settings" class="w-3.5 h-3.5" strokeWidth={1.6} />
          <span>{t('settings', 'Settings')}</span>
        </button>
      </div>
    </aside>
  {/if}

  <section class="flex-1 flex flex-col h-full min-w-0 relative">
    {#if !fullscreen}
      <div
        data-omni-titlebar=""
        class="h-11 w-full flex items-center justify-between pl-3 pr-2 shrink-0 z-40 relative bg-[#101012] border-b border-white/[0.06]"
      >
        <div class="flex items-center gap-2">
          {#if !mockSidebarOpen}
            <button
              type="button"
              onclick={() => (mockSidebarOpen = true)}
              class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
              title="Expand Sidebar"
            >
              <Icon name="sidebar" class="w-4 h-4" strokeWidth={1.5} />
            </button>
          {/if}
          <span class="font-mono text-[11px] text-[#71717A] tracking-wider uppercase">OmniHarness</span>
        </div>

        <div class="flex items-center gap-1">
          <button type="button" class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors" title="Minimize">
            <Icon name="minimize" class="w-3.5 h-3.5" strokeWidth={1.6} />
          </button>
          <button type="button" class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors" title="Maximize">
            <Icon name="maximize" class="w-3.5 h-3.5" strokeWidth={1.6} />
          </button>
          <button type="button" class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-white hover:bg-[#EF4444]/85 transition-colors" title="Close">
            <Icon name="close" class="w-3.5 h-3.5" strokeWidth={1.6} />
          </button>
        </div>
      </div>
    {/if}

    <div data-omni-main="" class="flex-1 flex min-h-0 overflow-hidden relative z-10">
      <main data-omni-chat-view="" class="flex-1 flex flex-col min-h-0 overflow-hidden relative bg-[#101012]">
        <div data-omni-messages="" class="flex-1 min-h-0 overflow-y-auto px-6 pt-3 pb-4">
          <div class="max-w-[720px] mx-auto space-y-3">
            {#each mockMessages as message (message.id)}
              <MessageItem {message} />
            {/each}
          </div>
        </div>

        <div class="shrink-0 px-6 pb-5 pt-2 relative">
          <div data-omni-composer="" class="relative w-full max-w-[720px] mx-auto select-none">
            <div data-omni-composer-box="" class="rounded-[10px] border p-3 transition-colors duration-150 border-white/[0.07] bg-[#1C1C1F]/95">
              <textarea
                data-omni-composer-input=""
                bind:value={mockInput}
                onkeydown={(e) => { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); sendMock(); } }}
                placeholder={t('ask_placeholder', 'Ask OmniHarness anything, @ to add context, / for commands or capabilities')}
                rows="1"
                class="w-full px-1 bg-transparent font-sans text-[13.5px] text-[#E5E5E8] placeholder-[#686870] outline-none focus:outline-none focus:ring-0 resize-none min-h-[24px] max-h-[180px] leading-relaxed"
              ></textarea>

              <div class="flex items-center justify-between pt-1.5 select-none gap-2">
                <div class="relative">
                  <button
                    type="button"
                    data-omni-button="plus"
                    onclick={() => { mockModelOpen = false; mockPlusOpen = !mockPlusOpen; }}
                    class="w-7 h-7 -ml-0.5 rounded-[4px] flex items-center justify-center text-[#8E8E95] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
                  >
                    <Icon name="plus" class="w-4 h-4" strokeWidth={1.6} />
                  </button>
                  {#if mockPlusOpen}
                    <div class="absolute bottom-full mb-2 left-0 w-48 bg-[#18181B] border border-white/[0.08] rounded-[6px] p-1.5 z-50 text-xs space-y-1">
                      <div class="px-2 py-1 text-[#C4C4CA] rounded-[4px] hover:bg-white/[0.06]">Add context (@)</div>
                      <div class="px-2 py-1 text-[#C4C4CA] rounded-[4px] hover:bg-white/[0.06]">Slash commands (/)</div>
                      <div class="px-2 py-1 text-[#C4C4CA] rounded-[4px] hover:bg-white/[0.06]">Attach files</div>
                    </div>
                  {/if}
                </div>

                <div class="flex items-center gap-2.5">
                  <div class="relative group/ctx flex items-center justify-center">
                    <div class="w-6 h-6 flex items-center justify-center rounded-[4px] hover:bg-white/[0.04] transition-colors">
                      <svg class="w-4 h-4 -rotate-90" viewBox="0 0 16 16" fill="none">
                        <circle cx="8" cy="8" r="6" stroke="rgba(255,255,255,0.14)" stroke-width="1.8" />
                        <circle cx="8" cy="8" r="6" stroke="#EDEDEF" stroke-width="1.8" stroke-linecap="round" stroke-dasharray="37.7" stroke-dashoffset="26" />
                      </svg>
                    </div>
                    <div class="pointer-events-none opacity-0 group-hover/ctx:opacity-100 transition-opacity duration-150 absolute bottom-full mb-2 right-0 px-2.5 py-1 rounded-[6px] bg-[#18181B] border border-white/[0.08] font-mono text-[10.5px] text-[#D4D4D8] whitespace-nowrap z-50">
                      Context: 28k / 200k tokens (14%)
                    </div>
                  </div>

                  <div class="relative">
                    <button
                      type="button"
                      data-omni-model-badge=""
                      onclick={() => { mockPlusOpen = false; mockModelOpen = !mockModelOpen; }}
                      class="px-2.5 py-1 rounded-[4px] flex items-center gap-1.5 text-[11.5px] font-mono text-[#A3A3AA] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors"
                    >
                      <span>{settingsStore.data.activeModel || 'gemini-3.8-flash-high'}</span>
                      <span data-omni-reasoning-badge="" class="text-[#73737A]">HIGH</span>
                      <Icon name="chevron-down" class="w-3 h-3 text-[#66666D]" strokeWidth={1.8} />
                    </button>
                    {#if mockModelOpen}
                      <div class="absolute bottom-full mb-2 right-0 w-60 bg-[#18181B] border border-white/[0.08] rounded-[6px] p-2 z-50 text-xs space-y-1">
                        <div class="px-2 py-1 text-[#E5E5E8] font-mono text-[11px] rounded-[4px] bg-white/[0.06]">gemini-3.8-flash-high · HIGH</div>
                        <div class="px-2 py-1 text-[#A3A3AA] font-mono text-[11px] rounded-[4px] hover:bg-white/[0.04]">claude-3-7-sonnet · MED</div>
                        <div class="px-2 py-1 text-[#A3A3AA] font-mono text-[11px] rounded-[4px] hover:bg-white/[0.04]">gpt-4o · LOW</div>
                      </div>
                    {/if}
                  </div>

                  <button
                    type="button"
                    data-omni-button="send"
                    onclick={sendMock}
                    class="w-7 h-7 rounded-[4px] flex items-center justify-center transition-colors {mockInput.trim() ? 'bg-[#EDEDEF] hover:bg-white text-[#111113] border-b-2 border-[#A8A8B0]' : 'bg-[#28282C] text-[#73737A] border-b-2 border-[#1B1B1E]'}"
                    title={t('send', 'Send')}
                  >
                    <Icon name="enter" class="w-3.5 h-3.5" strokeWidth={2.1} />
                  </button>
                </div>
              </div>
            </div>
          </div>
        </div>
      </main>
    </div>
  </section>
</div>
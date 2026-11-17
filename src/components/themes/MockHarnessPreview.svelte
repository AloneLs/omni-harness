<script lang="ts">
import Icon from '../Icon.svelte';
import { themeStore, compileThemeCss, type OmniTheme } from '../../lib/stores/theme.svelte';

let {
  theme,
  fullscreen = false
}: {
  theme: OmniTheme | null;
  fullscreen?: boolean;
} = $props();

const MOCK_CHATS = [
  { id: 'm1', title: 'Refactor shader pipeline', time: '2m', active: true },
  { id: 'm2', title: 'Optimize Netty packet codec', time: '18m', active: false },
  { id: 'm3', title: 'UI theme customization', time: '1h', active: false }
];

let selectedMockChat = $state('m1');
let mockInput = $state('');
let mockMessages = $state([
  {
    id: 'u1',
    role: 'user',
    text: 'Check the render loop and show me the diff for frame pacing.'
  },
  {
    id: 'a1',
    role: 'assistant',
    duration: '14s',
    thought: 'Inspecting frame_timer.rs and comparing swapchain present interval.',
    tools: [
      { name: 'read_file', arg: 'src/render/frame_timer.rs', ok: true, ms: '12ms' },
      { name: 'edit_file', arg: 'src/render/frame_timer.rs', ok: true, ms: '19ms' }
    ],
    text: 'Updated the swapchain pacing threshold and removed the redundant spin-wait. Frame time variance dropped from 3.4ms to 0.3ms.'
  }
]);

let scopeAttr = $derived(fullscreen ? 'data-omni-fullscreen-preview-root' : 'data-omni-preview-root');
let scopedCss = $derived(compileThemeCss(theme, `[${scopeAttr}]`));

function t(key: string, fallback: string): string {
  const val = theme?.textOverrides?.[key];
  return val && val.trim() ? val : fallback;
}

function sendMock() {
  const txt = mockInput.trim();
  if (!txt) return;
  mockInput = '';
  mockMessages = [
    ...mockMessages,
    { id: `u-${Date.now()}`, role: 'user', text: txt },
    {
      id: `a-${Date.now() + 1}`,
      role: 'assistant',
      duration: '2s',
      thought: 'Preview mode: simulated response without backend calls.',
      tools: [],
      text: 'Preview response rendered with current theme styles.'
    }
  ];
}
</script>

{#if scopedCss}
  {@html `<style>${scopedCss}</style>`}
{/if}

<div
  data-omni-preview-root={!fullscreen ? '' : undefined}
  data-omni-fullscreen-preview-root={fullscreen ? '' : undefined}
  data-omni-app=""
  class="w-full h-full flex flex-col bg-[#101012] text-[#E5E5E8] overflow-hidden select-none"
>
  <header data-omni-titlebar="" class="h-10 shrink-0 border-b border-white/[0.06] flex items-center justify-between px-3 bg-[#101012]">
    <div class="flex items-center gap-2">
      <button type="button" class="p-1.5 rounded hover:bg-white/[0.06] text-[#A3A3AA]">
        <Icon name="sidebar" class="w-4 h-4" />
      </button>
      <span class="font-mono text-xs text-[#D4D4D8] tracking-tight">OMNIHARNESS // PREVIEW</span>
    </div>
    <div class="flex items-center gap-1.5">
      <button type="button" data-omni-button="settings" class="p-1.5 rounded hover:bg-white/[0.06] text-[#A3A3AA]">
        <Icon name="settings" class="w-4 h-4" />
      </button>
    </div>
  </header>

  <div data-omni-main="" class="flex-1 flex min-h-0 overflow-hidden">
    <aside data-omni-sidebar="" class="w-56 shrink-0 bg-[#171719] border-r border-white/[0.06] flex flex-col p-2.5 gap-2">
      <div data-omni-sidebar-header="" class="flex items-center justify-between px-1 py-1">
        <span class="text-xs font-medium text-[#D4D4D8]">{t('task_list', 'Task list')}</span>
        <button
          type="button"
          data-omni-button="new-chat"
          class="flex items-center gap-1 px-2 py-1 rounded-[5px] bg-white/[0.06] hover:bg-white/[0.1] text-[11px] text-[#EDEDEF]"
        >
          <Icon name="plus" class="w-3 h-3" />
          <span>{t('new_task', 'New task')}</span>
        </button>
      </div>

      <div class="px-2 py-1.5 rounded-[5px] bg-[#121214] border border-white/[0.06] flex items-center gap-1.5 text-[11px] text-[#73737A]">
        <Icon name="search" class="w-3.5 h-3.5" />
        <span>{t('search_chats', 'Search chats...')}</span>
      </div>

      <div data-omni-chat-list="" class="flex-1 overflow-y-auto space-y-1">
        {#each MOCK_CHATS as mc}
          <button
            type="button"
            data-omni-chat-item=""
            onclick={() => (selectedMockChat = mc.id)}
            class="w-full text-left px-2.5 py-2 rounded-[6px] text-xs flex items-center justify-between transition-colors {selectedMockChat === mc.id ? 'bg-white/[0.08] text-[#EDEDEF]' : 'text-[#A3A3AA] hover:bg-white/[0.04]'}"
          >
            <span class="truncate">{mc.title}</span>
            <span class="text-[10px] text-[#66666D] ml-2">{mc.time}</span>
          </button>
        {/each}
      </div>
    </aside>

    <main data-omni-chat-view="" class="flex-1 flex flex-col min-w-0 bg-[#101012]">
      <div data-omni-messages="" class="flex-1 overflow-y-auto px-6 py-5 space-y-5">
        {#each mockMessages as m (m.id)}
          {#if m.role === 'user'}
            <div data-omni-message="user" class="flex justify-end">
              <div data-omni-message-bubble="" class="max-w-[75%] px-3.5 py-2.5 rounded-[10px] bg-[#1E1E24] border border-white/[0.08] text-[13px] text-[#EDEDEF]">
                {m.text}
              </div>
            </div>
          {:else}
            <div data-omni-message="assistant" class="space-y-2.5 max-w-[85%]">
              <div data-omni-timeline="" class="rounded-[8px] bg-[#141418] border border-white/[0.06] p-3 space-y-2">
                <div data-omni-timeline-header="" class="flex items-center justify-between text-xs text-[#A3A3AA]">
                  <span class="font-medium text-[#D4D4D8]">{t('worked_for', 'Worked for')} {m.duration}</span>
                  <Icon name="chevron-down" class="w-3.5 h-3.5" />
                </div>
                {#if m.thought}
                  <div class="text-[11px] text-[#73737A] italic">{m.thought}</div>
                {/if}
                {#each m.tools as tl}
                  <div data-omni-tool-row="" class="flex items-center justify-between text-[11px] font-mono px-2 py-1 rounded bg-white/[0.03] text-[#A3A3AA]">
                    <span class="flex items-center gap-1.5">
                      <Icon name="check" class="w-3 h-3 text-[#4ADE80]" />
                      <span>{tl.name} · {tl.arg}</span>
                    </span>
                    <span class="text-[#66666D]">{tl.ms}</span>
                  </div>
                {/each}
              </div>
              <div data-omni-message-bubble="" class="text-[13.5px] leading-relaxed text-[#E5E5E8]">
                {m.text}
              </div>
              <div class="flex items-center gap-1.5 text-[#66666D]">
                <button type="button" class="p-1 rounded hover:bg-white/[0.05] hover:text-[#D4D4D8]">
                  <Icon name="copy" class="w-3.5 h-3.5" />
                </button>
              </div>
            </div>
          {/if}
        {/each}
      </div>

      <div data-omni-composer="" class="px-6 pb-5 pt-2 shrink-0">
        <div data-omni-composer-box="" class="rounded-[12px] bg-[#17171B] border border-white/[0.08] p-3 space-y-2">
          <input
            type="text"
            data-omni-composer-input=""
            bind:value={mockInput}
            onkeydown={(e) => {
              if (e.key === 'Enter') sendMock();
            }}
            placeholder={t('ask_placeholder', 'Ask OmniHarness anything, @ to add context, / for commands')}
            class="w-full bg-transparent text-[13px] text-[#E5E5E8] placeholder-[#66666D] outline-none"
          />
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-1.5 text-[#8E8E95]">
              <button type="button" class="p-1 rounded hover:bg-white/[0.06]"><Icon name="plus" class="w-4 h-4" /></button>
              <button type="button" class="p-1 rounded hover:bg-white/[0.06]"><Icon name="terminal" class="w-4 h-4" /></button>
            </div>
            <button
              type="button"
              data-omni-button="send"
              onclick={sendMock}
              class="px-2.5 py-1 rounded-[6px] bg-[#EDEDEF] text-[#111113] text-xs font-medium flex items-center gap-1"
            >
              <Icon name="enter" class="w-3.5 h-3.5" />
              <span>{t('send', 'Send')}</span>
            </button>
          </div>
        </div>
      </div>
    </main>
  </div>
</div>

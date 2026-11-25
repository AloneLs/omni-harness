<script lang="ts">
import { fly, slide } from 'svelte/transition';
import { invoke } from '@tauri-apps/api/core';
import Icon from './Icon.svelte';
import { chatStore, type ChatMessage } from '../lib/stores/chat.svelte';
import { renderMarkdown, extractVizBlocks } from '../lib/markdown';
import TimelineBlock from './TimelineBlock.svelte';
import InteractiveVizCard from './InteractiveVizCard.svelte';
import { attachPressScale } from '../lib/motion';
import { isInternalArtifactTool } from '../lib/toolMapper';

let {
  message,
  isLatestAssistant = true,
  isLastUserUnanswered = false,
  onEditUser = () => {},
  onRegenerate = () => {},
  onContinue = () => {}
}: {
  message: ChatMessage;
  isLatestAssistant?: boolean;
  isLastUserUnanswered?: boolean;
  onEditUser?: (messageId: string, newText: string, rollbackFiles: boolean) => void;
  onRegenerate?: (messageId: string) => void;
  onContinue?: (messageId: string) => void;
} = $props();

let isEditing = $state<boolean>(false);
let editText = $state<string>('');
let rollbackFilesChecked = $state<boolean>(true);
let copied = $state<boolean>(false);
let errorCopied = $state<boolean>(false);
let isUserExpanded = $state<boolean>(false);
let confirmRetry = $state<boolean>(false);
let diffSummaryExpanded = $state<boolean>(false);
let confirmRetryTimer: ReturnType<typeof setTimeout> | null = null;

let hasHeavyHistory = $derived(
  (message.toolCalls && message.toolCalls.filter((tc) => !isInternalArtifactTool(tc.toolName, tc.rawArgs || tc.args)).length > 0) ||
  (message.steps && message.steps.filter((s) => (s.kind === 'tool' && !isInternalArtifactTool(s.record.toolName, s.record.rawArgs || s.record.args)) || s.kind === 'thought').length > 1) ||
  Boolean(message.reasoningContent && message.reasoningContent.length > 200)
);

function handleRetryClick() {
  if (!hasHeavyHistory || confirmRetry) {
    confirmRetry = false;
    if (confirmRetryTimer) {
      clearTimeout(confirmRetryTimer);
      confirmRetryTimer = null;
    }
    onRegenerate(message.id);
    return;
  }
  confirmRetry = true;
  if (confirmRetryTimer) clearTimeout(confirmRetryTimer);
  confirmRetryTimer = setTimeout(() => {
    confirmRetry = false;
    confirmRetryTimer = null;
  }, 4000);
}
let userBoxEl = $state<HTMLDivElement | null>(null);
let userFullHeight = $state<number>(600);

let isLongUserMessage = $derived(
  message.role === 'user' && (message.content.length > 320 || message.content.split('\n').length > 6)
);

let splitPipeline = $derived.by(() => {
  if (message.role !== 'assistant') {
    return { timelineSteps: [], finalContent: '' };
  }
  const rawSteps = message.steps || [];
  const allSteps = rawSteps.filter((s) => s.kind !== 'tool' || !isInternalArtifactTool(s.record?.toolName || '', s.record?.rawArgs || s.record?.args));
  if (allSteps.length === 0) {
    return { timelineSteps: [], finalContent: message.content };
  }
  const last = allSteps[allSteps.length - 1];
  const hasWork = allSteps.some((s) => s.kind === 'thought' || s.kind === 'tool');
  if (!hasWork) {
    return { timelineSteps: [], finalContent: message.content };
  }
  if (!message.streaming && last && last.kind === 'message') {
    return { timelineSteps: allSteps.slice(0, -1), finalContent: last.text };
  }
  return { timelineSteps: allSteps, finalContent: '' };
});

let turnDiffSummary = $derived.by(() => {
  if (message.role !== 'assistant' || message.streaming) {
    return null;
  }
  const calls = message.toolCalls || [];
  const byPath = new Map<string, { path: string; fileName: string; dirPath: string; added: number; removed: number }>();
  const snapshotIds: string[] = [];

  for (const tc of calls) {
    if (tc.status !== 'success') continue;
    if (tc.toolName !== 'edit_file' && tc.toolName !== 'write_file' && tc.toolName !== 'patch_file') continue;
    if (!tc.output) continue;
    try {
      const parsed = JSON.parse(tc.output);
      const rawPath = parsed.path || parsed.file_path || '';
      const sid = parsed.snapshotId || '';
      const added = Number(parsed.addedLines || 0);
      const removed = Number(parsed.removedLines || 0);
      if (sid && !snapshotIds.includes(sid)) snapshotIds.push(sid);
      if (!rawPath) continue;
      const norm = String(rawPath).replace(/\\/g, '/');
      const idx = norm.lastIndexOf('/');
      const fileName = idx === -1 ? norm : norm.slice(idx + 1);
      const dirPath = idx === -1 ? '' : norm.slice(0, idx);
      const prev = byPath.get(norm);
      if (prev) {
        prev.added += added;
        prev.removed += removed;
      } else {
        byPath.set(norm, { path: String(rawPath), fileName, dirPath, added, removed });
      }
    } catch {}
  }

  const files = Array.from(byPath.values());
  if (files.length === 0 || snapshotIds.length === 0) return null;
  let totalAdded = 0;
  let totalRemoved = 0;
  for (const f of files) {
    totalAdded += f.added;
    totalRemoved += f.removed;
  }
  return { files, snapshotIds, totalAdded, totalRemoved };
});

let parsedMarkdown = $derived.by(() => {
  if (message.role !== 'assistant' || !splitPipeline.finalContent) {
    return { html: '', vizBlocks: [] };
  }
  const { cleanContent, vizBlocks } = extractVizBlocks(splitPipeline.finalContent);
  const html = renderMarkdown(cleanContent);
  return { html, vizBlocks };
});

const EDIT_SLASH_COMMANDS = [
  { cmd: '/goal', desc: 'Autonomous completion loop (/goal <cond>, status, clear)' },
  { cmd: '/plan', desc: 'Generate engineering execution plan' },
  { cmd: '/compact', desc: 'Compact message context history' },
  { cmd: '/clear', desc: 'Clear messages in active session' },
  { cmd: '/rollback', desc: 'Rollback modified files to last snapshot' },
  { cmd: '/model', desc: 'Switch primary language model' },
  { cmd: '/reasoning', desc: 'Adjust deep reasoning level' },
  { cmd: '/export', desc: 'Export session as Markdown' }
];
let editSlashOpen = $state(false);
let editSlashFilter = $state('');
let editSlashIdx = $state(0);

let filteredEditSlash = $derived.by(() => {
  const q = editSlashFilter.toLowerCase();
  return EDIT_SLASH_COMMANDS.filter((c) => c.cmd.toLowerCase().includes(q) || c.desc.toLowerCase().includes(q));
});

function handleEditInput() {
  const m = editText.match(/^(\/[^\s]*)$/);
  if (m) {
    editSlashOpen = true;
    editSlashFilter = m[1];
    editSlashIdx = 0;
  } else {
    editSlashOpen = false;
  }
}

function applyEditSlash(cmd: string) {
  editText = `${cmd} `;
  editSlashOpen = false;
}

function handleEditKeydown(e: KeyboardEvent) {
  if (editSlashOpen && filteredEditSlash.length > 0) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      editSlashIdx = (editSlashIdx + 1) % filteredEditSlash.length;
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault();
      editSlashIdx = (editSlashIdx - 1 + filteredEditSlash.length) % filteredEditSlash.length;
      return;
    }
    if (e.key === 'Tab' || e.key === 'Enter') {
      e.preventDefault();
      applyEditSlash(filteredEditSlash[editSlashIdx].cmd);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault();
      editSlashOpen = false;
      return;
    }
  }
  if (e.key === 'Enter' && !e.shiftKey && !editSlashOpen) {
    e.preventDefault();
    saveEditing();
  }
}

function startEditing() {
  editText = message.content;
  editSlashOpen = false;
  isEditing = true;
}

function cancelEditing() {
  isEditing = false;
}

function saveEditing() {
  if (editText.trim()) {
    onEditUser(message.id, editText.trim(), rollbackFilesChecked);
    isEditing = false;
  }
}


function formatFriendlyError(raw: string): string {
  const statusMatch = raw.match(/HTTP\s*(?:error\s*)?(\d{3})/i);
  const statusPrefix = statusMatch ? `HTTP ${statusMatch[1]}: ` : '';
  const braceIdx = raw.indexOf('{');
  if (braceIdx !== -1) {
    try {
      const parsed = JSON.parse(raw.slice(braceIdx));
      const msg = parsed?.error?.message || parsed?.message || parsed?.detail;
      if (typeof msg === 'string' && msg.trim()) {
        return `${statusPrefix}${msg.trim()}`;
      }
    } catch {}
    return raw.slice(0, braceIdx).replace(/[:\s]+$/, '') || 'Request failed';
  }
  return raw;
}

async function copyRawError(raw: string) {
  try {
    await navigator.clipboard.writeText(raw);
    errorCopied = true;
    setTimeout(() => {
      errorCopied = false;
    }, 1500);
  } catch {}
}

async function copyMessageText() {
  try {
    await navigator.clipboard.writeText(message.content);
    copied = true;
    setTimeout(() => {
      copied = false;
    }, 1500);
  } catch {}
}

function formatAudioTime(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return '0:00';
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${s.toString().padStart(2, '0')}`;
}

function handleContainerClick(event: MouseEvent) {
  const target = event.target as HTMLElement;
  const voicePlayBtn = target.closest('.omni-voice-play') as HTMLButtonElement | null;
  if (voicePlayBtn) {
    const box = voicePlayBtn.closest('.omni-voice-msg') as HTMLElement | null;
    const audio = box?.querySelector('.omni-voice-audio') as HTMLAudioElement | null;
    if (box && audio) {
      const playIcon = voicePlayBtn.querySelector('.omni-icon-play');
      const pauseIcon = voicePlayBtn.querySelector('.omni-icon-pause');
      const timeEl = box.querySelector('.omni-voice-time');
      const bars = Array.from(box.querySelectorAll('.omni-wave-bar')) as HTMLElement[];
      if (!audio.dataset.bound) {
        audio.dataset.bound = '1';
        audio.addEventListener('timeupdate', () => {
          const pct = audio.duration ? audio.currentTime / audio.duration : 0;
          const activeCount = Math.round(pct * bars.length);
          bars.forEach((b, i) => {
            b.classList.toggle('bg-[#EDEDEF]', i < activeCount);
            b.classList.toggle('bg-white/25', i >= activeCount);
          });
          if (timeEl) timeEl.textContent = formatAudioTime(audio.currentTime);
        });
        audio.addEventListener('ended', () => {
          playIcon?.classList.remove('hidden');
          pauseIcon?.classList.add('hidden');
          bars.forEach((b) => {
            b.classList.remove('bg-[#EDEDEF]');
            b.classList.add('bg-white/25');
          });
          if (timeEl) timeEl.textContent = formatAudioTime(audio.duration || 0);
        });
      }
      if (audio.paused) {
        audio.play().catch(() => {});
        playIcon?.classList.add('hidden');
        pauseIcon?.classList.remove('hidden');
      } else {
        audio.pause();
        playIcon?.classList.remove('hidden');
        pauseIcon?.classList.add('hidden');
      }
    }
    return;
  }
  const fileLink = target.closest('.omni-file-link') as HTMLAnchorElement | null;
  if (fileLink) {
    event.preventDefault();
    const openPath = fileLink.getAttribute('data-open-path');
    if (openPath) {
      invoke('open_path_in_explorer', { path: openPath }).catch(() => {});
    }
    return;
  }
  const copyBtn = target.closest('.copy-code-btn') as HTMLButtonElement | null;
  if (copyBtn) {
    const targetId = copyBtn.getAttribute('data-clipboard-target');
    if (targetId) {
      const textarea = document.getElementById(targetId) as HTMLTextAreaElement | null;
      if (textarea) {
        navigator.clipboard.writeText(textarea.value);
        const label = copyBtn.querySelector('.copy-label');
        if (label) {
          label.textContent = 'Copied!';
          setTimeout(() => {
            label.textContent = 'Copy';
          }, 1500);
        }
      }
    }
  }
}
</script>

<div
  in:fly={{ y: 10, duration: 200 }}
  class="w-full py-2.5 group"
  onclick={handleContainerClick}
  role="presentation"
>
  {#if message.role === 'system'}
    <div class="w-full flex items-center gap-3 py-2 select-none">
      <div class="flex-1 h-px bg-white/[0.16]"></div>
      <span class="font-mono text-[10.5px] text-[#8E8E95] shrink-0">model changed · <span class="text-[#D4D4D8]">{message.content}</span></span>
      <div class="flex-1 h-px bg-white/[0.16]"></div>
    </div>
  {:else if message.role === 'user'}
    <div data-omni-message="user" class="flex flex-col items-end">
      {#if isEditing}
        <div class="w-full max-w-[85%] space-y-2.5 p-3.5 rounded-[8px] border border-white/[0.1] bg-[#1C1C1F] relative">
          {#if editSlashOpen && filteredEditSlash.length > 0}
            <div class="mb-2 rounded-[6px] bg-[#141417] border border-white/[0.08] p-1 max-h-44 overflow-y-auto divide-y divide-white/[0.04]">
              {#each filteredEditSlash as item, idx}
                <button
                  type="button"
                  onclick={() => applyEditSlash(item.cmd)}
                  class="w-full px-2.5 py-1.5 text-left rounded-[4px] flex items-center justify-between gap-2 text-xs transition-colors {idx === editSlashIdx ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#A3A3AA] hover:bg-white/[0.04]'}"
                >
                  <span class="font-mono text-[#E5E5E8]">{item.cmd}</span>
                  <span class="text-[11px] text-[#66666D] truncate">{item.desc}</span>
                </button>
              {/each}
            </div>
          {/if}
          <textarea
            bind:value={editText}
            oninput={handleEditInput}
            onkeydown={handleEditKeydown}
            rows="3"
            class="w-full bg-[#121214] border border-white/[0.08] rounded-[6px] p-2.5 text-[13px] text-[#EDEDEF] outline-none resize-none"
          ></textarea>
          <div class="flex items-center justify-between gap-3 pt-1 select-none">
            <button
              type="button"
              class="flex items-center gap-2 cursor-pointer text-xs text-[#A3A3AA] hover:text-[#EDEDEF]"
              onclick={() => (rollbackFilesChecked = !rollbackFilesChecked)}
            >
              <span class="w-4 h-4 rounded-[4px] border flex items-center justify-center transition-colors {rollbackFilesChecked ? 'bg-[#1E1E22] border-white/[0.3] text-[#EDEDEF]' : 'bg-[#121214] border-white/[0.12] text-transparent'}">
                <Icon name="check" class="w-3 h-3" strokeWidth={2.2} />
              </span>
              <span>Rollback file modifications to snapshot</span>
            </button>

            <div class="flex items-center gap-2">
              <button
                type="button"
                class="h-7 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#A3A3AA] hover:text-[#EDEDEF] active:scale-[0.97] transition-all"
                onclick={cancelEditing}
              >
                Cancel
              </button>
              <button
                type="button"
                class="h-7 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-xs text-[#111113] font-medium active:scale-[0.97] transition-all"
                onclick={saveEditing}
              >
                Save & Regenerate
              </button>
            </div>
          </div>
        </div>
      {:else}
        {#if message.attachments && message.attachments.length > 0}
          <div class="mb-1.5 flex flex-wrap justify-end gap-1.5 max-w-[78%]">
            {#each message.attachments as att (att.id)}
              <div class="flex items-center gap-2 pl-1.5 pr-2.5 py-1 rounded-[8px] bg-[#18181B] border border-white/[0.08] text-xs max-w-[220px]">
                {#if att.kind === 'image' && att.previewUrl}
                  <img src={att.previewUrl} alt={att.name} class="w-7 h-7 rounded-[5px] object-cover shrink-0 bg-[#121214]" />
                {:else if att.kind === 'video' && att.previewUrl}
                  <video src={att.previewUrl} class="w-7 h-7 rounded-[5px] object-cover shrink-0 bg-[#121214]" muted></video>
                {:else}
                  <div class="w-7 h-7 rounded-[5px] bg-[#121214] border border-white/[0.06] flex items-center justify-center shrink-0 text-[#8E8E95]">
                    <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6">
                      <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
                      <polyline points="14 2 14 8 20 8"></polyline>
                    </svg>
                  </div>
                {/if}
                <span class="font-mono text-[11px] text-[#D4D4D8] truncate">{att.name}</span>
              </div>
            {/each}
          </div>
        {/if}

        {#if message.content}
          <div
            data-omni-message-bubble=""
            bind:this={userBoxEl}
            style={isLongUserMessage ? `max-height: ${isUserExpanded ? (userFullHeight ? `${userFullHeight}px` : 'none') : '132px'};` : undefined}
            class="max-w-[78%] rounded-[12px] bg-[#1E1E22] border border-white/[0.07] px-3.5 pt-2 font-sans text-[13.5px] text-[#EDEDEF] leading-relaxed select-text relative transition-[max-height] duration-200 ease-out overflow-hidden {isLongUserMessage && !isUserExpanded ? 'pb-5' : 'pb-2'}">
            <span class="whitespace-pre-wrap">{message.content}</span>
            {#if isLongUserMessage && !isUserExpanded}
              <div class="absolute inset-x-0 bottom-0 h-9 bg-gradient-to-t from-[#1E1E22] via-[#1E1E22]/85 to-transparent pointer-events-none"></div>
            {/if}
            {#if isLongUserMessage}
              <button
                type="button"
                class="flex items-center justify-center text-[#8E8E95] hover:text-[#EDEDEF] transition-colors {isUserExpanded ? 'w-full pt-1 -mb-0.5' : 'w-full absolute bottom-0.5 inset-x-0 z-10'}"
                onclick={() => { if (userBoxEl) userFullHeight = userBoxEl.scrollHeight; isUserExpanded = !isUserExpanded; }}
                title={isUserExpanded ? 'Collapse' : 'Expand'}
              >
                <Icon name="chevron-down" class="w-3.5 h-3.5 transition-transform duration-150 {isUserExpanded ? 'rotate-180' : ''}" strokeWidth={1.8} />
              </button>
            {/if}
          </div>
        {/if}

        {#if isLastUserUnanswered}
          <div class="mt-2 flex items-center justify-end gap-2 select-none">
            <span class="inline-flex items-center gap-1.5 font-sans text-[11.5px] text-[#A3A3AA]">
              <span class="w-1.5 h-1.5 rounded-full bg-[#F59E0B] animate-pulse"></span>
              Задача прервана
            </span>
            <button
              type="button"
              class="h-6 px-2.5 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[11.5px] font-sans font-medium text-[#111113] active:scale-[0.97] transition-all flex items-center gap-1"
              onclick={() => onContinue(message.id)}
              title="Продолжить выполнение задачи"
            >
              <Icon name="enter" class="w-3 h-3 text-[#111113]" strokeWidth={2.2} />
              <span>Продолжить</span>
            </button>
          </div>
        {/if}

        <div class="mt-1 flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity duration-150">
          <button
            type="button"
            class="p-1 rounded-[4px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors"
            onclick={copyMessageText}
            title="Copy"
            use:attachPressScale
          >
            {#if copied}
              <Icon name="check" class="w-3.5 h-3.5 text-[#D4D4D8]" strokeWidth={1.8} />
            {:else}
              <Icon name="copy" class="w-3.5 h-3.5" strokeWidth={1.6} />
            {/if}
          </button>
          <button
            type="button"
            class="p-1 rounded-[4px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors"
            onclick={startEditing}
            title="Edit"
            use:attachPressScale
          >
            <Icon name="edit" class="w-3.5 h-3.5" strokeWidth={1.6} />
          </button>
        </div>
      {/if}
    </div>
  {:else}
    <div data-omni-message="assistant" class="w-full space-y-2">
      {#if message.imageGenerating}
        <div class="my-1 w-[260px] h-[260px] rounded-[14px] border border-white/[0.1] bg-[#141418] overflow-hidden relative flex flex-col items-center justify-center gap-2.5 select-none">
          <div class="absolute inset-0 bg-gradient-to-tr from-[#8B5CF6]/15 via-[#38BDF8]/10 to-[#EC4899]/15 animate-pulse"></div>
          <div class="relative z-10 w-10 h-10 rounded-full border-2 border-white/[0.14] border-t-[#E5E5E8] animate-spin"></div>
          <span class="relative z-10 font-mono text-[11px] text-[#A3A3AA]">Generating image...</span>
        </div>
      {:else if message.streaming || message.paused || message.error || message.reasoningContent || (message.toolCalls && message.toolCalls.length > 0) || (message.steps && message.steps.length > 0) || (message.todos && message.todos.length > 0)}
        <TimelineBlock
          steps={splitPipeline.timelineSteps}
          reasoningContent={message.reasoningContent}
          toolCalls={message.toolCalls}
          isStreaming={message.streaming}
          paused={message.paused}
          canResume={isLatestAssistant && !chatStore.isStreaming}
          hasFinalContent={Boolean(splitPipeline.finalContent)}
          statusText={message.statusText}
          retryReason={message.retryReason}
          startedAt={message.startedAt}
          finishedAt={message.finishedAt}
          error={message.error}
          onRetry={() => onRegenerate(message.id)}
          onContinue={() => onContinue(message.id)}
        />
      {/if}

      {#if splitPipeline.finalContent}
        <div data-omni-message-prose="" class="markdown-prose font-sans text-[15px] text-[#EDEDEF] leading-relaxed select-text">
          {@html parsedMarkdown.html}
        </div>
      {/if}

      {#if parsedMarkdown.vizBlocks && parsedMarkdown.vizBlocks.length > 0}
        <div class="space-y-4 my-3">
          {#each parsedMarkdown.vizBlocks as viz (viz.id)}
            <InteractiveVizCard
              id={viz.id}
              title={viz.title}
              code={viz.code}
              type={viz.type}
            />
          {/each}
        </div>
      {/if}



      {#if turnDiffSummary}
        <div class="mt-2.5 rounded-[8px] bg-[#121215] border border-white/[0.08] overflow-hidden">
          <div class="px-3 py-2 flex items-center justify-between gap-3">
            <button
              type="button"
              onclick={() => (diffSummaryExpanded = !diffSummaryExpanded)}
              class="flex items-center gap-2 text-xs text-[#D4D4D8] hover:text-white transition-colors min-w-0"
            >
              <span class="font-medium">{turnDiffSummary.files.length} {turnDiffSummary.files.length === 1 ? 'file' : 'files'} changed</span>
              <span class="font-mono text-[11px] text-[#4ADE80]">+{turnDiffSummary.totalAdded}</span>
              <span class="font-mono text-[11px] text-[#F87171]">-{turnDiffSummary.totalRemoved}</span>
              <Icon
                name="chevron-down"
                class="w-3.5 h-3.5 text-[#73737A] transition-transform duration-150 {diffSummaryExpanded ? 'rotate-180' : '-rotate-90'}"
                strokeWidth={1.6}
              />
            </button>

            <button
              type="button"
              onclick={() =>
                chatStore.openDiffReview(
                  message.id,
                  turnDiffSummary.files.map((f) => f.fileName).slice(0, 3).join(', '),
                  turnDiffSummary.snapshotIds
                )
              }
              class="h-6 px-2.5 rounded-[5px] bg-[#1C1C21] hover:bg-[#27272D] border border-white/[0.08] text-[11px] font-medium text-[#E5E5E8] transition-colors shrink-0"
            >
              Review
            </button>
          </div>

          {#if diffSummaryExpanded}
            <div transition:slide={{ duration: 150 }} class="border-t border-white/[0.06] divide-y divide-white/[0.04]">
              {#each turnDiffSummary.files as f (f.path)}
                <button
                  type="button"
                  onclick={() =>
                    chatStore.openDiffReview(
                      message.id,
                      f.fileName,
                      turnDiffSummary.snapshotIds,
                      f.path
                    )
                  }
                  class="w-full px-3 py-1.5 flex items-center justify-between gap-3 hover:bg-white/[0.03] text-left transition-colors"
                >
                  <div class="flex items-baseline gap-2 min-w-0">
                    <span class="font-mono text-xs text-[#EDEDEF] truncate">{f.fileName}</span>
                    {#if f.dirPath}
                      <span class="font-mono text-[11px] text-[#66666D] truncate">{f.dirPath}</span>
                    {/if}
                  </div>
                  <div class="flex items-center gap-1.5 shrink-0 font-mono text-[11px]">
                    <span class="text-[#4ADE80]">+{f.added}</span>
                    <span class="text-[#F87171]">-{f.removed}</span>
                  </div>
                </button>
              {/each}
            </div>
          {/if}
        </div>
      {/if}

      {#if !message.streaming && !message.error}
        <div class="space-y-1 pt-0.5">
          <div class="flex items-center gap-1 {confirmRetry ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'} transition-opacity duration-150">
            <button
              type="button"
              class="p-1 rounded-[4px] text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors"
              onclick={copyMessageText}
              title="Copy"
              use:attachPressScale
            >
              {#if copied}
                <Icon name="check" class="w-3.5 h-3.5 text-[#D4D4D8]" strokeWidth={1.8} />
              {:else}
                <Icon name="copy" class="w-3.5 h-3.5" strokeWidth={1.6} />
              {/if}
            </button>
            <button
              type="button"
              class="p-1 rounded-[4px] transition-colors {confirmRetry ? 'text-[#F59E0B] bg-[#F59E0B]/10' : 'text-[#66666D] hover:text-[#E5E5E8] hover:bg-white/[0.05]'}"
              onclick={handleRetryClick}
              title={confirmRetry ? 'Click again to confirm retry' : 'Retry'}
              use:attachPressScale
            >
              <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6">
                <polyline points="23 4 23 10 17 10"></polyline>
                <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"></path>
              </svg>
            </button>
            {#if message.modelId}
              <span class="ml-1.5 font-mono text-[10.5px] text-[#66666D] select-none">{message.modelId}</span>
            {/if}
          </div>
          {#if confirmRetry}
            <div class="text-[11px] text-[#A3A3AA] select-none">
              Нажмите ещё раз — повтор сотрёт историю работы этого шага
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>

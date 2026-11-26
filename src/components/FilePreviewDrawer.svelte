<script lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { fly } from 'svelte/transition';
import { cubicOut } from 'svelte/easing';
import { chatStore } from '../lib/stores/chat.svelte';
import { workspaceStore } from '../lib/stores/workspace.svelte';
import { highlightCode } from '../lib/markdown';
import FileToolIcon from './FileToolIcon.svelte';
import Icon from './Icon.svelte';

let fileContent = $state<string>('');
let loading = $state<boolean>(false);
let errorMsg = $state<string | null>(null);
let isFallbackContent = $state<boolean>(false);

let preview = $derived(chatStore.filePreview);
let fileName = $derived(preview ? (preview.path.split(/[\\/]/).pop() || preview.path) : '');
let ext = $derived(fileName.split('.').pop()?.toLowerCase() || 'text');

let addedSet = $derived.by(() => {
  const s = new Set<string>();
  for (const block of preview?.addedTexts || []) {
    for (const line of block.split('\n')) {
      const t = line.trim();
      if (t) s.add(t);
    }
  }
  return s;
});

let removedLines = $derived.by(() => {
  const out: string[] = [];
  for (const block of preview?.removedTexts || []) {
    for (const line of block.split('\n')) {
      out.push(line);
    }
  }
  return out;
});

type DiffDisplayRow =
  | { kind: 'del'; key: string; raw: string; html: string }
  | { kind: 'line'; key: string; num: number; raw: string; html: string; isAdded: boolean };

let renderedRows = $derived.by<DiffDisplayRow[]>(() => {
  if (!fileContent) return [];
  const rawLines = fileContent.replace(/\r\n/g, '\n').split('\n');
  const addedBlocks = preview?.addedTexts || [];
  const removedBlocks = preview?.removedTexts || [];
  const delBeforeLine = new Map<number, string[]>();
  const orphanRemoved: string[] = [];

  for (let b = 0; b < removedBlocks.length; b++) {
    const remLines = (removedBlocks[b] || '').split('\n');
    const addBlock = (addedBlocks[b] || '').trim();
    const firstAddLine = addBlock.split('\n').map((l) => l.trim()).find((l) => l.length > 0) || '';
    let anchorIdx = -1;
    if (firstAddLine) {
      anchorIdx = rawLines.findIndex((rl) => rl.trim() === firstAddLine);
    }
    if (anchorIdx >= 0) {
      const existing = delBeforeLine.get(anchorIdx) || [];
      delBeforeLine.set(anchorIdx, [...existing, ...remLines]);
    } else {
      orphanRemoved.push(...remLines);
    }
  }

  const rows: DiffDisplayRow[] = [];
  for (let i = 0; i < rawLines.length; i++) {
    const dels = delBeforeLine.get(i);
    if (dels) {
      dels.forEach((d, di) => {
        rows.push({ kind: 'del', key: `del-${i}-${di}`, raw: d, html: highlightCode(d, ext) });
      });
    }
    const line = rawLines[i];
    rows.push({
      kind: 'line',
      key: `ln-${i + 1}`,
      num: i + 1,
      raw: line,
      html: highlightCode(line, ext),
      isAdded: addedSet.has(line.trim())
    });
  }
  if (orphanRemoved.length > 0) {
    orphanRemoved.forEach((d, di) => {
      rows.push({ kind: 'del', key: `del-end-${di}`, raw: d, html: highlightCode(d, ext) });
    });
  }
  return rows;
});

$effect(() => {
  const req = preview;
  if (!req) {
    fileContent = '';
    errorMsg = null;
    loading = false;
    isFallbackContent = false;
    return;
  }
  let cancelled = false;
  loading = true;
  errorMsg = null;
  isFallbackContent = false;
  const activeSession = chatStore.sessions.find((s) => s.id === chatStore.activeChatId);
  const workspaceDir = activeSession?.projectPath || workspaceStore.currentPath || null;
  invoke<string>('read_file_full', {
    path: req.path,
    workspaceDir,
    chatId: chatStore.activeChatId || null
  })
    .then((res) => {
      if (cancelled) return;
      fileContent = res;
      loading = false;
    })
    .catch((err) => {
      if (cancelled) return;
      if (req.fallbackContent) {
        fileContent = req.fallbackContent;
        isFallbackContent = true;
        errorMsg = null;
        loading = false;
        return;
      }
      errorMsg = err instanceof Error ? err.message : String(err);
      fileContent = '';
      loading = false;
    });
  return () => {
    cancelled = true;
  };
});
</script>

{#if preview}
  <aside
    transition:fly={{ x: 24, duration: 180, easing: cubicOut }}
    class="w-[440px] max-w-[44vw] min-w-[260px] h-full shrink-0 bg-[#141417] border-l border-white/[0.08] flex flex-col select-none z-30"
  >
    <div class="h-11 px-3.5 border-b border-white/[0.06] flex items-center justify-between gap-2 shrink-0">
      <div class="flex items-center gap-2 min-w-0">
        <FileToolIcon name={fileName} class="w-4 h-4" />
        <span class="font-mono text-[12px] text-[#EDEDEF] truncate" title={preview.path}>{fileName}</span>
      </div>
      <div class="flex items-center gap-2 shrink-0">
        {#if addedSet.size > 0 || removedLines.length > 0}
          <span class="font-mono text-[11px] flex items-center gap-1.5">
            {#if addedSet.size > 0}<span class="text-[#4ADE80]">+{addedSet.size}</span>{/if}
            {#if removedLines.length > 0}<span class="text-[#F87171]">-{removedLines.length}</span>{/if}
          </span>
        {/if}
        <button
          type="button"
          class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#7A7A82] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
          onclick={() => chatStore.closeFilePreview()}
          title="Close preview"
        >
          <Icon name="close" class="w-3.5 h-3.5" strokeWidth={1.8} />
        </button>
      </div>
    </div>

    <div class="flex-1 overflow-auto font-mono text-[11.5px] leading-5 select-text bg-[#101012]">
      {#if loading}
        <div class="p-4 text-[#73737A] text-xs">Loading file...</div>
      {:else if errorMsg}
        <div class="p-4 text-[#F87171] text-xs whitespace-pre-wrap">{errorMsg}</div>
      {:else}
        {#if isFallbackContent}
          <div class="px-3 py-1.5 border-b border-white/[0.06] bg-[#F59E0B]/10 text-[#FCD34D] text-[11px] font-sans">
            Cached tool output (file is not on disk at this path)
          </div>
        {/if}
        <div class="py-2">
          {#each renderedRows as row (row.key)}
            {#if row.kind === 'del'}
              <div class="flex px-3 bg-[#EF4444]/15 border-l-2 border-[#EF4444] text-[#FCA5A5]">
                <span class="w-9 shrink-0 select-none text-right pr-3 text-[#F87171]/80">-</span>
                <span class="whitespace-pre">{@html row.html || ' '}</span>
              </div>
            {:else}
              <div class="flex px-3 {row.isAdded ? 'bg-[#22C55E]/15 border-l-2 border-[#22C55E]' : 'hover:bg-white/[0.02]'}">
                <span class="w-9 shrink-0 select-none text-right pr-3 text-[#55555C]">{row.num}</span>
                <span class="whitespace-pre text-[#E5E5E8]">{@html row.html || ' '}</span>
              </div>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  </aside>
{/if}

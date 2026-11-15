<script lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { fly } from 'svelte/transition';
import { cubicOut } from 'svelte/easing';
import { chatStore } from '../lib/stores/chat.svelte';
import { highlightCode } from '../lib/markdown';
import FileToolIcon from './FileToolIcon.svelte';
import Icon from './Icon.svelte';

let fileContent = $state<string>('');
let loading = $state<boolean>(false);
let errorMsg = $state<string | null>(null);

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

let renderedLines = $derived.by(() => {
  if (!fileContent) return [];
  const raw = fileContent.replace(/\r\n/g, '\n').split('\n');
  return raw.map((line, i) => ({
    num: i + 1,
    raw: line,
    html: highlightCode(line, ext),
    isAdded: addedSet.has(line.trim())
  }));
});

$effect(() => {
  const req = preview;
  if (!req) {
    fileContent = '';
    errorMsg = null;
    return;
  }
  loading = true;
  errorMsg = null;
  invoke<string>('read_file_full', { path: req.path })
    .then((res) => {
      fileContent = res;
      loading = false;
    })
    .catch((err) => {
      errorMsg = err instanceof Error ? err.message : String(err);
      fileContent = '';
      loading = false;
    });
});
</script>

{#if preview}
  <aside
    transition:fly={{ x: 24, duration: 180, easing: cubicOut }}
    class="w-[460px] max-w-[46vw] h-full shrink-0 bg-[#141417] border-l border-white/[0.08] flex flex-col select-none z-30"
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
        {#if removedLines.length > 0}
          <div class="border-b border-white/[0.06] bg-[#EF4444]/[0.06]">
            {#each removedLines as rl}
              <div class="flex px-3 py-0.5 bg-[#EF4444]/15 border-l-2 border-[#EF4444] text-[#FCA5A5]">
                <span class="w-9 shrink-0 select-none text-[#F87171]/70">-</span>
                <span class="whitespace-pre">{rl}</span>
              </div>
            {/each}
          </div>
        {/if}
        <div class="py-2">
          {#each renderedLines as line (line.num)}
            <div class="flex px-3 {line.isAdded ? 'bg-[#22C55E]/15 border-l-2 border-[#22C55E]' : 'hover:bg-white/[0.02]'}">
              <span class="w-9 shrink-0 select-none text-right pr-3 text-[#55555C]">{line.num}</span>
              <span class="whitespace-pre text-[#E5E5E8]">{@html line.html || ' '}</span>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </aside>
{/if}

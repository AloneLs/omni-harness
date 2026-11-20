<script lang="ts">
import { slide } from 'svelte/transition';
import { chatStore, type TurnFileDiff } from '../lib/stores/chat.svelte';
import Icon from './Icon.svelte';

interface DiffRow {
  kind: 'ctx' | 'add' | 'del' | 'gap';
  oldLine?: number;
  newLine?: number;
  text: string;
}

let collapsedFiles = $state<Record<string, boolean>>({});
let activeCommentTarget = $state<{
  path: string;
  rowKey: string;
  lineNumber: number;
  lineType: 'add' | 'del' | 'ctx';
  lineText: string;
} | null>(null);
let draftComment = $state<string>('');

$effect(() => {
  const focus = chatStore.diffReview?.focusPath;
  if (focus) {
    collapsedFiles = { ...collapsedFiles, [focus]: false };
  }
});

function splitPath(fullPath: string): { fileName: string; dirPath: string } {
  const norm = fullPath.replace(/\\/g, '/');
  const idx = norm.lastIndexOf('/');
  if (idx === -1) return { fileName: norm, dirPath: '' };
  return { fileName: norm.slice(idx + 1), dirPath: norm.slice(0, idx) };
}

function buildDiffRows(file: TurnFileDiff): DiffRow[] {
  const oldLines = file.beforeText ? file.beforeText.replace(/\r\n/g, '\n').split('\n') : [];
  const newLines = file.afterText ? file.afterText.replace(/\r\n/g, '\n').split('\n') : [];

  if (!file.existedBefore && oldLines.length === 0) {
    return newLines.map((text, idx) => ({ kind: 'add', newLine: idx + 1, text }));
  }

  let head = 0;
  while (head < oldLines.length && head < newLines.length && oldLines[head] === newLines[head]) head++;
  let tail = 0;
  while (
    tail < oldLines.length - head &&
    tail < newLines.length - head &&
    oldLines[oldLines.length - 1 - tail] === newLines[newLines.length - 1 - tail]
  )
    tail++;

  const oldMid = oldLines.slice(head, oldLines.length - tail);
  const newMid = newLines.slice(head, newLines.length - tail);
  const rows: DiffRow[] = [];

  for (let k = 0; k < head; k++) {
    rows.push({ kind: 'ctx', oldLine: k + 1, newLine: k + 1, text: oldLines[k] });
  }

  const m = oldMid.length;
  const n = newMid.length;
  if (m === 0) {
    for (let j = 0; j < n; j++) rows.push({ kind: 'add', newLine: head + j + 1, text: newMid[j] });
  } else if (n === 0) {
    for (let i = 0; i < m; i++) rows.push({ kind: 'del', oldLine: head + i + 1, text: oldMid[i] });
  } else if (m * n > 1000000) {
    for (let i = 0; i < m; i++) rows.push({ kind: 'del', oldLine: head + i + 1, text: oldMid[i] });
    for (let j = 0; j < n; j++) rows.push({ kind: 'add', newLine: head + j + 1, text: newMid[j] });
  } else {
    const cols = n + 1;
    const dp = new Int32Array((m + 1) * cols);
    for (let i = m - 1; i >= 0; i--) {
      for (let j = n - 1; j >= 0; j--) {
        dp[i * cols + j] =
          oldMid[i] === newMid[j]
            ? dp[(i + 1) * cols + (j + 1)] + 1
            : Math.max(dp[(i + 1) * cols + j], dp[i * cols + (j + 1)]);
      }
    }

    let i = 0;
    let j = 0;
    while (i < m && j < n) {
      if (oldMid[i] === newMid[j]) {
        rows.push({ kind: 'ctx', oldLine: head + i + 1, newLine: head + j + 1, text: oldMid[i] });
        i++;
        j++;
      } else if (dp[(i + 1) * cols + j] >= dp[i * cols + (j + 1)]) {
        rows.push({ kind: 'del', oldLine: head + i + 1, text: oldMid[i] });
        i++;
      } else {
        rows.push({ kind: 'add', newLine: head + j + 1, text: newMid[j] });
        j++;
      }
    }
    while (i < m) {
      rows.push({ kind: 'del', oldLine: head + i + 1, text: oldMid[i] });
      i++;
    }
    while (j < n) {
      rows.push({ kind: 'add', newLine: head + j + 1, text: newMid[j] });
      j++;
    }
  }

  for (let k = 0; k < tail; k++) {
    const oldIdx = oldLines.length - tail + k;
    const newIdx = newLines.length - tail + k;
    rows.push({ kind: 'ctx', oldLine: oldIdx + 1, newLine: newIdx + 1, text: oldLines[oldIdx] });
  }

  return capRows(collapseContext(rows));
}

function capRows(rows: DiffRow[]): DiffRow[] {
  if (rows.length <= 1400) return rows;
  return [
    ...rows.slice(0, 1000),
    { kind: 'gap', text: `${rows.length - 1200} more lines hidden` },
    ...rows.slice(rows.length - 200)
  ];
}

function collapseContext(rows: DiffRow[]): DiffRow[] {
  const ctxRadius = 3;
  const keep = new Uint8Array(rows.length);
  for (let i = 0; i < rows.length; i++) {
    if (rows[i].kind === 'add' || rows[i].kind === 'del') {
      const from = Math.max(0, i - ctxRadius);
      const to = Math.min(rows.length - 1, i + ctxRadius);
      for (let k = from; k <= to; k++) keep[k] = 1;
    }
  }
  const out: DiffRow[] = [];
  let inGap = false;
  for (let i = 0; i < rows.length; i++) {
    if (keep[i]) {
      out.push(rows[i]);
      inGap = false;
    } else if (!inGap) {
      out.push({ kind: 'gap', text: '...' });
      inGap = true;
    }
  }
  return out.length > 0 ? out : rows.slice(0, 80);
}

function openCommentBox(filePath: string, rowKey: string, row: DiffRow) {
  const lineNumber = row.newLine ?? row.oldLine ?? 1;
  const lineType = row.kind === 'add' ? 'add' : row.kind === 'del' ? 'del' : 'ctx';
  activeCommentTarget = {
    path: filePath,
    rowKey,
    lineNumber,
    lineType,
    lineText: row.text
  };
  draftComment = '';
}

function submitInlineComment() {
  if (!activeCommentTarget || !draftComment.trim()) return;
  chatStore.addInlineReviewComment({
    path: activeCommentTarget.path,
    lineNumber: activeCommentTarget.lineNumber,
    lineType: activeCommentTarget.lineType,
    lineText: activeCommentTarget.lineText,
    comment: draftComment.trim()
  });
  activeCommentTarget = null;
  draftComment = '';
}
</script>

{#if chatStore.diffReview}
  <aside
    transition:slide={{ axis: 'x', duration: 180 }}
    class="h-full w-[48%] min-w-[380px] max-w-[780px] shrink-0 border-l border-white/[0.08] bg-[#0D0D10] flex flex-col overflow-hidden"
  >
    <div class="h-10 px-3 border-b border-white/[0.08] flex items-center justify-between gap-2 shrink-0 bg-[#121215]">
      <div class="flex items-center gap-2 min-w-0">
        <span class="px-1.5 py-0.5 rounded-[4px] bg-[#1E1E24] border border-white/[0.08] text-[10.5px] font-medium text-[#EDEDEF] shrink-0">
          For Turn
        </span>
        <span class="text-xs text-[#A3A3AA] truncate">{chatStore.diffReview.title}</span>
      </div>

      <div class="flex items-center gap-1.5 shrink-0">
        {#if chatStore.inlineReviewComments.length > 0}
          <button
            type="button"
            onclick={() => chatStore.sendInlineReviewComments()}
            class="h-6 px-2.5 rounded-[5px] bg-[#2563EB] hover:bg-[#3B82F6] text-[11px] font-medium text-white transition-colors"
          >
            Send Review ({chatStore.inlineReviewComments.length})
          </button>
        {/if}
        <button
          type="button"
          onclick={() => chatStore.closeDiffReview()}
          class="p-1 rounded-[4px] text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
          title="Close review"
        >
          <Icon name="close" class="w-3.5 h-3.5" strokeWidth={1.7} />
        </button>
      </div>
    </div>

    <div class="flex-1 overflow-y-auto divide-y divide-white/[0.07]">
      {#if chatStore.diffReview.loading}
        <div class="p-6 text-xs text-[#73737A]">Loading diff...</div>
      {:else if chatStore.diffReview.files.length === 0}
        <div class="p-6 text-xs text-[#73737A]">No snapshot diffs available for this turn.</div>
      {:else}
        {#each chatStore.diffReview.files as file (file.path)}
          {@const parts = splitPath(file.path)}
          {@const isCollapsed = Boolean(collapsedFiles[file.path])}
          {@const rows = isCollapsed ? [] : buildDiffRows(file)}

          <div class="bg-[#0D0D10]">
            <button
              type="button"
              onclick={() => (collapsedFiles = { ...collapsedFiles, [file.path]: !isCollapsed })}
              class="w-full px-3 py-2 bg-[#141418] hover:bg-[#19191E] border-b border-white/[0.06] flex items-center justify-between gap-2 text-left transition-colors"
            >
              <div class="flex items-baseline gap-2 min-w-0">
                <span class="font-mono text-xs font-medium text-[#EDEDEF] truncate">{parts.fileName}</span>
                {#if parts.dirPath}
                  <span class="font-mono text-[11px] text-[#6E6E77] truncate">{parts.dirPath}</span>
                {/if}
              </div>

              <div class="flex items-center gap-2 shrink-0 font-mono text-[11px]">
                <span class="text-[#4ADE80]">+{file.addedLines}</span>
                <span class="text-[#F87171]">-{file.removedLines}</span>
                <Icon
                  name="chevron-down"
                  class="w-3.5 h-3.5 text-[#73737A] transition-transform duration-150 {isCollapsed ? '-rotate-90' : ''}"
                  strokeWidth={1.6}
                />
              </div>
            </button>

            {#if !isCollapsed}
              <div class="font-mono text-[11.5px] leading-[19px] select-text">
                {#each rows as row, rIdx (`${file.path}-${rIdx}`)}
                  {#if row.kind === 'gap'}
                    <div class="px-4 py-0.5 bg-[#121216] text-[#55555E] text-[10.5px] border-y border-white/[0.04]">
                      {row.text === '...' ? '···' : row.text}
                    </div>
                  {:else}
                    {@const rowKey = `${file.path}:${rIdx}`}
                    {@const lineNum = row.newLine ?? row.oldLine ?? 0}
                    {@const lineComments = chatStore.inlineReviewComments.filter((c) => c.path === file.path && c.lineNumber === lineNum)}

                    <div
                      class="group/line flex items-stretch relative {row.kind === 'add' ? 'bg-emerald-500/[0.12]' : row.kind === 'del' ? 'bg-rose-500/[0.14]' : 'hover:bg-white/[0.03]'}"
                    >
                      <div class="w-10 shrink-0 px-1.5 text-right text-[10.5px] text-[#5E5E66] select-none border-r border-white/[0.05]">
                        {row.oldLine ?? ''}
                      </div>
                      <div class="w-10 shrink-0 px-1.5 text-right text-[10.5px] text-[#5E5E66] select-none border-r border-white/[0.05] relative">
                        <span class="group-hover/line:opacity-0 transition-opacity">{row.newLine ?? ''}</span>
                        <button
                          type="button"
                          onclick={() => openCommentBox(file.path, rowKey, row)}
                          title="Add inline comment"
                          class="opacity-0 group-hover/line:opacity-100 absolute right-1 top-1/2 -translate-y-1/2 w-4 h-4 rounded-[4px] bg-[#2563EB] hover:bg-[#3B82F6] text-white flex items-center justify-center text-[12px] leading-none font-sans transition-opacity"
                        >
                          +
                        </button>
                      </div>
                      <div class="flex-1 px-3 whitespace-pre-wrap break-all {row.kind === 'add' ? 'text-[#D1FAE5]' : row.kind === 'del' ? 'text-[#FECDD3]' : 'text-[#D4D4D8]'}">
                        {row.text}
                      </div>
                    </div>

                    {#each lineComments as c (c.id)}
                      <div class="mx-4 my-1.5 px-3 py-2 rounded-[6px] bg-[#161822] border border-[#2563EB]/40 flex items-start justify-between gap-2 text-xs font-sans">
                        <div class="min-w-0">
                          <span class="font-mono text-[10.5px] text-[#60A5FA] mr-1.5">L{c.lineNumber}</span>
                          <span class="text-[#EDEDEF]">{c.comment}</span>
                        </div>
                        <button
                          type="button"
                          onclick={() => chatStore.removeInlineReviewComment(c.id)}
                          class="text-[#73737A] hover:text-[#F87171] shrink-0"
                          title="Remove comment"
                        >
                          ×
                        </button>
                      </div>
                    {/each}

                    {#if activeCommentTarget?.rowKey === rowKey}
                      <div transition:slide={{ duration: 140 }} class="mx-4 my-2 p-3 rounded-[8px] bg-[#141418] border border-white/[0.12] space-y-2.5 font-sans">
                        <textarea
                          bind:value={draftComment}
                          rows="2"
                          placeholder="Leave a comment"
                          class="w-full bg-transparent text-xs text-[#EDEDEF] placeholder-[#66666D] outline-none resize-none"
                        ></textarea>
                        <div class="flex items-center justify-end gap-2">
                          <button
                            type="button"
                            onclick={() => (activeCommentTarget = null)}
                            class="h-7 px-3 rounded-[6px] text-xs text-[#A3A3AA] hover:text-[#EDEDEF]"
                          >
                            Cancel
                          </button>
                          <button
                            type="button"
                            disabled={!draftComment.trim()}
                            onclick={submitInlineComment}
                            class="h-7 px-3.5 rounded-full bg-[#2563EB] hover:bg-[#3B82F6] disabled:opacity-40 text-xs font-medium text-white transition-colors"
                          >
                            Add Comment
                          </button>
                        </div>
                      </div>
                    {/if}
                  {/if}
                {/each}
              </div>
            {/if}
          </div>
        {/each}
      {/if}
    </div>
  </aside>
{/if}

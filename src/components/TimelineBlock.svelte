<script lang="ts">
import { themeStore } from '../lib/stores/theme.svelte';
import { slide } from 'svelte/transition';
import { convertFileSrc } from '@tauri-apps/api/core';
import Icon from './Icon.svelte';
import FileToolIcon from './FileToolIcon.svelte';
import type { ToolExecutionRecord } from '../lib/toolMapper';
import type { TimelineStep } from '../lib/stores/chat.svelte';
import { renderMarkdown } from '../lib/markdown';
import { chatStore } from '../lib/stores/chat.svelte';

let {
  steps = [],
  reasoningContent = '',
  toolCalls = [],
  isStreaming = false,
  paused = false,
  canResume = true,
  hasFinalContent = false,
  statusText = '',
  retryReason = '',
  startedAt = 0,
  finishedAt = 0,
  error = '',
  onRetry = () => {},
  onContinue = () => {}
}: {
  steps?: TimelineStep[];
  reasoningContent?: string;
  toolCalls?: ToolExecutionRecord[];
  isStreaming?: boolean;
  paused?: boolean;
  canResume?: boolean;
  hasFinalContent?: boolean;
  statusText?: string;
  retryReason?: string;
  startedAt?: number;
  finishedAt?: number;
  error?: string;
  onRetry?: () => void;
  onContinue?: () => void;
} = $props();

let pipelineExpanded = $state<boolean>(false);
let userToggledPipeline = $state<boolean>(false);
let prevStreamingState = false;

$effect(() => {
  if (isStreaming && !prevStreamingState) {
    userToggledPipeline = false;
    pipelineExpanded = true;
  }
  prevStreamingState = isStreaming;

  if (!userToggledPipeline) {
    if (canResume && (error || paused)) {
      pipelineExpanded = true;
    } else if (!isStreaming || hasFinalContent) {
      pipelineExpanded = false;
    } else {
      pipelineExpanded = true;
    }
  }
});
let expandedThoughts = $state<Record<string, boolean>>({});
let expandedTools = $state<Record<string, boolean>>({});
let expandedCompacts = $state<Record<string, boolean>>({});
let showErrorTrace = $state<boolean>(false);
let confirmRetryErr = $state<boolean>(false);
let confirmRetryTimer: ReturnType<typeof setTimeout> | null = null;

function handleRetryErrorClick() {
  if (confirmRetryErr) {
    if (confirmRetryTimer) clearTimeout(confirmRetryTimer);
    confirmRetryErr = false;
    onRetry();
    return;
  }
  confirmRetryErr = true;
  if (confirmRetryTimer) clearTimeout(confirmRetryTimer);
  confirmRetryTimer = setTimeout(() => {
    confirmRetryErr = false;
    confirmRetryTimer = null;
  }, 4000);
}

function buildMiniDiffLines(call: ToolExecutionRecord): Array<{ type: 'add' | 'del' | 'same'; text: string }> {
  if (call.diff?.changes && call.diff.changes.length > 0) {
    const changes = call.diff.changes;
    const keep = new Set<number>();
    for (let i = 0; i < changes.length; i++) {
      if (changes[i].type !== 'same') {
        for (let j = Math.max(0, i - 2); j <= Math.min(changes.length - 1, i + 2); j++) {
          keep.add(j);
        }
      }
    }
    if (keep.size > 0) {
      return changes.filter((_, idx) => keep.has(idx));
    }
    return changes;
  }
  const meta = extractEditTexts(call);
  const out: Array<{ type: 'add' | 'del' | 'same'; text: string }> = [];
  const maxPairs = Math.max(meta.removedTexts.length, meta.addedTexts.length);
  for (let i = 0; i < maxPairs; i++) {
    const rem = meta.removedTexts[i];
    const add = meta.addedTexts[i];
    if (rem) {
      for (const l of rem.split('\n')) out.push({ type: 'del', text: l });
    }
    if (add) {
      for (const l of add.split('\n')) out.push({ type: 'add', text: l });
    }
  }
  return out;
}
let errorCopied = $state<boolean>(false);
let nowMs = $state<number>(Date.now());

$effect(() => {
  if (!isStreaming) return;
  nowMs = Date.now();
  const id = setInterval(() => {
    nowMs = Date.now();
  }, 1000);
  return () => clearInterval(id);
});

let normalizedSteps = $derived.by<TimelineStep[]>(() => {
  if (steps && steps.length > 0) return steps;
  const fallback: TimelineStep[] = [];
  if (reasoningContent) {
    fallback.push({
      kind: 'thought',
      id: 'legacy-thought',
      text: reasoningContent,
      startedAt: startedAt || Date.now(),
      finishedAt: finishedAt || Date.now()
    });
  }
  for (const tc of toolCalls || []) {
    fallback.push({ kind: 'tool', record: tc });
  }
  return fallback;
});

function formatDuration(diffMs: number, allowMs = false): string {
  if (diffMs < 1000) {
    return allowMs ? `${Math.max(1, Math.round(diffMs))}ms` : '0s';
  }
  const totalSecs = Math.floor(diffMs / 1000);
  const hrs = Math.floor(totalSecs / 3600);
  const mins = Math.floor((totalSecs % 3600) / 60);
  const secs = totalSecs % 60;

  if (hrs > 0) {
    return mins > 0 ? `${hrs}h ${mins}m ${secs}s` : `${hrs}h ${secs}s`;
  }
  if (mins > 0) {
    return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
  }
  return `${secs}s`;
}

let totalDurationLabel = $derived.by(() => {
  const end = finishedAt || (isStreaming ? nowMs : 0);
  const diffMs =
    startedAt && end >= startedAt
      ? end - startedAt
      : toolCalls.reduce((a, c) => a + (c.elapsedMs || 0), 0);
  return formatDuration(diffMs, !isStreaming);
});

function formatThoughtDuration(step: Extract<TimelineStep, { kind: 'thought' }>): string {
  const diff = Math.max(0, (step.finishedAt || nowMs) - step.startedAt);
  if (diff < 1000) return '1s';
  return formatDuration(diff, false);
}

function formatCompactTokens(t: number): string {
  if (t >= 1000) return `${Math.round(t / 1000)}k`;
  return `${t}`;
}

function extractEditTexts(record: ToolExecutionRecord): { filePath: string; addedTexts: string[]; removedTexts: string[]; added: number; removed: number } {
  const filePath = String(record.args?.path || record.args?.Path || record.previewArgs || '');
  const addedTexts: string[] = [];
  const removedTexts: string[] = [];
  let added = 0;
  let removed = 0;

  if (record.diff?.changes) {
    for (const c of record.diff.changes) {
      if (c.type === 'add') {
        added++;
        addedTexts.push(c.text);
      } else if (c.type === 'del') {
        removed++;
        removedTexts.push(c.text);
      }
    }
  } else if (Array.isArray(record.args?.edits)) {
    for (const ed of record.args.edits) {
      const oldT = typeof ed?.old_text === 'string' ? ed.old_text : '';
      const newT = typeof ed?.new_text === 'string' ? ed.new_text : '';
      if (oldT) {
        removed += oldT.split('\n').length;
        removedTexts.push(oldT);
      }
      if (newT) {
        added += newT.split('\n').length;
        addedTexts.push(newT);
      }
    }
  } else if (typeof record.args?.content === 'string' && record.toolName.toLowerCase().includes('write')) {
    added += record.args.content.split('\n').length;
    addedTexts.push(record.args.content);
  }
  return { filePath, addedTexts, removedTexts, added, removed };
}

function formatToolLabel(record: ToolExecutionRecord): { verb: string; target: string; added?: number; removed?: number } {
  const n = record.toolName.toLowerCase();
  if (n === 'tool_hub' && record.args?.action === 'load_skill') {
    return { verb: 'Skill', target: String(record.args?.name || 'skill') };
  }
  if (n.includes('read')) {
    const { filePath } = extractEditTexts(record);
    const fileName = filePath ? (filePath.split(/[\\/]/).pop() || filePath) : '';
    let rangeSuffix = '';
    let startLine = Number(record.args?.offset ?? record.args?.start_line ?? record.args?.line ?? 0);
    let endLine = Number(record.args?.end_line ?? 0);
    if (record.output && record.output.trim().startsWith('{')) {
      try {
        const parsed = JSON.parse(record.output);
        if (typeof parsed?.startLine === 'number') startLine = parsed.startLine;
        if (typeof parsed?.endLine === 'number') endLine = parsed.endLine;
        else if (typeof parsed?.linesRead === 'number' && parsed.linesRead > 0) {
          const s0 = startLine > 0 ? startLine : 1;
          endLine = s0 + parsed.linesRead - 1;
          startLine = s0;
        } else if (typeof parsed?.content === 'string') {
          const lineCnt = parsed.content.split('\n').length;
          const s0 = startLine > 0 ? startLine : 1;
          endLine = s0 + Math.max(0, lineCnt - 1);
          startLine = s0;
        }
      } catch {}
    } else if (startLine > 0 && Number(record.args?.limit) > 0) {
      endLine = startLine + Number(record.args.limit) - 1;
    }
    if (startLine > 0 && endLine >= startLine) {
      rangeSuffix = ` #L${startLine}-${endLine}`;
    }
    return { verb: 'Read', target: `${fileName || record.actionName}${rangeSuffix}` };
  }
  if (record.args?.description && typeof record.args.description === 'string') {
    return { verb: '', target: record.args.description };
  }
  const { filePath, added, removed } = extractEditTexts(record);
  const fileName = filePath ? (filePath.split(/[\\/]/).pop() || filePath) : '';

  if (n.includes('edit') || n.includes('write')) {
    const isErr = record.status === 'error';
    return {
      verb: isErr ? 'Edit' : 'Edited',
      target: fileName || record.actionName,
      added: isErr ? 0 : added,
      removed: isErr ? 0 : removed
    };
  }
  if (n.includes('read')) {
    return { verb: 'Read', target: fileName || record.actionName };
  }
  if (n.includes('glob')) {
    return { verb: 'Glob', target: record.args?.pattern || fileName || record.actionName };
  }
  if (n.includes('grep') || n.includes('search')) {
    const q = record.args?.pattern || record.args?.query || fileName;
    return { verb: 'Searched', target: q ? String(q) : record.actionName };
  }
  if (n.includes('terminal_exec') || n.includes('run_command') || n.includes('pty') || n.includes('git')) {
    const cmd = record.args?.command || record.args?.CommandLine || record.previewArgs;
    return { verb: 'Ran', target: cmd ? String(cmd) : record.actionName };
  }
  if (n.includes('computer_use')) {
    const act = String(record.args?.action || 'action');
    if (act === 'launch_app') return { verb: 'Launched', target: String(record.args?.app || 'app') };
    if (act === 'bind_window' || act === 'focus_window') {
      return { verb: 'Bound', target: String(record.args?.window_title || record.args?.process_name || record.args?.hwnd || 'window') };
    }
    if (act === 'screenshot') return { verb: 'Captured', target: String(record.args?.window_title || 'window') };
    if (act === 'snapshot_ui') return { verb: 'Inspected UI', target: String(record.args?.filter || 'controls') };
    if (act === 'uia_action' || act === 'click_element' || act === 'set_value') {
      const el = record.args?.element_id || record.args?.element_name || record.args?.automation_id || 'element';
      return { verb: 'UIA', target: `${record.args?.uia_op || act} ${el}` };
    }
    if (act === 'type') return { verb: 'Typed', target: `${String(record.args?.text || '').slice(0, 36)}` };
    if (act === 'hotkey') return { verb: 'Hotkey', target: String(record.args?.keys || '') };
    if (act.includes('click') || act === 'drag' || act === 'scroll') {
      return { verb: 'Ghost Input', target: `${act} (${record.args?.x ?? 500}, ${record.args?.y ?? 500})` };
    }
    return { verb: 'Computer Use', target: act };
  }
  return { verb: record.actionName, target: record.previewArgs || '' };
}

function extractToolImagePreview(call: ToolExecutionRecord): string | null {
  const raw = (call.output || '').trim();
  if (raw.startsWith('{')) {
    try {
      const parsed = JSON.parse(raw);
      if (typeof parsed?.dataUrl === 'string' && parsed.dataUrl.startsWith('data:image/')) {
        return parsed.dataUrl;
      }
      if (typeof parsed?.previewUrl === 'string' && parsed.previewUrl) {
        return parsed.previewUrl;
      }
    } catch {}
  }
  const p = typeof call.args?.path === 'string' ? call.args.path.trim() : '';
  if (p && /\.(png|jpg|jpeg|webp|gif|bmp)$/i.test(p)) {
    try {
      return convertFileSrc(p);
    } catch {}
  }
  return null;
}

function formatToolOutputReadable(call: ToolExecutionRecord): string {
  const raw = (call.output || '').trim();
  if (!raw) return '';
  if (raw.startsWith('{') || raw.startsWith('[')) {
    try {
      const parsed = JSON.parse(raw);
      if (typeof parsed?.content === 'string') return parsed.content;
      if (typeof parsed?.output === 'string') return parsed.output;
      if (typeof parsed?.text === 'string') return parsed.text;
      if (typeof parsed?.stdout === 'string') {
        return parsed.stderr ? `${parsed.stdout}\n${parsed.stderr}` : parsed.stdout;
      }
      if (typeof parsed?.markdown === 'string') return parsed.markdown;
      if (typeof parsed?.httpStatus === 'number') {
        const preview = typeof parsed?.htmlPreview === 'string' ? parsed.htmlPreview.trim() : '';
        return preview ? `HTTP ${parsed.httpStatus}\n${preview}` : `HTTP ${parsed.httpStatus}`;
      }
      if (Array.isArray(parsed?.files)) return parsed.files.join('\n');
      if (Array.isArray(parsed?.matches)) {
        return parsed.matches
          .map((m: any) => `${m.file}:${m.line}\n${m.snippet}`)
          .join('\n\n');
      }
      if (Array.isArray(parsed?.items)) {
        return parsed.items.map((it: any) => `[${it.status}] ${it.text}`).join('\n');
      }
      const searchArr = Array.isArray(parsed) ? parsed : Array.isArray(parsed?.results) ? parsed.results : null;
      if (searchArr) {
        if (searchArr.length === 0) return 'No results found.';
        return searchArr
          .map((item: any) => {
            if (typeof item === 'string') return item;
            if (item && typeof item === 'object') {
              const title = item.title || item.name || '';
              const url = item.url || item.link || '';
              const snippet = item.snippet || item.description || item.content || '';
              if (title || url || snippet) {
                return [title, url, snippet].filter(Boolean).join('\n');
              }
            }
            return JSON.stringify(item);
          })
          .join('\n\n');
      }
      if (typeof parsed?.error === 'string') return parsed.error;
      if (typeof parsed === 'object' && parsed !== null) {
        const hiddenKeys = new Set(['action', 'url', 'snapshotId', 'session_id', 'policy', 'running', 'dataUrl', 'base64', 'system_reminder']);
        const entries = Object.entries(parsed).filter(
          ([k, v]) => !hiddenKeys.has(k) && v !== null && v !== undefined && v !== ''
        );
        if (entries.length === 0) return '';
        if (entries.length === 1) {
          const [, v] = entries[0];
          if (v && typeof v === 'object') {
            return Object.entries(v)
              .map(([sk, sv]) => `${sk}: ${typeof sv === 'object' ? JSON.stringify(sv) : String(sv)}`)
              .join('\n');
          }
          return String(v);
        }
        return entries
          .map(([k, v]) => {
            if (Array.isArray(v)) {
              return `${k}: ${v.map((x) => (typeof x === 'object' ? (x?.name || x?.path || x?.title || JSON.stringify(x)) : String(x))).join(', ')}`;
            }
            if (v && typeof v === 'object') {
              const sub = Object.entries(v)
                .map(([sk, sv]) => `${sk}=${typeof sv === 'object' ? JSON.stringify(sv) : String(sv)}`)
                .join(', ');
              return `${k}: ${sub}`;
            }
            return `${k}: ${String(v)}`;
          })
          .join('\n');
        return entries
          .map(([k, v]) => `${k}: ${typeof v === 'object' ? JSON.stringify(v) : String(v)}`)
          .join('\n');
      }
    } catch {}
  }
  return raw;
}

function formatFriendlyError(raw: string): string {
  if (raw === 'User canceled execution') return raw;
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
  return raw.split('\n')[0] || raw;
}

async function copyRawError() {
  if (!error) return;
  try {
    await navigator.clipboard.writeText(error);
    errorCopied = true;
    setTimeout(() => {
      errorCopied = false;
    }, 1500);
  } catch {}
}

function toggleThought(id: string) {
  expandedThoughts[id] = !expandedThoughts[id];
}

function toggleCompact(id: string) {
  expandedCompacts[id] = !expandedCompacts[id];
}

function toggleTool(id: string) {
  expandedTools[id] = !expandedTools[id];
}
</script>

{#if isStreaming || (paused && canResume) || normalizedSteps.length > 0 || (error && (canResume || error !== 'User canceled execution'))}
  {@const isCanceled = error === 'User canceled execution'}
  <div data-omni-timeline="" class="w-full my-1.5 text-[14.5px] select-none">
    {#if isStreaming && normalizedSteps.length === 0 && !error}
      <div class="relative inline-flex items-center gap-2 py-1 group/retry">
        <span class="font-sans text-[14px] font-medium shimmer-text">
          {statusText || themeStore.t('working', 'Working')} · {totalDurationLabel}
        </span>
        {#if retryReason}
          <div class="pointer-events-none absolute left-0 bottom-full mb-1.5 z-30 max-w-md px-2.5 py-1.5 rounded-[6px] bg-[#18181B] border border-white/[0.12] shadow-lg font-mono text-[11px] text-[#F87171] whitespace-pre-wrap opacity-0 translate-y-0.5 group-hover/retry:opacity-100 group-hover/retry:translate-y-0 transition-all duration-150">
            {retryReason}
          </div>
        {/if}
      </div>
    {:else if normalizedSteps.length > 0}
      <button
        type="button"
        data-omni-timeline-header=""
        class="flex items-center gap-1.5 py-1 text-[#A3A3AA] hover:text-[#EDEDEF] transition-colors"
        onclick={() => {
          userToggledPipeline = true;
          pipelineExpanded = !pipelineExpanded;
        }}
      >
        {#if isStreaming}
          <span class="relative inline-flex items-center group/retry">
            <span class="font-sans text-[14px] font-medium shimmer-text">
              {statusText || themeStore.t('working', 'Working')} · {totalDurationLabel}
            </span>
            {#if retryReason}
              <span class="pointer-events-none absolute left-0 bottom-full mb-1.5 z-30 max-w-md px-2.5 py-1.5 rounded-[6px] bg-[#18181B] border border-white/[0.12] shadow-lg font-mono text-[11px] text-[#F87171] whitespace-pre-wrap text-left opacity-0 translate-y-0.5 group-hover/retry:opacity-100 group-hover/retry:translate-y-0 transition-all duration-150">
                {retryReason}
              </span>
            {/if}
          </span>
        {:else if paused}
          <span class="font-sans text-[14px] font-medium text-[#D4D4D8]">Paused · {totalDurationLabel}</span>
        {:else}
          <span class="font-sans text-[14px] font-medium">{themeStore.t('worked_for', 'Worked for')} {totalDurationLabel}</span>
        {/if}
        <Icon
          name="chevron-down"
          class="w-3.5 h-3.5 text-[#66666D] transition-transform duration-150 {pipelineExpanded ? 'rotate-180' : ''}"
          strokeWidth={1.7}
        />
      </button>

      {#if pipelineExpanded}
        <div transition:slide={{ duration: 150 }} class="mt-1 pl-3 space-y-1">
          {#each normalizedSteps as step, idx (step.kind === 'tool' ? step.record.toolCallId : step.id)}
            {#if step.kind === 'compact'}
              <div>
                <button
                  type="button"
                  class="flex items-center gap-1.5 py-0.5 text-left text-[#8E8E95] hover:text-[#D4D4D8] transition-colors"
                  onclick={() => toggleCompact(step.id)}
                >
                  <span class="font-sans text-[12.5px]">
                    Сжат контекст: {formatCompactTokens(step.beforeTokens)} → {formatCompactTokens(step.afterTokens)} токенов
                  </span>
                  <Icon
                    name="chevron-right"
                    class="w-3 h-3 text-[#66666D] transition-transform duration-150 {expandedCompacts[step.id] ? 'rotate-90' : ''}"
                    strokeWidth={1.8}
                  />
                </button>
                {#if expandedCompacts[step.id] && step.summary}
                  <div
                    transition:slide={{ duration: 150 }}
                    class="mt-1 mb-1.5 pl-3 border-l border-white/[0.08] font-mono text-[11px] text-[#8E8E95] whitespace-pre-wrap leading-relaxed max-h-64 overflow-y-auto select-text"
                  >
                    {step.summary}
                  </div>
                {/if}
              </div>
            {:else if step.kind === 'message'}
              <div class="markdown-prose font-sans text-[13.5px] text-[#EDEDEF] leading-relaxed select-text py-1">
                {@html renderMarkdown(step.text)}
              </div>
            {:else if step.kind === 'thought'}
              {@const isLastThoughtStreaming = isStreaming && idx === normalizedSteps.length - 1}
              <div>
                <button
                  type="button"
                  class="flex items-center gap-1.5 py-0.5 text-left text-[#8E8E95] hover:text-[#D4D4D8] transition-colors"
                  onclick={() => toggleThought(step.id)}
                >
                  <span class="font-sans text-[12.5px] {isLastThoughtStreaming ? 'shimmer-text' : ''}">
                    {isLastThoughtStreaming ? 'Thinking...' : `Thought for ${formatThoughtDuration(step)}`}
                  </span>
                  <Icon
                    name="chevron-right"
                    class="w-3 h-3 text-[#66666D] transition-transform duration-150 {expandedThoughts[step.id] ? 'rotate-90' : ''}"
                    strokeWidth={1.8}
                  />
                </button>
                {#if expandedThoughts[step.id]}
                  <div
                    transition:slide={{ duration: 150 }}
                    data-omni-thought-body=""
                    class="thought-prose mt-1 mb-1.5 pl-3 border-l border-white/[0.08] font-sans text-[13.5px] text-[#8E8E95] leading-relaxed max-h-64 overflow-y-auto select-text"
                  >
                    {@html renderMarkdown(step.text)}
                  </div>
                {/if}
              </div>
            {:else}
              {@const call = step.record}
              {@const info = formatToolLabel(call)}
              {@const isToolRunning = isStreaming && (call.status === 'running' || call.status === 'pending')}
              {@const readableOut = formatToolOutputReadable(call)}
              {@const editMeta = extractEditTexts(call)}
              {@const isFileTool = call.toolName.toLowerCase().includes('edit') || call.toolName.toLowerCase().includes('write') || call.toolName.toLowerCase().includes('read')}
              <div>
                <button
                  type="button"
                  data-omni-tool-row=""
                  class="w-full flex items-center gap-2 py-0.5 text-left group/step transition-colors"
                  onclick={() => toggleTool(call.toolCallId)}
                >
                  <FileToolIcon name={String(call.args?.path || call.args?.Path || info.target || '')} toolName={call.toolName} class="w-3.5 h-3.5" />
                  <span class="font-sans text-[13.5px] truncate {isToolRunning ? 'shimmer-text' : 'text-[#A3A3AA] group-hover/step:text-[#EDEDEF]'}">
                    {#if info.verb}{info.verb}{/if}
                    {#if info.target}
                      {#if isFileTool && editMeta.filePath}
                        <span
                          role="button"
                          tabindex="0"
                          class="{isToolRunning ? '' : 'text-[#D4D4D8] hover:text-white hover:underline'} {info.verb ? 'ml-1' : ''}"
                          onclick={(e) => {
                            e.stopPropagation();
                            chatStore.openFilePreview(editMeta.filePath, editMeta.addedTexts, editMeta.removedTexts);
                          }}
                          onkeydown={(e) => {
                            if (e.key === 'Enter') {
                              e.stopPropagation();
                              chatStore.openFilePreview(editMeta.filePath, editMeta.addedTexts, editMeta.removedTexts);
                            }
                          }}
                        >{info.target}</span>
                      {:else}
                        <span class="{isToolRunning ? '' : 'text-[#D4D4D8]'} {info.verb ? 'ml-1' : ''}">{info.target}</span>
                      {/if}
                    {/if}
                  </span>
                  {#if info.added || info.removed}
                    <span class="font-mono text-[11px] shrink-0 flex items-center gap-1">
                      {#if info.added}<span class="text-[#4ADE80]">+{info.added}</span>{/if}
                      {#if info.removed}<span class="text-[#F87171]">-{info.removed}</span>{/if}
                    </span>
                  {/if}
                  {#if call.status === 'error'}
                    <span class="font-mono text-[10.5px] text-[#F87171] shrink-0">failed</span>
                  {:else if call.status === 'paused'}
                    <span class="font-mono text-[10.5px] text-[#A3A3AA] shrink-0">paused</span>
                  {:else if call.status === 'cancelled'}
                    <span class="font-mono text-[10.5px] text-[#73737A] shrink-0">canceled</span>
                  {/if}
                  <Icon
                    name="chevron-right"
                    class="w-3 h-3 text-[#55555C] group-hover/step:text-[#A3A3AA] shrink-0 transition-transform duration-150 {expandedTools[call.toolCallId] ? 'rotate-90' : ''}"
                    strokeWidth={1.8}
                  />
                </button>

                {#if expandedTools[call.toolCallId]}
                  {@const toolImg = extractToolImagePreview(call)}
                  <div transition:slide={{ duration: 150 }} class="mt-1 mb-1.5 pl-3 border-l border-white/[0.08] font-mono text-[11px] text-[#8E8E95] space-y-1.5 select-text">
                    {#if toolImg}
                      <img
                        src={toolImg}
                        alt="Captured target window"
                        class="rounded-[6px] border border-white/[0.08] max-h-72 w-auto object-contain bg-[#0B0B0D]"
                      />
                    {/if}
                    {#if call.terminalData}
                      <div class="text-[#D4D4D8] whitespace-pre-wrap break-all">$ {call.terminalData.command}</div>
                      {#if call.terminalData.stdout}
                        <pre class="text-[#9BA1B0] whitespace-pre-wrap break-all max-h-52 overflow-y-auto">{call.terminalData.stdout}</pre>
                      {/if}
                      {#if call.terminalData.stderr}
                        <pre class="text-[#F87171] whitespace-pre-wrap break-all max-h-40 overflow-y-auto">{call.terminalData.stderr}</pre>
                      {/if}
                    {:else if call.diff || ((call.toolName.toLowerCase().includes('edit') || call.toolName.toLowerCase().includes('write') || call.toolName.toLowerCase().includes('patch')) && buildMiniDiffLines(call).length > 0)}
                      {@const miniDiff = buildMiniDiffLines(call)}
                      <div data-omni-tool-diff="" class="rounded-[6px] border border-white/[0.06] bg-[#0B0B0D] py-1 max-h-60 overflow-y-auto">
                        {#each miniDiff as change}
                          <div class="px-2 py-0.5 flex gap-2 {change.type === 'add' ? 'bg-[#22C55E]/10 text-[#6EE7B7]' : change.type === 'del' ? 'bg-[#EF4444]/10 text-[#FCA5A5]' : 'text-[#73737A]'}">
                            <span class="w-3 shrink-0 select-none">{change.type === 'add' ? '+' : change.type === 'del' ? '-' : ' '}</span>
                            <span class="whitespace-pre-wrap break-all">{change.text || ' '}</span>
                          </div>
                        {/each}
                      </div>
                    {:else if call.error}
                      <pre class="text-[#F87171] whitespace-pre-wrap break-all max-h-48 overflow-y-auto">{call.error}</pre>
                    {:else if call.toolName === 'tool_hub' && call.args?.action === 'load_skill' && call.output}
                      {@const skillContent = (() => { try { const p = JSON.parse(call.output); return typeof p?.content === 'string' ? p.content : readableOut; } catch { return readableOut; } })()}
                      <div class="markdown-prose text-[12px] text-[#C4C4CA] max-h-64 overflow-y-auto pr-1">{@html renderMarkdown(skillContent)}</div>
                    {:else if readableOut}
                      <pre data-omni-tool-output="" class="text-[#9BA1B0] whitespace-pre-wrap break-all max-h-52 overflow-y-auto">{readableOut}</pre>
                    {:else if isToolRunning}
                      <div class="text-[#66666D] italic">Waiting for output...</div>
                    {:else}
                      <div class="text-[#66666D] italic">No output</div>
                    {/if}
                  </div>
                {/if}
              </div>
            {/if}
          {/each}

          {#if paused && canResume && !isStreaming && !error}
            <div class="pt-1 flex items-center gap-2">
              <span class="font-sans text-[12px] text-[#8E8E95]">Работа на паузе</span>
              <button
                type="button"
                class="px-2.5 py-0.5 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[11.5px] font-sans font-medium text-[#111113] transition-colors"
                onclick={onContinue}
              >
                Продолжить
              </button>
            </div>
          {/if}

          {#if error && (!isCanceled || canResume)}
            <div class="pt-0.5">
              <div class="flex items-center gap-2 py-0.5">
                {#if isCanceled}
                  <span class="font-sans text-[12.5px] text-[#73737A]">{error}</span>
                  {#if canResume}
                    <button
                      type="button"
                      class="px-2 py-0.5 rounded-[5px] bg-white/[0.08] hover:bg-white/[0.14] text-[11px] font-sans font-medium text-[#EDEDEF] transition-colors"
                      onclick={onContinue}
                    >
                      Продолжить
                    </button>
                  {/if}
                {:else}
                  <button
                    type="button"
                    class="flex items-center gap-1.5 min-w-0 text-left text-[#F87171]/90 hover:text-[#FCA5A5] transition-colors"
                    onclick={() => (showErrorTrace = !showErrorTrace)}
                  >
                    <span class="font-sans text-[13.5px] truncate">{formatFriendlyError(error)}</span>
                    <Icon
                      name="chevron-right"
                      class="w-3 h-3 text-[#F87171]/60 shrink-0 transition-transform duration-150 {showErrorTrace ? 'rotate-90' : ''}"
                      strokeWidth={1.8}
                    />
                  </button>
                  <div class="flex items-center gap-1 shrink-0 ml-1">
                    <button
                      type="button"
                      class="p-1 rounded-[4px] text-[#73737A] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors"
                      onclick={copyRawError}
                      title="Copy raw error"
                    >
                      {#if errorCopied}
                        <Icon name="check" class="w-3 h-3 text-[#D4D4D8]" strokeWidth={1.8} />
                      {:else}
                        <Icon name="copy" class="w-3 h-3" strokeWidth={1.6} />
                      {/if}
                    </button>
                    {#if canResume}
                      <button
                        type="button"
                        class="px-2 py-0.5 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[11px] font-sans font-medium text-[#111113] transition-colors"
                        onclick={onContinue}
                      >
                        Продолжить
                      </button>
                      <button
                        type="button"
                        class="px-2 py-0.5 rounded-[4px] text-[11px] font-sans transition-colors {confirmRetryErr ? 'bg-[#F59E0B]/15 text-[#F59E0B]' : 'text-[#A3A3AA] hover:text-[#EDEDEF] hover:bg-white/[0.06]'}"
                        onclick={handleRetryErrorClick}
                      >
                        {confirmRetryErr ? 'Confirm?' : 'Retry'}
                      </button>
                    {/if}
                  </div>
                {/if}
              </div>
              {#if showErrorTrace && !isCanceled}
                <div
                  transition:slide={{ duration: 150 }}
                  class="mt-1 mb-1 pl-3 border-l border-[#F87171]/25 font-mono text-[11px] text-[#A3A3AA] whitespace-pre-wrap break-all max-h-48 overflow-y-auto select-text"
                >
                  {error}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      {/if}
    {:else if paused && canResume && !isStreaming && !error}
      <div class="flex items-center gap-2 py-1">
        <span class="font-sans text-[12.5px] text-[#8E8E95]">Paused · {totalDurationLabel}</span>
        <button
          type="button"
          class="px-2.5 py-0.5 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[11.5px] font-sans font-medium text-[#111113] transition-colors"
          onclick={onContinue}
        >
          Продолжить
        </button>
      </div>
    {:else if error && (!isCanceled || canResume)}
      <div>
        <div class="flex items-center gap-2 py-0.5">
          {#if isCanceled}
            <span class="font-sans text-[12.5px] text-[#73737A]">{error}</span>
            {#if canResume}
              <button
                type="button"
                class="px-2 py-0.5 rounded-[5px] bg-white/[0.08] hover:bg-white/[0.14] text-[11px] font-sans font-medium text-[#EDEDEF] transition-colors"
                onclick={onContinue}
              >
                Продолжить
              </button>
            {/if}
          {:else}
            <button
              type="button"
              class="flex items-center gap-1.5 min-w-0 text-left text-[#F87171]/90 hover:text-[#FCA5A5] transition-colors"
              onclick={() => (showErrorTrace = !showErrorTrace)}
            >
              <span class="font-sans text-[13.5px] truncate">{formatFriendlyError(error)}</span>
              <Icon
                name="chevron-right"
                class="w-3 h-3 text-[#F87171]/60 shrink-0 transition-transform duration-150 {showErrorTrace ? 'rotate-90' : ''}"
                strokeWidth={1.8}
              />
            </button>
            <div class="flex items-center gap-1 shrink-0 ml-1">
              <button
                type="button"
                class="p-1 rounded-[4px] text-[#73737A] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors"
                onclick={copyRawError}
                title="Copy raw error"
              >
                {#if errorCopied}
                  <Icon name="check" class="w-3 h-3 text-[#D4D4D8]" strokeWidth={1.8} />
                {:else}
                  <Icon name="copy" class="w-3 h-3" strokeWidth={1.6} />
                {/if}
              </button>
              <button
                type="button"
                class="px-2 py-0.5 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[11px] font-sans font-medium text-[#111113] transition-colors"
                onclick={onContinue}
              >
                Продолжить
              </button>
              <button
                type="button"
                class="px-2 py-0.5 rounded-[4px] text-[11px] font-sans text-[#A3A3AA] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
                onclick={onRetry}
              >
                Retry
              </button>
            </div>
          {/if}
        </div>
        {#if showErrorTrace && !isCanceled}
          <div
            transition:slide={{ duration: 150 }}
            class="mt-1 mb-1 pl-3 border-l border-[#F87171]/25 font-mono text-[11px] text-[#A3A3AA] whitespace-pre-wrap break-all max-h-48 overflow-y-auto select-text"
          >
            {error}
          </div>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style>
  .thought-prose,
  .thought-prose :global(*) {
    color: #8e8e95 !important;
    font-size: 12px !important;
  }
  .thought-prose :global(strong),
  .thought-prose :global(h1),
  .thought-prose :global(h2),
  .thought-prose :global(h3),
  .thought-prose :global(h4),
  .thought-prose :global(code) {
    color: #a3a3aa !important;
  }
  .shimmer-text {
    background: linear-gradient(
      90deg,
      #8e8e95 0%,
      #8e8e95 35%,
      #ffffff 50%,
      #8e8e95 65%,
      #8e8e95 100%
    );
    background-size: 200% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    will-change: background-position;
    animation: omni-shimmer 2.2s linear infinite;
  }

  @keyframes omni-shimmer {
    0% {
      background-position: 200% 0;
    }
    100% {
      background-position: 0% 0;
    }
  }
</style>

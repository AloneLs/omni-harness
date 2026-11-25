<script lang="ts">
import { themeStore } from '../lib/stores/theme.svelte';
import { slide } from 'svelte/transition';
import { invoke } from '@tauri-apps/api/core';
import Icon from './Icon.svelte';
import TimelineStepRow from './TimelineStepRow.svelte';
import { type ToolExecutionRecord, isInternalArtifactTool } from '../lib/toolMapper';
import type { TimelineStep } from '../lib/stores/chat.svelte';
import { formatDuration, formatFriendlyError } from '../lib/timelineHelpers';

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

let expandedSteps = $state<Record<string, boolean>>({});
let restoringHwnds = $state<Record<number, boolean>>({});
let restoredHwnds = $state<Record<number, boolean>>({});

async function handleRestoreWindow(hwnd: number) {
  if (!hwnd) return;
  restoringHwnds[hwnd] = true;
  try {
    await invoke('cu_stage_window', { hwnd, offscreen: false });
    restoredHwnds[hwnd] = true;
    setTimeout(() => {
      restoredHwnds[hwnd] = false;
    }, 2500);
  } catch (e) {
    console.error('Failed to restore window:', e);
  } finally {
    restoringHwnds[hwnd] = false;
  }
}

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
  let list: TimelineStep[] = [];
  if (steps && steps.length > 0) {
    list = steps;
  } else {
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
    list = fallback;
  }
  let hasInternal = false;
  for (let i = 0; i < list.length; i++) {
    const s = list[i];
    if (s.kind === 'tool' && s.record && isInternalArtifactTool(s.record.toolName, s.record.rawArgs || s.record.args)) {
      hasInternal = true;
      break;
    }
  }
  if (!hasInternal) return list;
  return list.filter((s) => {
    if (s.kind === 'tool' && s.record) {
      return !isInternalArtifactTool(s.record.toolName, s.record.rawArgs || s.record.args);
    }
    return true;
  });
});

let totalDurationLabel = $derived.by(() => {
  const end = finishedAt || (isStreaming ? nowMs : 0);
  const diffMs =
    startedAt && end >= startedAt
      ? end - startedAt
      : toolCalls.reduce((a, c) => a + (c.elapsedMs || 0), 0);
  return formatDuration(diffMs, !isStreaming);
});

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

function toggleStep(key: string) {
  expandedSteps[key] = !expandedSteps[key];
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
            {@const stepKey = step.kind === 'tool' ? `tool:${step.record.toolCallId}` : `${step.kind}:${step.id}`}
            {@const isLastThoughtStreaming = step.kind === 'thought' && isStreaming && idx === normalizedSteps.length - 1}
            <TimelineStepRow
              {step}
              {isStreaming}
              {isLastThoughtStreaming}
              nowMs={step.kind === 'thought' && !step.finishedAt ? nowMs : 0}
              expanded={Boolean(expandedSteps[stepKey])}
              {restoringHwnds}
              {restoredHwnds}
              onToggle={() => toggleStep(stepKey)}
              onRestoreWindow={handleRestoreWindow}
            />
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
                        П��одолжить
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

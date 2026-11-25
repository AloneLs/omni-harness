<script lang="ts">
import { slide } from 'svelte/transition';
import Icon from './Icon.svelte';
import FileToolIcon from './FileToolIcon.svelte';
import { renderMarkdown } from '../lib/markdown';
import { chatStore, type TimelineStep } from '../lib/stores/chat.svelte';
import {
  formatDuration,
  formatCompactTokens,
  extractEditTexts,
  formatToolLabel,
  buildMiniDiffLines,
  parseComputerUse,
  extractToolImagePreview,
  parseGenerateImage,
  parseImageFile,
  parseWriteFile,
  parseReadFile,
  formatToolOutputReadable
} from '../lib/timelineHelpers';

let {
  step,
  isStreaming = false,
  isLastThoughtStreaming = false,
  nowMs = 0,
  expanded = false,
  restoringHwnds,
  restoredHwnds,
  onToggle,
  onRestoreWindow
}: {
  step: TimelineStep;
  isStreaming?: boolean;
  isLastThoughtStreaming?: boolean;
  nowMs?: number;
  expanded?: boolean;
  restoringHwnds: Record<number, boolean>;
  restoredHwnds: Record<number, boolean>;
  onToggle: () => void;
  onRestoreWindow: (hwnd: number) => void;
} = $props();

let thoughtDurationLabel = $derived.by(() => {
  if (step.kind !== 'thought') return '';
  if (isLastThoughtStreaming) return '';
  const diff = Math.max(0, (step.finishedAt || nowMs || Date.now()) - step.startedAt);
  if (diff < 1000) return '1s';
  return formatDuration(diff, false);
});

let messageHtml = $derived.by(() => {
  if (step.kind !== 'message') return '';
  return renderMarkdown(step.text);
});

let thoughtHtml = $derived.by(() => {
  if (step.kind !== 'thought' || !expanded) return '';
  return renderMarkdown(step.text);
});

let toolInfo = $derived.by(() => {
  if (step.kind !== 'tool') return null;
  return formatToolLabel(step.record, isStreaming);
});

let toolEditMeta = $derived.by(() => {
  if (step.kind !== 'tool') return null;
  return extractEditTexts(step.record);
});

let expandedToolView = $derived.by(() => {
  if (step.kind !== 'tool' || !expanded) return null;
  const call = step.record;
  const readableOut = formatToolOutputReadable(call);
  const genImg = parseGenerateImage(call);
  const imgFile = parseImageFile(call);
  const writeFile = parseWriteFile(call);
  const readFile = parseReadFile(call);
  const toolImg = genImg?.dataUrl || imgFile?.dataUrl || extractToolImagePreview(call);
  const cu = parseComputerUse(call);
  const isDiffTool =
    call.toolName.toLowerCase().includes('edit') ||
    call.toolName.toLowerCase().includes('write') ||
    call.toolName.toLowerCase().includes('patch');
  const miniDiff = !call.terminalData && (call.diff || isDiffTool) ? buildMiniDiffLines(call) : [];
  let skillHtml = '';
  if (call.toolName === 'tool_hub' && call.args?.action === 'load_skill' && call.output) {
    let skillContent = readableOut;
    try {
      const p = JSON.parse(call.output);
      if (typeof p?.content === 'string') skillContent = p.content;
    } catch {}
    skillHtml = renderMarkdown(skillContent);
  }
  return { readableOut, genImg, imgFile, writeFile, readFile, toolImg, cu, miniDiff, skillHtml };
});
</script>

<div class="timeline-step-row">
  {#if step.kind === 'compact'}
    <div>
      <button
        type="button"
        class="flex items-center gap-1.5 py-0.5 text-left text-[#8E8E95] hover:text-[#D4D4D8] transition-colors"
        onclick={onToggle}
      >
        <span class="font-sans text-[12.5px]">
          Сжат контекст: {formatCompactTokens(step.beforeTokens)} → {formatCompactTokens(step.afterTokens)} токенов
        </span>
        <Icon
          name="chevron-right"
          class="w-3 h-3 text-[#66666D] transition-transform duration-150 {expanded ? 'rotate-90' : ''}"
          strokeWidth={1.8}
        />
      </button>
      {#if expanded && step.summary}
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
      {@html messageHtml}
    </div>
  {:else if step.kind === 'thought'}
    <div>
      <button
        type="button"
        class="flex items-center gap-1.5 py-0.5 text-left text-[#8E8E95] hover:text-[#D4D4D8] transition-colors"
        onclick={onToggle}
      >
        <span class="font-sans text-[12.5px] {isLastThoughtStreaming ? 'shimmer-text' : ''}">
          {isLastThoughtStreaming ? 'Thinking...' : `Thought for ${thoughtDurationLabel}`}
        </span>
        <Icon
          name="chevron-right"
          class="w-3 h-3 text-[#66666D] transition-transform duration-150 {expanded ? 'rotate-90' : ''}"
          strokeWidth={1.8}
        />
      </button>
      {#if expanded}
        <div
          transition:slide={{ duration: 150 }}
          data-omni-thought-body=""
          class="thought-prose mt-1 mb-1.5 pl-3 border-l border-white/[0.08] font-sans text-[13.5px] text-[#8E8E95] leading-relaxed max-h-64 overflow-y-auto select-text"
        >
          {@html thoughtHtml}
        </div>
      {/if}
    </div>
  {:else if toolInfo && toolEditMeta}
    {@const call = step.record}
    {@const isToolRunning = isStreaming && (call.status === 'running' || call.status === 'pending')}
    {@const isFileTool = call.toolName.toLowerCase().includes('edit') || call.toolName.toLowerCase().includes('write') || call.toolName.toLowerCase().includes('read')}
    <div>
      <button
        type="button"
        data-omni-tool-row=""
        class="w-full flex items-center gap-2 py-0.5 text-left group/step transition-colors"
        onclick={onToggle}
      >
        <FileToolIcon name={String(call.args?.path || call.args?.Path || toolInfo.target || '')} toolName={call.toolName} class="w-3.5 h-3.5" />
        <span class="font-sans text-[13.5px] truncate {isToolRunning ? 'shimmer-text' : 'text-[#A3A3AA] group-hover/step:text-[#EDEDEF]'}">
          {#if toolInfo.verb}{toolInfo.verb}{/if}
          {#if toolInfo.target}
            {#if isFileTool && toolEditMeta.filePath}
              <span
                role="button"
                tabindex="0"
                class="{isToolRunning ? '' : 'text-[#D4D4D8] hover:text-white hover:underline'} {toolInfo.verb ? 'ml-1' : ''}"
                onclick={(e) => {
                  e.stopPropagation();
                  chatStore.openFilePreview(toolEditMeta.filePath, toolEditMeta.addedTexts, toolEditMeta.removedTexts);
                }}
                onkeydown={(e) => {
                  if (e.key === 'Enter') {
                    e.stopPropagation();
                    chatStore.openFilePreview(toolEditMeta.filePath, toolEditMeta.addedTexts, toolEditMeta.removedTexts);
                  }
                }}
              >{toolInfo.target}</span>
            {:else}
              <span class="{isToolRunning ? '' : 'text-[#D4D4D8]'} {toolInfo.verb ? 'ml-1' : ''}">{toolInfo.target}</span>
            {/if}
          {/if}
        </span>
        {#if toolInfo.added || toolInfo.removed}
          <span class="font-mono text-[11px] shrink-0 inline-flex items-center gap-1 leading-none self-center tabular-nums">
            {#if toolInfo.added}<span class="text-[#4ADE80]">+{toolInfo.added}</span>{/if}
            {#if toolInfo.removed}<span class="text-[#F87171]">-{toolInfo.removed}</span>{/if}
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
          class="w-3 h-3 text-[#55555C] group-hover/step:text-[#A3A3AA] shrink-0 transition-transform duration-150 {expanded ? 'rotate-90' : ''}"
          strokeWidth={1.8}
        />
      </button>

      {#if expanded && expandedToolView}
        {@const { readableOut, genImg, imgFile, writeFile, readFile, toolImg, cu, miniDiff, skillHtml } = expandedToolView}
        <div transition:slide={{ duration: 150 }} class="mt-1 mb-1.5 pl-3 border-l border-white/[0.08] font-mono text-[11px] text-[#8E8E95] space-y-1.5 select-text">
          {#if genImg}
            <div class="rounded-[8px] border border-white/[0.08] bg-[#0E0E11] p-3 space-y-2.5">
              {#if genImg.dataUrl}
                <div class="flex justify-center p-2 rounded-[6px] bg-[#0A0A0C] border border-white/[0.04]">
                  <button
                    type="button"
                    class="cursor-zoom-in rounded-[6px] focus:outline-none"
                    onclick={() => chatStore.openImagePreview(genImg.dataUrl!, genImg.prompt || 'Generated image')}
                  >
                    <img
                      src={genImg.dataUrl}
                      alt={genImg.prompt || 'Generated image'}
                      class="rounded-[6px] border border-white/[0.08] max-h-80 w-auto object-contain shadow-md hover:opacity-95 transition-opacity"
                    />
                  </button>
                </div>
              {/if}
              {#if genImg.prompt}
                <div class="p-2.5 rounded-[6px] bg-[#141417] border border-white/[0.06] text-[12px] text-[#EDEDEF] font-sans italic leading-relaxed">
                  "{genImg.prompt}"
                </div>
              {/if}
              <div class="flex flex-wrap items-center gap-2 text-[10.5px] font-mono">
                {#if genImg.aspectRatio}
                  <span class="px-1.5 py-0.5 rounded-[4px] bg-white/[0.06] text-[#A3A3AA]">
                    {genImg.aspectRatio}
                  </span>
                {/if}
                {#if genImg.model}
                  <span class="px-1.5 py-0.5 rounded-[4px] bg-[#8B5CF6]/15 text-[#DDD6FE]">
                    {genImg.model}
                  </span>
                {/if}
                {#if genImg.bytes}
                  <span class="text-[#71717A]">
                    {(genImg.bytes / 1024).toFixed(0)} KB
                  </span>
                {/if}
                {#if genImg.path}
                  <span class="text-[#A3A3AA] truncate max-w-xs" title={genImg.path}>
                    {genImg.path}
                  </span>
                {/if}
              </div>
            </div>
          {:else if imgFile}
            <div class="rounded-[8px] border border-white/[0.08] bg-[#0E0E11] p-3 space-y-2.5">
              {#if imgFile.dataUrl}
                <div class="flex justify-center p-2 rounded-[6px] bg-[#0A0A0C] border border-white/[0.04]">
                  <button
                    type="button"
                    class="cursor-zoom-in rounded-[6px] focus:outline-none"
                    onclick={() => chatStore.openImagePreview(imgFile.dataUrl!, imgFile.path)}
                  >
                    <img
                      src={imgFile.dataUrl}
                      alt={imgFile.path}
                      class="rounded-[6px] border border-white/[0.08] max-h-80 w-auto object-contain shadow-md hover:opacity-95 transition-opacity"
                    />
                  </button>
                </div>
              {/if}
              <div class="flex items-center justify-between text-[11px] font-mono text-[#A3A3AA]">
                <span class="truncate">{imgFile.path}</span>
                {#if imgFile.bytes}
                  <span class="shrink-0 text-[#71717A]">{(imgFile.bytes / 1024).toFixed(0)} KB</span>
                {/if}
              </div>
            </div>
          {:else if writeFile && writeFile.content}
            <div class="rounded-[8px] border border-white/[0.08] bg-[#0E0E11] overflow-hidden">
              <div class="px-3 py-1.5 border-b border-white/[0.06] flex items-center justify-between text-[11px]">
                <div class="flex items-center gap-2 truncate">
                  <FileToolIcon name={writeFile.filePath} class="w-3.5 h-3.5" />
                  <span class="text-[#EDEDEF] font-mono truncate">{writeFile.filePath}</span>
                </div>
                <span class="px-1.5 py-0.5 rounded-[4px] bg-white/[0.06] text-[#A3A3AA] font-mono text-[10px] shrink-0">
                  {writeFile.linesCount} lines
                </span>
              </div>
              <div class="p-2 max-h-60 overflow-y-auto font-mono text-[11px] leading-relaxed bg-[#0A0A0C]">
                {#each writeFile.content.slice(0, 10000).split('\n') as line, li}
                  <div class="flex gap-3 px-1 hover:bg-white/[0.02]">
                    <span class="w-6 text-right text-[#52525B] select-none shrink-0">{li + 1}</span>
                    <span class="text-[#D4D4D8] whitespace-pre-wrap break-all">{line || ' '}</span>
                  </div>
                {/each}
              </div>
            </div>
          {:else if readFile && readFile.content}
            <div class="rounded-[8px] border border-white/[0.08] bg-[#0E0E11] overflow-hidden">
              <div class="px-3 py-1.5 border-b border-white/[0.06] flex items-center justify-between text-[11px]">
                <div class="flex items-center gap-2 truncate">
                  <FileToolIcon name={readFile.filePath} class="w-3.5 h-3.5" />
                  <span class="text-[#EDEDEF] font-mono truncate">{readFile.filePath}</span>
                </div>
                {#if readFile.totalLines}
                  <span class="px-1.5 py-0.5 rounded-[4px] bg-white/[0.06] text-[#A3A3AA] font-mono text-[10px] shrink-0">
                    {readFile.startLine && readFile.endLine ? `L${readFile.startLine}-${readFile.endLine} of ${readFile.totalLines}` : `${readFile.totalLines} lines`}
                  </span>
                {/if}
              </div>
              <div class="p-2 max-h-60 overflow-y-auto font-mono text-[11px] leading-relaxed bg-[#0A0A0C]">
                <pre class="text-[#C4C4CA] whitespace-pre-wrap break-all">{readFile.content}</pre>
              </div>
            </div>
          {:else if toolImg}
            <button
              type="button"
              class="cursor-zoom-in rounded-[6px] focus:outline-none block"
              onclick={() => chatStore.openImagePreview(toolImg, 'Captured preview')}
            >
              <img
                src={toolImg}
                alt="Captured preview"
                class="rounded-[6px] border border-white/[0.08] max-h-72 w-auto object-contain bg-[#0B0B0D] hover:opacity-95 transition-opacity"
              />
            </button>
          {/if}
          {#if call.terminalData}
            <div class="text-[#D4D4D8] whitespace-pre-wrap break-all">$ {call.terminalData.command}</div>
            {#if call.terminalData.stdout}
              <pre class="text-[#9BA1B0] whitespace-pre-wrap break-all max-h-52 overflow-y-auto">{call.terminalData.stdout}</pre>
            {/if}
            {#if call.terminalData.stderr}
              <pre class="text-[#F87171] whitespace-pre-wrap break-all max-h-40 overflow-y-auto">{call.terminalData.stderr}</pre>
            {/if}
          {:else if miniDiff.length > 0}
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
          {:else if skillHtml}
            <div class="markdown-prose text-[12px] text-[#C4C4CA] max-h-64 overflow-y-auto pr-1">{@html skillHtml}</div>
          {:else if cu}
            {#if cu.isListWindows}
              <div class="rounded-[6px] border border-white/[0.08] bg-[#0E0E11] overflow-hidden">
                <div class="px-3 py-1.5 border-b border-white/[0.06] flex items-center justify-between text-[11px]">
                  <span class="font-medium text-[#EDEDEF]">Доступные окна ({cu.windows?.length || 0})</span>
                  {#if cu.boundHwnd}
                    <span class="text-[#A78BFA] font-mono text-[10px]">Текущее окно: HWND {cu.boundHwnd}</span>
                  {/if}
                </div>
                {#if cu.windows && cu.windows.length > 0}
                  <div class="max-h-56 overflow-y-auto divide-y divide-white/[0.04]">
                    {#each cu.windows as win}
                      <div class="px-3 py-1.5 flex items-center justify-between gap-2 hover:bg-white/[0.02] transition-colors">
                        <div class="flex items-center gap-2 min-w-0">
                          <span class="text-[#EDEDEF] text-[11.5px] truncate font-sans">{win.title || win.processName}</span>
                          <span class="text-[#71717A] text-[10px] font-mono shrink-0">{win.processName}</span>
                          {#if win.isBound || (cu.boundHwnd && win.hwnd === cu.boundHwnd)}
                            <span class="px-1.5 py-0.2 rounded-[3px] border border-[#8B5CF6]/50 bg-[#8B5CF6]/15 font-mono text-[9px] text-[#DDD6FE] shrink-0">
                              bound
                            </span>
                          {/if}
                          {#if win.isOffscreen}
                            <span class="px-1.5 py-0.2 rounded-[3px] border border-white/[0.1] bg-white/[0.05] font-mono text-[9px] text-[#A3A3AA] shrink-0">
                              off-screen
                            </span>
                          {/if}
                          {#if win.isMinimized}
                            <span class="px-1.5 py-0.2 rounded-[3px] border border-white/[0.08] bg-white/[0.03] font-mono text-[9px] text-[#71717A] shrink-0">
                              minimized
                            </span>
                          {/if}
                        </div>
                        <div class="flex items-center gap-2 shrink-0 font-mono text-[10px] text-[#71717A]">
                          {#if win.bounds}
                            <span>{win.bounds.width}×{win.bounds.height}</span>
                          {/if}
                          {#if win.isOffscreen}
                            <button
                              type="button"
                              class="px-1.5 py-0.5 rounded-[3px] bg-white/[0.08] hover:bg-white/[0.14] text-[#EDEDEF] text-[10px] transition-colors disabled:opacity-50"
                              disabled={restoringHwnds[win.hwnd]}
                              onclick={() => onRestoreWindow(win.hwnd)}
                            >
                              {restoringHwnds[win.hwnd] ? '...' : restoredHwnds[win.hwnd] ? '✓ На экране' : 'На экран'}
                            </button>
                          {/if}
                        </div>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <div class="p-3 text-[11px] text-[#71717A] italic">Окна не найдены</div>
                {/if}
              </div>
            {:else if cu.isBindWindow}
              <div class="rounded-[6px] border border-white/[0.08] bg-[#0E0E11] p-3 space-y-2">
                <div class="flex items-center justify-between gap-2">
                  <div class="min-w-0">
                    <div class="text-[12.5px] font-medium text-[#EDEDEF] truncate font-sans">
                      {cu.boundInfo?.title || cu.boundInfo?.processName || 'Целевое окно'}
                    </div>
                    <div class="flex items-center gap-2 text-[10.5px] text-[#71717A] font-mono mt-0.5">
                      {#if cu.boundInfo?.processName}<span>{cu.boundInfo.processName}</span>{/if}
                      {#if cu.boundInfo?.pid}<span>PID: {cu.boundInfo.pid}</span>{/if}
                      {#if cu.boundInfo?.hwnd}<span>HWND: {cu.boundInfo.hwnd}</span>{/if}
                      {#if cu.boundInfo?.bounds}<span>{cu.boundInfo.bounds.width}×{cu.boundInfo.bounds.height}</span>{/if}
                    </div>
                  </div>
                  <div class="flex items-center gap-1.5 shrink-0">
                    <span class="px-1.5 py-0.5 rounded-[4px] border border-[#8B5CF6]/50 bg-[#8B5CF6]/15 font-mono text-[10px] text-[#DDD6FE]">
                      Привязано
                    </span>
                    {#if cu.boundInfo?.isOffscreen}
                      <span class="px-1.5 py-0.5 rounded-[4px] border border-white/[0.1] bg-white/[0.05] font-mono text-[10px] text-[#A3A3AA]">
                        В фоне
                      </span>
                      <button
                        type="button"
                        class="px-2 py-0.5 rounded-[4px] bg-white/[0.08] hover:bg-white/[0.14] text-[10.5px] text-[#EDEDEF] font-sans transition-colors disabled:opacity-50"
                        disabled={restoringHwnds[cu.boundInfo?.hwnd || 0]}
                        onclick={() => onRestoreWindow(cu.boundInfo?.hwnd || 0)}
                      >
                        {restoringHwnds[cu.boundInfo?.hwnd || 0] ? '...' : restoredHwnds[cu.boundInfo?.hwnd || 0] ? '✓ На экране' : 'Вернуть на экран'}
                      </button>
                    {/if}
                    {#if cu.boundInfo?.isMinimized}
                      <span class="px-1.5 py-0.5 rounded-[4px] border border-white/[0.08] bg-white/[0.03] font-mono text-[10px] text-[#71717A]">
                        Свернуто
                      </span>
                    {/if}
                  </div>
                </div>
              </div>
            {:else if cu.isSnapshotUi}
              <div class="rounded-[6px] border border-white/[0.08] bg-[#0E0E11] overflow-hidden">
                <div class="px-3 py-1.5 border-b border-white/[0.06] flex items-center justify-between text-[11px]">
                  <span class="font-medium text-[#EDEDEF]">UI Элементы окна ({cu.elements?.length || 0})</span>
                  <span class="text-[#71717A] text-[10px] font-mono">UIAutomation</span>
                </div>
                {#if cu.elements && cu.elements.length > 0}
                  <div class="max-h-56 overflow-y-auto divide-y divide-white/[0.04]">
                    {#each cu.elements as el}
                      <div class="px-3 py-1 flex items-center justify-between gap-2 text-[11px] font-mono">
                        <div class="flex items-center gap-2 min-w-0">
                          <span class="text-[#A78BFA] font-bold shrink-0">{el.id}</span>
                          <span class="px-1.5 py-0.2 rounded-[3px] bg-white/[0.05] text-[#D4D4D8] text-[10px] shrink-0 font-sans">
                            {el.controlType}
                          </span>
                          {#if el.name}
                            <span class="text-[#EDEDEF] truncate font-sans">"{el.name}"</span>
                          {/if}
                          {#if el.automationId}
                            <span class="text-[#71717A] text-[10px] truncate">id: {el.automationId}</span>
                          {/if}
                        </div>
                        {#if el.enabled === false}
                          <span class="text-[#F87171] text-[9.5px] shrink-0">disabled</span>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {:else}
                  <div class="p-3 text-[11px] text-[#71717A] italic">Элементы управления не найдены</div>
                {/if}
              </div>
            {:else if cu.isMultiActions}
              <div class="rounded-[6px] border border-white/[0.08] bg-[#0E0E11] p-2.5 flex flex-col gap-2 text-[11.5px]">
                <div class="flex items-center justify-between gap-2 border-b border-white/[0.06] pb-1.5">
                  <span class="text-[#A855F7] font-medium font-sans flex items-center gap-1.5">
                    <span class="w-1.5 h-1.5 rounded-full bg-[#A855F7]"></span>
                    Batch: {cu.multiInfo?.count ?? 0} actions
                  </span>
                  {#if cu.multiInfo?.delayMs !== undefined}
                    <span class="text-[#71717A] font-mono text-[10px]">{cu.multiInfo.delayMs}ms delay</span>
                  {/if}
                </div>
                <div class="flex flex-wrap gap-1.5 pt-0.5">
                  {#each (cu.multiInfo?.steps || []) as stepItem, sIdx}
                    <span class="px-2 py-0.5 rounded-[4px] bg-white/[0.05] border border-white/[0.06] font-mono text-[10.5px] text-[#EDEDEF] flex items-center gap-1">
                      <span class="text-[#71717A] text-[9.5px]">#{sIdx + 1}</span>
                      <span class="text-[#C084FC]">{stepItem.action || stepItem.action_name || 'step'}</span>
                      {#if stepItem.text}
                        <span class="text-[#A3A3AA] truncate max-w-[80px]">"{stepItem.text}"</span>
                      {:else if stepItem.keys}
                        <span class="text-[#F59E0B]">[{stepItem.keys}]</span>
                      {/if}
                    </span>
                  {/each}
                </div>
              </div>
            {:else if cu.isInput}
              <div class="rounded-[6px] border border-white/[0.08] bg-[#0E0E11] p-2.5 flex items-center justify-between gap-3 text-[11.5px]">
                <div class="flex items-center gap-2 min-w-0">
                  <span class="px-1.5 py-0.5 rounded-[4px] bg-white/[0.06] text-[#EDEDEF] font-mono text-[10.5px]">
                    {cu.inputInfo?.action}
                  </span>
                  {#if cu.inputInfo?.text}
                    <span class="text-[#EDEDEF] truncate font-sans">Текст: <span class="text-white font-mono">"{cu.inputInfo.text}"</span></span>
                  {:else if cu.inputInfo?.keys}
                    <span class="text-[#EDEDEF] font-sans">Клавиши: <span class="text-[#A78BFA] font-mono font-bold">{cu.inputInfo.keys}</span></span>
                  {:else if cu.inputInfo?.coords}
                    <span class="text-[#A3A3AA] font-mono text-[11px]">
                      ({cu.inputInfo.coords.win_x ?? cu.inputInfo.coords.norm_x}, {cu.inputInfo.coords.win_y ?? cu.inputInfo.coords.norm_y})
                    </span>
                  {/if}
                  {#if cu.inputInfo?.element}
                    <span class="text-[#71717A] text-[10.5px]">({cu.inputInfo.element})</span>
                  {/if}
                </div>
                <span class="px-1.5 py-0.5 rounded-[3px] bg-[#22C55E]/10 text-[#4ADE80] font-mono text-[10px] shrink-0">
                  {cu.inputInfo?.status || 'ok'}
                </span>
              </div>
            {:else if cu.isLaunchApp}
              <div class="rounded-[6px] border border-white/[0.08] bg-[#0E0E11] p-2.5 flex items-center justify-between gap-3 text-[11.5px]">
                <div class="flex items-center gap-2 min-w-0">
                  <span class="text-[#EDEDEF] font-medium font-sans truncate">{cu.launchInfo?.executable}</span>
                  {#if cu.launchInfo?.pid}<span class="text-[#71717A] font-mono text-[10.5px]">PID: {cu.launchInfo.pid}</span>{/if}
                  {#if cu.launchInfo?.offscreen}
                    <span class="px-1.5 py-0.2 rounded-[3px] bg-white/[0.05] text-[#A3A3AA] font-mono text-[9.5px]">off-screen</span>
                  {/if}
                </div>
                <span class="px-1.5 py-0.5 rounded-[3px] bg-[#22C55E]/10 text-[#4ADE80] font-mono text-[10px] shrink-0">
                  {cu.launchInfo?.status || 'запущен'}
                </span>
              </div>
            {:else if cu.isRestoreWindow}
              <div class="rounded-[6px] border border-white/[0.08] bg-[#0E0E11] p-2.5 flex items-center justify-between gap-3 text-[11.5px]">
                <div class="flex items-center gap-2 min-w-0">
                  <span class="text-[#4ADE80] font-medium">✓ Окно возвращено на рабочий экран</span>
                  {#if cu.boundInfo?.title}<span class="text-[#EDEDEF] font-sans truncate">"{cu.boundInfo.title}"</span>{/if}
                </div>
              </div>
            {:else if cu.isCloseWindow}
              <div class="rounded-[6px] border border-white/[0.08] bg-[#0E0E11] p-2.5 flex items-center justify-between gap-3 text-[11.5px]">
                <div class="flex items-center gap-2 min-w-0">
                  <span class="text-[#EDEDEF] font-sans">Окно закрыто:</span>
                  <span class="text-[#A3A3AA] font-mono truncate">{cu.closeInfo?.title || cu.closeInfo?.processName || 'окно'}</span>
                </div>
              </div>
            {:else if readableOut}
              <pre data-omni-tool-output="" class="text-[#9BA1B0] whitespace-pre-wrap break-all max-h-52 overflow-y-auto">{readableOut}</pre>
            {/if}
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
</div>

<style>
  .timeline-step-row {
    content-visibility: auto;
    contain-intrinsic-size: auto 24px;
    contain: layout style;
  }
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
</style>

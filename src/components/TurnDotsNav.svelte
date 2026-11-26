<script lang="ts">
import { Spring } from 'svelte/motion';
import type { ChatMessage } from '../lib/stores/chat.svelte';
import { SPRING_POPUP } from '../lib/motion';

let {
  messages = [],
  activeIndex = 0,
  onSelectTurn = () => {}
}: {
  messages?: ChatMessage[];
  activeIndex?: number;
  onSelectTurn?: (messageId: string) => void;
} = $props();

interface TurnItem {
  id: string;
  prompt: string;
  replyPreview: string;
  createdAt: number;
}

let turns = $derived.by<TurnItem[]>(() => {
  const list: TurnItem[] = [];
  for (let i = 0; i < messages.length; i++) {
    const m = messages[i];
    if (m.role !== 'user') continue;
    let reply = '';
    for (let j = i + 1; j < messages.length; j++) {
      if (messages[j].role === 'user') break;
      if (messages[j].role === 'assistant') {
        reply = messages[j].content || messages[j].error || '';
        break;
      }
    }
    list.push({
      id: m.id,
      prompt: m.content || 'Attachment',
      replyPreview: reply,
      createdAt: m.createdAt
    });
  }
  return list;
});

const MAX_VISIBLE = 16;
let windowStart = $state(0);
let hoveredIdx = $state<number | null>(null);
let navContainerEl = $state<HTMLDivElement | null>(null);

const cardSpring = new Spring({ opacity: 0, scale: 0.96, x: -6 }, SPRING_POPUP);

$effect(() => {
  const len = turns.length;
  if (len <= MAX_VISIBLE) {
    windowStart = 0;
    return;
  }
  const maxStart = len - MAX_VISIBLE;
  if (activeIndex < windowStart) {
    windowStart = Math.max(0, activeIndex);
  } else if (activeIndex >= windowStart + MAX_VISIBLE) {
    windowStart = Math.min(maxStart, activeIndex - MAX_VISIBLE + 1);
  } else {
    windowStart = Math.max(0, Math.min(windowStart, maxStart));
  }
});

function handleWheel(e: WheelEvent) {
  if (turns.length <= MAX_VISIBLE) return;
  const maxStart = turns.length - MAX_VISIBLE;
  const dir = e.deltaY > 0 ? 1 : -1;
  const nextStart = Math.max(0, Math.min(maxStart, windowStart + dir));
  if (nextStart !== windowStart) {
    e.preventDefault();
    e.stopPropagation();
    windowStart = nextStart;
  }
}

let visibleTurns = $derived.by(() => {
  if (turns.length <= MAX_VISIBLE) {
    return turns.map((turn, idx) => ({ turn, idx }));
  }
  const maxStart = Math.max(0, turns.length - MAX_VISIBLE);
  const start = Math.max(0, Math.min(windowStart, maxStart));
  return turns.slice(start, start + MAX_VISIBLE).map((turn, i) => ({
    turn,
    idx: start + i
  }));
});

$effect(() => {
  cardSpring.target = hoveredIdx !== null
    ? { opacity: 1, scale: 1, x: 0 }
    : { opacity: 0, scale: 0.96, x: -6 };
});

const turnDateFormatter = new Intl.DateTimeFormat('ru-RU', {
  day: 'numeric',
  month: 'long',
  year: 'numeric',
  hour: '2-digit',
  minute: '2-digit'
});

function formatTurnDate(ts: number): string {
  try {
    return turnDateFormatter.format(ts);
  } catch {
    return '';
  }
}

let hoveredTurn = $derived(hoveredIdx !== null ? turns[hoveredIdx] || null : null);
</script>

{#if turns.length >= 2}
  <div
    bind:this={navContainerEl}
    class="absolute left-2 top-1/2 -translate-y-1/2 z-30 flex items-center select-none"
    onmouseleave={() => (hoveredIdx = null)}
    role="navigation"
    aria-label="Conversation turns"
  >
    <div
      onwheel={handleWheel}
      class="flex flex-col items-center gap-1.5 py-2 px-1 overflow-hidden select-none"
    >
      {#each visibleTurns as item (item.turn.id)}
        {@const isActive = item.idx === activeIndex}
        {@const isHovered = item.idx === hoveredIdx}
        <button
          type="button"
          class="w-3.5 h-3.5 flex items-center justify-center group/dot focus:outline-none"
          onmouseenter={() => (hoveredIdx = item.idx)}
          onclick={() => onSelectTurn(item.turn.id)}
          aria-label="Jump to message {item.idx + 1}"
        >
          <span
            class="rounded-full transition-all duration-200 {isActive
              ? 'w-2 h-2 bg-white shadow-[0_0_6px_rgba(255,255,255,0.45)]'
              : isHovered
              ? 'w-2 h-2 bg-white/70 scale-110'
              : 'w-1.5 h-1.5 bg-white/[0.18] group-hover/dot:bg-white/50'}"
          ></span>
        </button>
      {/each}
    </div>

    {#if (hoveredTurn !== null) || cardSpring.current.opacity > 0.02}
      {#if hoveredTurn}
        <div
          class="ml-2 w-[310px] rounded-[12px] bg-[#161618]/95 backdrop-blur-md border border-white/[0.09] p-3.5 shadow-2xl pointer-events-none"
          style="opacity: {cardSpring.current.opacity}; transform: translateX({cardSpring.current.x}px) scale({cardSpring.current.scale});"
        >
          <div class="text-[13px] font-semibold text-[#EDEDEF] leading-snug line-clamp-2">
            {hoveredTurn.prompt}
          </div>
          {#if hoveredTurn.replyPreview}
            <div class="mt-1.5 text-[12px] text-[#8E8E95] leading-relaxed line-clamp-3">
              {hoveredTurn.replyPreview}
            </div>
          {/if}
          <div class="mt-2.5 pt-2 border-t border-white/[0.06] flex items-center justify-between text-[11px] font-mono text-[#73737A]">
            <span>{(hoveredIdx ?? 0) + 1} of {turns.length}</span>
            <span>{formatTurnDate(hoveredTurn.createdAt)}</span>
          </div>
        </div>
      {/if}
    {/if}
  </div>
{/if}

<script lang="ts">
import { slide, scale, fly } from 'svelte/transition';
import { flip } from 'svelte/animate';
import { Spring } from 'svelte/motion';
import Icon from './Icon.svelte';
import { chatStore } from '../lib/stores/chat.svelte';
import { SPRING_POPUP } from '../lib/motion';
import { omniBridge } from '../lib/runtime/omniBridge';

let collapsed = $state<boolean>(false);
let topSlotEl: HTMLDivElement | undefined = $state();
let bottomSlotEl: HTMLDivElement | undefined = $state();

$effect(() => {
  if (topSlotEl) {
    const unreg = omniBridge.registerSlot('taskboard-top', topSlotEl);
    return unreg;
  }
});

$effect(() => {
  if (bottomSlotEl) {
    const unreg = omniBridge.registerSlot('taskboard-bottom', bottomSlotEl);
    return unreg;
  }
});

let todos = $derived(chatStore.activeTodos);
let hasTodos = $derived(todos.length > 0);
let doneCount = $derived(todos.filter((t) => t.status === 'completed').length);

const widgetSpring = new Spring({ opacity: 0, scale: 0.94, y: -10, x: 10 }, SPRING_POPUP);

$effect(() => {
  widgetSpring.target = hasTodos
    ? { opacity: 1, scale: 1, y: 0, x: 0 }
    : { opacity: 0, scale: 0.94, y: -10, x: 10 };
});
</script>

{#if hasTodos || widgetSpring.current.opacity > 0.02}
  <div
    data-omni-taskboard=""
    class="absolute top-2 right-5 z-30 w-[280px] select-none {hasTodos ? 'pointer-events-auto' : 'pointer-events-none'}"
    style="opacity: {widgetSpring.current.opacity}; transform: translate3d({widgetSpring.current.x}px, {widgetSpring.current.y}px, 0) scale({widgetSpring.current.scale});"
  >
    <div class="rounded-[10px] bg-[#161618]/92 backdrop-blur-[10px] border border-white/[0.08] shadow-xl overflow-hidden">
      <div bind:this={topSlotEl} data-omni-slot="taskboard-top"></div>
      <button
        type="button"
        data-omni-taskboard-header=""
        class="w-full px-3.5 py-2.5 flex items-center justify-between text-left hover:bg-white/[0.03] transition-colors"
        onclick={() => (collapsed = !collapsed)}
      >
        <div class="flex items-center gap-2 min-w-0">
          {#if doneCount < todos.length && chatStore.isStreaming}
            <svg class="w-3.5 h-3.5 text-[#EDEDEF] animate-spin shrink-0" viewBox="0 0 24 24" fill="none">
              <circle class="opacity-20" cx="12" cy="12" r="9" stroke="currentColor" stroke-width="2.2"></circle>
              <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"></path>
            </svg>
          {:else if todos.length > 0 && doneCount === todos.length}
            <span in:scale={{ start: 0.5, duration: 180 }} class="w-3.5 h-3.5 rounded-full bg-[#6EE7B7]/20 text-[#6EE7B7] flex items-center justify-center shrink-0">
              <Icon name="check" class="w-2.5 h-2.5" strokeWidth={2.5} />
            </span>
          {:else}
            <span class="w-2 h-2 rounded-full bg-[#A3A3AA] shrink-0"></span>
          {/if}
          <span class="text-xs font-medium text-[#EDEDEF] truncate">Tasks</span>
        </div>

        <div class="flex items-center gap-1.5 shrink-0">
          <span class="font-mono text-[10.5px] text-[#73737A]">{doneCount}/{todos.length}</span>
          <Icon
            name="chevron-down"
            class="w-3.5 h-3.5 text-[#66666D] transition-transform duration-150 {collapsed ? '' : 'rotate-180'}"
            strokeWidth={1.7}
          />
        </div>
      </button>

      {#if !collapsed}
        <div transition:slide={{ duration: 160 }} class="px-3 pb-3 pt-0.5 space-y-1.5 max-h-64 overflow-y-auto border-t border-white/[0.05]">
          {#each todos as todo (todo.id)}
            <div
              animate:flip={{ duration: 180 }}
              in:fly={{ x: 8, duration: 160 }}
              data-omni-task-card=""
              data-task-status={todo.status}
              class="flex items-start gap-2.5 pt-1 text-[12px] leading-snug"
            >
              <div data-omni-task-status="" class="w-4 h-4 mt-0.5 flex items-center justify-center shrink-0">
                {#if todo.status === 'completed'}
                  <span
                    in:scale={{ start: 0.4, duration: 180 }}
                    class="w-3.5 h-3.5 rounded-full bg-[#6EE7B7]/18 text-[#6EE7B7] flex items-center justify-center"
                  >
                    <Icon name="check" class="w-2.5 h-2.5" strokeWidth={2.5} />
                  </span>
                {:else if todo.status === 'in_progress'}
                  <svg class="w-3.5 h-3.5 text-[#EDEDEF] animate-spin" viewBox="0 0 24 24" fill="none">
                    <circle class="opacity-20" cx="12" cy="12" r="9" stroke="currentColor" stroke-width="2.2"></circle>
                    <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" stroke-width="2.2" stroke-linecap="round"></path>
                  </svg>
                {:else if todo.status === 'failed'}
                  <span
                    in:scale={{ start: 0.5, duration: 160 }}
                    class="w-3.5 h-3.5 rounded-full bg-[#F87171]/18 text-[#F87171] flex items-center justify-center"
                  >
                    <Icon name="close" class="w-2.5 h-2.5" strokeWidth={2.4} />
                  </span>
                {:else}
                  <span class="w-3 h-3 rounded-full border border-white/[0.18]"></span>
                {/if}
              </div>

              <span
                data-omni-task-title=""
                class="min-w-0 flex-1 break-words {todo.status === 'completed'
                  ? 'text-[#66666D] line-through transition-colors'
                  : todo.status === 'in_progress'
                  ? 'font-medium shimmer-task'
                  : todo.status === 'failed'
                  ? 'text-[#F87171] transition-colors'
                  : 'text-[#9BA1B0] transition-colors'}"
              >
                {todo.text}
              </span>
            </div>
          {/each}
        </div>
      {/if}
      <div bind:this={bottomSlotEl} data-omni-slot="taskboard-bottom"></div>
    </div>
  </div>
{/if}

<style>
  .shimmer-task {
    background: linear-gradient(
      90deg,
      rgba(212, 212, 216, 0.78) 0%,
      rgba(212, 212, 216, 0.78) 40%,
      rgba(255, 255, 255, 1) 50%,
      rgba(212, 212, 216, 0.78) 60%,
      rgba(212, 212, 216, 0.78) 100%
    );
    background-size: 300% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
    will-change: background-position;
    animation: task-shimmer 2s linear infinite;
  }

  @keyframes task-shimmer {
    0% {
      background-position: 100% 0;
    }
    100% {
      background-position: 0% 0;
    }
  }
</style>

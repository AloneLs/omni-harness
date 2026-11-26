<script lang="ts">
import { tick } from 'svelte';
import { fly } from 'svelte/transition';
import { cubicOut } from 'svelte/easing';
import { Spring } from 'svelte/motion';
import Icon from './Icon.svelte';
import { chatStore } from '../lib/stores/chat.svelte';
import MessageItem from './MessageItem.svelte';
import Composer from './Composer.svelte';
import TaskBoardWidget from './TaskBoardWidget.svelte';
import TurnDotsNav from './TurnDotsNav.svelte';
import { SPRING_POPUP, SPRING_PANEL, attachPressScale } from '../lib/motion';

let { onToggleContracts }: {
  onToggleContracts?: () => void;
} = $props();

let scrollContainerEl = $state<HTMLDivElement | null>(null);
let showScrollDown = $state<boolean>(false);
let userHasScrolledUp = $state<boolean>(false);
let activeTurnIndex = $state<number>(0);

const scrollBtnSpring = new Spring({ opacity: 0, y: 8, scale: 0.9 }, SPRING_POPUP);
const emptyDockSpring = new Spring(chatStore.messages.length === 0 ? 1 : 0, SPRING_PANEL);

let isEmpty = $derived(chatStore.messages.length === 0);
let lastAssistantMsgId = $derived.by(() => {
  const list = chatStore.messages;
  for (let i = list.length - 1; i >= 0; i--) {
    if (list[i].role === 'assistant') return list[i].id;
    if (list[i].role === 'user') return null;
  }
  return null;
});

$effect(() => {
  if (!isEmpty && emptyDockSpring.current > 0.01 && !chatStore.isStreaming) {
    emptyDockSpring.set(0, { instant: true });
  } else {
    emptyDockSpring.target = isEmpty ? 1 : 0;
  }
});

$effect(() => {
  scrollBtnSpring.target = showScrollDown && !isEmpty
    ? { opacity: 1, y: 0, scale: 1 }
    : { opacity: 0, y: 8, scale: 0.9 };
});

let userTurnCount = $derived.by(() => {
  let c = 0;
  for (const m of chatStore.messages) {
    if (m.role === 'user') c++;
  }
  return c;
});

$effect(() => {
  const root = scrollContainerEl;
  const _chatId = chatStore.activeChatId;
  const _count = userTurnCount;
  if (!root || _count === 0 || typeof IntersectionObserver === 'undefined') return;

  let observer: IntersectionObserver | null = null;
  let cancelled = false;
  const visibleIndices = new Set<number>();

  void tick().then(() => {
    if (cancelled || !scrollContainerEl) return;
    const els = Array.from(scrollContainerEl.querySelectorAll('[data-user-turn-id]'));
    if (els.length === 0) return;
    const indexByEl = new Map<Element, number>();
    els.forEach((el, idx) => indexByEl.set(el, idx));

    observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const idx = indexByEl.get(entry.target);
          if (idx === undefined) continue;
          if (entry.isIntersecting) visibleIndices.add(idx);
          else visibleIndices.delete(idx);
        }
        if (visibleIndices.size > 0) {
          let maxIdx = 0;
          for (const idx of visibleIndices) {
            if (idx > maxIdx) maxIdx = idx;
          }
          activeTurnIndex = maxIdx;
        }
      },
      { root: scrollContainerEl, threshold: 0.05 }
    );
    for (const el of els) observer.observe(el);
  });

  return () => {
    cancelled = true;
    if (observer) observer.disconnect();
  };
});

function scrollToTurn(msgId: string) {
  if (!scrollContainerEl) return;
  const el = scrollContainerEl.querySelector(`[data-user-turn-id="${msgId}"]`);
  if (el) {
    el.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
}

let lastObservedChatId: string | null = null;
let wasStreaming = false;
let programmaticScroll = false;
let scrollCheckRaf: number | null = null;
let lastAutoScrollAt = 0;
let autoScrollTimer: ReturnType<typeof setTimeout> | null = null;
const initialMsgIdsByChat = new Set<string>();
let trackedChatForInitial: string | null = null;

$effect(() => {
  const cid = chatStore.activeChatId;
  if (cid !== trackedChatForInitial) {
    trackedChatForInitial = cid;
    initialMsgIdsByChat.clear();
    for (const m of chatStore.messages) {
      initialMsgIdsByChat.add(m.id);
    }
  }
});

function handleUserWheel(e: WheelEvent) {
  if (e.deltaY < 0) {
    userHasScrolledUp = true;
  }
}

function handleUserTouch() {
  userHasScrolledUp = true;
}

function handleScroll() {
  if (!scrollContainerEl || scrollCheckRaf !== null) return;
  scrollCheckRaf = requestAnimationFrame(() => {
    scrollCheckRaf = null;
    if (!scrollContainerEl) return;
    const { scrollTop, scrollHeight, clientHeight } = scrollContainerEl;
    const distFromBottom = scrollHeight - scrollTop - clientHeight;

    if (distFromBottom > 140) {
      showScrollDown = true;
    } else {
      showScrollDown = false;
      if (distFromBottom < 24) {
        userHasScrolledUp = false;
      }
    }
  });
}

let scrollRafId: number | null = null;

function scrollToBottom(smooth = true) {
  if (!scrollContainerEl) return;
  programmaticScroll = true;
  if (smooth) {
    scrollContainerEl.scrollTo({ top: scrollContainerEl.scrollHeight, behavior: 'smooth' });
  } else {
    scrollContainerEl.scrollTop = scrollContainerEl.scrollHeight;
  }
  showScrollDown = false;
  userHasScrolledUp = false;
  setTimeout(() => {
    programmaticScroll = false;
  }, 120);
}

function scrollToLastAssistantStart(behavior: ScrollBehavior = 'smooth') {
  if (!scrollContainerEl) return;
  const asstEls = scrollContainerEl.querySelectorAll('[data-assistant-turn-id]');
  const target = (asstEls.length > 0
    ? asstEls[asstEls.length - 1]
    : scrollContainerEl.querySelector('[data-user-turn-id]:last-of-type')) as HTMLElement | null;
  if (!target) return;
  programmaticScroll = true;
  target.scrollIntoView({ behavior, block: 'start' });
  setTimeout(() => {
    programmaticScroll = false;
  }, 160);
}

$effect(() => {
  const currentChatId = chatStore.activeChatId;
  const _msgCount = chatStore.messages.length;
  const _tokenCount = chatStore.currentStreamTokenCount;
  const streamingNow = chatStore.isStreaming;

  if (currentChatId !== lastObservedChatId) {
    lastObservedChatId = currentChatId;
    userHasScrolledUp = false;
    wasStreaming = streamingNow;
    emptyDockSpring.set(_msgCount === 0 ? 1 : 0, { instant: true });
    if (_msgCount > 0) {
      tick().then(() => {
        requestAnimationFrame(() => {
          scrollToBottom(false);
        });
      });
    }
    return;
  }
  if (!wasStreaming && streamingNow) {
    userHasScrolledUp = false;
  }

  if (wasStreaming && !streamingNow && !userHasScrolledUp && _msgCount > 0) {
    wasStreaming = false;
    tick().then(() => {
      requestAnimationFrame(() => {
        if (!scrollContainerEl || userHasScrolledUp) return;
        const asstEls = scrollContainerEl.querySelectorAll('[data-assistant-turn-id]');
        const lastEl = asstEls.length > 0 ? (asstEls[asstEls.length - 1] as HTMLElement) : null;
        if (lastEl && lastEl.scrollHeight > scrollContainerEl.clientHeight * 0.82) {
          scrollToLastAssistantStart('auto');
        } else {
          scrollToBottom(false);
        }
      });
    });
    return;
  }
  wasStreaming = streamingNow;

  if (!userHasScrolledUp && _msgCount > 0) {
    const now = Date.now();
    const elapsed = now - lastAutoScrollAt;
    if (elapsed >= 60) {
      if (scrollRafId === null) {
        scrollRafId = requestAnimationFrame(() => {
          scrollRafId = null;
          lastAutoScrollAt = Date.now();
          scrollToBottom(false);
        });
      }
    } else if (!autoScrollTimer) {
      autoScrollTimer = setTimeout(() => {
        autoScrollTimer = null;
        if (!userHasScrolledUp && scrollRafId === null) {
          scrollRafId = requestAnimationFrame(() => {
            scrollRafId = null;
            lastAutoScrollAt = Date.now();
            scrollToBottom(false);
          });
        }
      }, 60 - elapsed);
    }
  }
});
</script>

<div data-omni-chat-view="" class="flex-1 flex flex-col h-full relative overflow-hidden select-none">
  <TaskBoardWidget />
  <TurnDotsNav messages={chatStore.messages} activeIndex={activeTurnIndex} onSelectTurn={scrollToTurn} />
  <div
    data-omni-messages=""
    bind:this={scrollContainerEl}
    onscroll={handleScroll}
    onwheel={handleUserWheel}
    ontouchmove={handleUserTouch}
    role="presentation"
    class="flex-1 min-h-0 overflow-y-auto pl-9 pr-6 pt-2 pb-4"
  >
    {#if !isEmpty}
      {#key chatStore.activeChatId}
        <div
          in:fly={{ y: 10, duration: 200, easing: cubicOut }}
          class="max-w-[720px] mx-auto space-y-2"
        >
          {#each chatStore.messages as message, idx (message.id)}
            <div
              class="chat-message-item"
              data-user-turn-id={message.role === 'user' ? message.id : undefined}
              data-assistant-turn-id={message.role === 'assistant' ? message.id : undefined}
            >
            <MessageItem
              {message}
              animateEntrance={!initialMsgIdsByChat.has(message.id)}
              isLatestAssistant={message.id === lastAssistantMsgId}
              isLastUserUnanswered={idx === chatStore.messages.length - 1 && message.role === 'user' && !chatStore.isStreaming}
              onEditUser={(id: string, newText: string, rollback: boolean) => chatStore.editUserMessage(id, newText, rollback)}
              onRegenerate={(id: string) => chatStore.regenerateAssistantMessage(id)}
              onContinue={(id: string) => chatStore.continueAssistantMessage(id)}
            />
            </div>
          {/each}
        </div>
      {/key}
    {/if}
  </div>

  <div class="shrink-0 px-6 pb-5 pt-2 relative">
    {#if (showScrollDown && !isEmpty) || scrollBtnSpring.current.opacity > 0.02}
      <div
        class="absolute bottom-full mb-3 left-1/2 -translate-x-1/2 z-40 {showScrollDown && !isEmpty ? 'pointer-events-auto' : 'pointer-events-none'}"
        style="opacity: {scrollBtnSpring.current.opacity}; transform: translateY({scrollBtnSpring.current.y}px) scale({scrollBtnSpring.current.scale});"
      >
        <button
          type="button"
          class="w-8 h-8 rounded-full bg-[#1C1C1F] hover:bg-[#27272A] border border-white/[0.1] text-[#E5E5E8] flex items-center justify-center shadow-lg transition-colors"
          onclick={() => scrollToBottom(true)}
          title="Scroll to bottom"
          use:attachPressScale
        >
          <Icon name="arrow-down" class="w-4 h-4 text-[#D4D4D8]" strokeWidth={1.8} />
        </button>
      </div>
    {/if}

    <div class="w-full max-w-[680px] mx-auto">
      <Composer />
    </div>
  </div>

  {#if emptyDockSpring.current > 0.005}
    <div class="shrink-0 pointer-events-none" style="height: {emptyDockSpring.current * 36}vh;"></div>
  {/if}
</div>

<style>
  .chat-message-item {
    content-visibility: auto;
    contain-intrinsic-size: auto 120px;
    contain: layout style;
  }
</style>

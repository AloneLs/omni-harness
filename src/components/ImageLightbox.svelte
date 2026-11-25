<script lang="ts">
import { fade, scale } from 'svelte/transition';
import { cubicOut } from 'svelte/easing';
import Icon from './Icon.svelte';
import { chatStore } from '../lib/stores/chat.svelte';

function handleKeydown(e: KeyboardEvent) {
  if (chatStore.imagePreview && e.key === 'Escape') {
    e.preventDefault();
    e.stopPropagation();
    chatStore.closeImagePreview();
  }
}
</script>

<svelte:window onkeydown={handleKeydown} />

{#if chatStore.imagePreview}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="fixed inset-0 z-[1000] flex items-center justify-center bg-black/85 backdrop-blur-sm p-4 select-none"
    transition:fade={{ duration: 140 }}
    onclick={() => chatStore.closeImagePreview()}
    role="dialog"
    aria-modal="true"
    tabindex="-1"
  >
    <button
      type="button"
      class="fixed top-4 right-4 z-10 w-9 h-9 rounded-full bg-[#1C1C21]/90 hover:bg-[#27272D] border border-white/[0.12] flex items-center justify-center text-[#D4D4D8] hover:text-white transition-colors"
      onclick={(e) => {
        e.stopPropagation();
        chatStore.closeImagePreview();
      }}
      title="Close (Esc)"
    >
      <Icon name="close" class="w-4 h-4" strokeWidth={1.8} />
    </button>

    <div
      class="relative flex flex-col items-center justify-center max-w-[90vw] max-h-[90vh]"
      transition:scale={{ start: 0.96, duration: 160, easing: cubicOut }}
      onclick={(e) => e.stopPropagation()}
      role="presentation"
    >
      <img
        src={chatStore.imagePreview.src}
        alt={chatStore.imagePreview.alt || 'Image preview'}
        class="max-w-[90vw] max-h-[86vh] w-auto h-auto object-contain rounded-[10px] border border-white/[0.1] shadow-2xl bg-[#0B0B0D]"
      />
      {#if chatStore.imagePreview.alt}
        <div class="mt-2 px-3 py-1 rounded-[6px] bg-[#141418]/90 border border-white/[0.08] text-[11.5px] font-sans text-[#C4C4CA] max-w-[80vw] truncate">
          {chatStore.imagePreview.alt}
        </div>
      {/if}
    </div>
  </div>
{/if}

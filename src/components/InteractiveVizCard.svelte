<script lang="ts">
import { attachPressScale, animateSwapIcon } from '../lib/motion';
import { highlightCode, escapeHtml } from '../lib/markdown';

let {
  id = 'viz-1',
  title = 'Interactive Visualization',
  code = '',
  type = 'html'
}: {
  id?: string;
  title?: string;
  code?: string;
  type?: string;
} = $props();

let viewMode = $state<'preview' | 'code'>('preview');
let isFullscreen = $state<boolean>(false);
let refreshKey = $state<number>(0);
let copied = $state<boolean>(false);
let toggleBtnEl = $state<HTMLButtonElement | null>(null);

let srcdoc = $derived.by(() => {
  const dummy = refreshKey;
  return `
    <!DOCTYPE html>
    <html lang="en">
    <head>
      <meta charset="UTF-8">
      <meta name="viewport" content="width=device-width, initial-scale=1.0">
      <style>
        body {
          margin: 0;
          padding: 16px;
          background-color: #0E0F12;
          color: #F0F3F8;
          font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
          font-size: 13px;
          line-height: 1.5;
          overflow: auto;
        }
        * { box-sizing: border-box; }
        ::-webkit-scrollbar { width: 6px; height: 6px; }
        ::-webkit-scrollbar-thumb { background: #2B2E36; border-radius: 9999px; }
      </style>
    </head>
    <body>
      ${code}
    </body>
    </html>
  `;
});

function toggleView() {
  if (toggleBtnEl) {
    animateSwapIcon(toggleBtnEl, () => {
      viewMode = viewMode === 'preview' ? 'code' : 'preview';
    });
  } else {
    viewMode = viewMode === 'preview' ? 'code' : 'preview';
  }
}

function handleRefresh() {
  refreshKey++;
}

function toggleFullscreen() {
  isFullscreen = !isFullscreen;
}

async function copyCode() {
  try {
    await navigator.clipboard.writeText(code);
    copied = true;
    setTimeout(() => {
      copied = false;
    }, 1500);
  } catch {}
}
</script>

<div
  class="rounded-xl border border-[#2B2E36] bg-[#131416] overflow-hidden my-4 shadow-lg transition-all duration-200 {isFullscreen ? 'fixed inset-4 z-[999] flex flex-col bg-[#0E0F12] border-[#3E7BFA]' : 'w-full'}"
>
  <div class="h-10 px-3 bg-[#181A20] border-b border-[#2B2E36] flex items-center justify-between select-none">
    <div class="flex items-center gap-2 truncate">
      <div class="w-2 h-2 rounded-full bg-[#00D2D3]"></div>
      <span class="font-semibold text-xs text-[#F0F3F8] truncate">{title}</span>
      <span class="text-[10px] font-mono px-1.5 py-0.5 rounded bg-[#1E222B] text-[#9BA1B0] uppercase">{type}</span>
    </div>

    <div class="flex items-center gap-1.5">
      <button
        bind:this={toggleBtnEl}
        type="button"
        class="px-2 py-1 rounded text-xs text-[#9BA1B0] hover:text-[#F0F3F8] hover:bg-[#222631] border border-[#2B2E36] transition-colors flex items-center gap-1"
        onclick={toggleView}
        title={viewMode === 'preview' ? 'View Source Code' : 'View Interactive Preview'}
        use:attachPressScale
      >
        {#if viewMode === 'preview'}
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="16 18 22 12 16 6"></polyline>
            <polyline points="8 6 2 12 8 18"></polyline>
          </svg>
          <span class="text-[11px]">Code</span>
        {:else}
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"></circle>
            <polygon points="10 8 16 12 10 16 10 8"></polygon>
          </svg>
          <span class="text-[11px]">Preview</span>
        {/if}
      </button>

      <button
        type="button"
        class="p-1.5 rounded text-[#9BA1B0] hover:text-[#F0F3F8] hover:bg-[#222631] border border-[#2B2E36] transition-colors"
        onclick={handleRefresh}
        title="Reload Sandbox"
        use:attachPressScale
      >
        <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <polyline points="23 4 23 10 17 10"></polyline>
          <path d="M20.49 15a9 9 0 1 1-2.12-9.36L23 10"></path>
        </svg>
      </button>

      <button
        type="button"
        class="p-1.5 rounded text-[#9BA1B0] hover:text-[#F0F3F8] hover:bg-[#222631] border border-[#2B2E36] transition-colors"
        onclick={toggleFullscreen}
        title={isFullscreen ? 'Exit Fullscreen' : 'Fullscreen'}
        use:attachPressScale
      >
        {#if isFullscreen}
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="4 14 10 14 10 20"></polyline>
            <polyline points="20 10 14 10 14 4"></polyline>
            <line x1="14" y1="10" x2="21" y2="3"></line>
            <line x1="3" y1="21" x2="10" y2="14"></line>
          </svg>
        {:else}
          <svg class="w-3.5 h-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <polyline points="15 3 21 3 21 9"></polyline>
            <polyline points="9 21 3 21 3 15"></polyline>
            <line x1="21" y1="3" x2="14" y2="10"></line>
            <line x1="3" y1="21" x2="10" y2="14"></line>
          </svg>
        {/if}
      </button>
    </div>
  </div>

  <div class="relative w-full {isFullscreen ? 'flex-1' : 'h-[360px]'} bg-[#0E0F12]">
    {#if viewMode === 'preview'}
      <iframe
        title={title}
        srcdoc={srcdoc}
        sandbox="allow-scripts allow-modals allow-forms"
        class="w-full h-full border-none bg-[#0E0F12]"
      ></iframe>
    {:else}
      <div class="w-full h-full overflow-auto p-4 bg-[#131416] text-xs font-mono">
        <div class="flex justify-end mb-2">
          <button
            type="button"
            class="px-2 py-1 rounded bg-[#181A20] hover:bg-[#1E222B] border border-[#2B2E36] text-xs text-[#9BA1B0] hover:text-[#F0F3F8] transition-colors flex items-center gap-1.5"
            onclick={copyCode}
          >
            {#if copied}
              <span class="text-[#10B981]">Copied!</span>
            {:else}
              <span>Copy Full Code</span>
            {/if}
          </button>
        </div>
        <pre class="text-[#F0F3F8] whitespace-pre-wrap break-all"><code>{@html highlightCode(code, type)}</code></pre>
      </div>
    {/if}
  </div>
</div>

<script lang="ts">
import { getContext } from 'svelte';
import { themeStore } from '../lib/stores/theme.svelte';

export type IconName =
  | 'plus-circle'
  | 'search'
  | 'filter'
  | 'folder'
  | 'settings'
  | 'sidebar'
  | 'minimize'
  | 'maximize'
  | 'close'
  | 'plus'
  | 'enter'
  | 'send'
  | 'archive'
  | 'chevron-down'
  | 'chevron-right'
  | 'pin'
  | 'edit'
  | 'trash'
  | 'arrow-down'
  | 'user'
  | 'terminal'
  | 'copy'
  | 'check'
  | 'mic';

let {
  name,
  class: className = 'w-4 h-4',
  filled = false,
  strokeWidth = 1.6,
  previewOnly = false
}: {
  name: IconName;
  class?: string;
  filled?: boolean;
  strokeWidth?: number;
  previewOnly?: boolean;
} = $props();

const inPreviewContext = getContext<boolean>('omni-preview-only') || false;
let isPreview = $derived(previewOnly || inPreviewContext);
let override = $derived(themeStore.resolveIconOverride(name, isPreview));
let isRawSvg = $derived(Boolean(override && override.trim().startsWith('<svg')));
let isHtmlTag = $derived(Boolean(override && /^<[a-z]+/i.test(override.trim()) && !isRawSvg));
let isImageUrl = $derived(
  Boolean(
    override &&
      (override.startsWith('data:image/') ||
        override.startsWith('http://') ||
        override.startsWith('https://') ||
        override.startsWith('blob:') ||
        override.startsWith('asset://') ||
        /\.(png|jpg|jpeg|webp|gif|svg|ico)$/i.test(override))
  )
);
</script>

{#if isRawSvg || isHtmlTag}
  <span class="inline-flex items-center justify-center shrink-0 {className}" data-omni-icon={name}>{@html override}</span>
{:else if isImageUrl && override}
  <img src={override} alt={name} class="inline-block object-contain shrink-0 {className}" data-omni-icon={name} />
{:else}
<svg
  data-omni-icon={name}
  class={className}
  viewBox="0 0 24 24"
  fill={filled ? 'currentColor' : 'none'}
  stroke="currentColor"
  stroke-width={strokeWidth}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {#if override}
    <path d={override}></path>
  {:else if name === 'plus-circle'}
    <circle cx="12" cy="12" r="9"></circle>
    <line x1="12" y1="8.5" x2="12" y2="15.5"></line>
    <line x1="8.5" y1="12" x2="15.5" y2="12"></line>
  {:else if name === 'search'}
    <circle cx="11" cy="11" r="7.5"></circle>
    <line x1="20.5" y1="20.5" x2="16.4" y2="16.4"></line>
  {:else if name === 'filter'}
    <line x1="4" y1="7" x2="20" y2="7"></line>
    <line x1="7" y1="12" x2="17" y2="12"></line>
    <line x1="10" y1="17" x2="14" y2="17"></line>
  {:else if name === 'folder'}
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
  {:else if name === 'settings'}
    <circle cx="12" cy="12" r="3"></circle>
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
  {:else if name === 'sidebar'}
    <rect x="3" y="4" width="18" height="16" rx="2"></rect>
    <line x1="9" y1="4" x2="9" y2="20"></line>
  {:else if name === 'minimize'}
    <line x1="5" y1="12" x2="19" y2="12"></line>
  {:else if name === 'maximize'}
    <rect x="5" y="5" width="14" height="14" rx="1.5"></rect>
  {:else if name === 'close'}
    <line x1="18" y1="6" x2="6" y2="18"></line>
    <line x1="6" y1="6" x2="18" y2="18"></line>
  {:else if name === 'plus'}
    <line x1="12" y1="5" x2="12" y2="19"></line>
    <line x1="5" y1="12" x2="19" y2="12"></line>
  {:else if name === 'enter' || name === 'send'}
    <polyline points="9 10 4 15 9 20"></polyline>
    <path d="M20 4v7a4 4 0 0 1-4 4H4"></path>
  {:else if name === 'archive'}
    <polyline points="21 8 21 21 3 21 3 8"></polyline>
    <rect x="1" y="3" width="22" height="5"></rect>
    <line x1="10" y1="12" x2="14" y2="12"></line>
  {:else if name === 'chevron-down'}
    <polyline points="6 9 12 15 18 9"></polyline>
  {:else if name === 'chevron-right'}
    <polyline points="9 18 15 12 9 6"></polyline>
  {:else if name === 'pin'}
    <path d="M12 17v5"></path>
    <path d="M9 10.76a2 2 0 0 1-1.11 1.79l-1.78.9A2 2 0 0 0 5 15.24V16a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1v-.76a2 2 0 0 0-1.11-1.79l-1.78-.9A2 2 0 0 1 15 10.76V7a1 1 0 0 1 1-1 2 2 0 0 0 0-4H8a2 2 0 0 0 0 4 1 1 0 0 1 1 1z"></path>
  {:else if name === 'edit'}
    <path d="M12 20h9"></path>
    <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L7 19l-4 1 1-4L16.5 3.5z"></path>
  {:else if name === 'trash'}
    <polyline points="3 6 5 6 21 6"></polyline>
    <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"></path>
  {:else if name === 'arrow-down'}
    <line x1="12" y1="5" x2="12" y2="19"></line>
    <polyline points="19 12 12 19 5 12"></polyline>
  {:else if name === 'user'}
    <path d="M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2"></path>
    <circle cx="12" cy="7" r="4"></circle>
  {:else if name === 'terminal'}
    <polyline points="4 17 10 11 4 5"></polyline>
    <line x1="12" y1="19" x2="20" y2="19"></line>
  {:else if name === 'copy'}
    <rect x="9" y="9" width="13" height="13" rx="2"></rect>
    <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"></path>
  {:else if name === 'check'}
    <polyline points="20 6 9 17 4 12"></polyline>
  {:else if name === 'mic'}
    <path d="M12 1a3 3 0 0 0-3 3v8a3 3 0 0 0 6 0V4a3 3 0 0 0-3-3z"></path>
    <path d="M19 10v2a7 7 0 0 1-14 0v-2"></path>
    <line x1="12" y1="19" x2="12" y2="23"></line>
    <line x1="8" y1="23" x2="16" y2="23"></line>
  {/if}
</svg>
{/if}
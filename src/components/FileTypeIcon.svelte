<script lang="ts">
let {
  filename = '',
  kind = 'auto',
  class: className = 'w-3.5 h-3.5'
}: {
  filename?: string;
  kind?: 'auto' | 'terminal' | 'search' | 'globe' | 'desktop' | 'chat' | 'file';
  class?: string;
} = $props();

let resolved = $derived.by(() => {
  if (kind !== 'auto') return kind;
  const ext = filename.split('.').pop()?.toLowerCase() || '';
  if (['js', 'mjs', 'cjs', 'jsx'].includes(ext)) return 'js';
  if (['ts', 'tsx'].includes(ext)) return 'ts';
  if (ext === 'rs') return 'rs';
  if (ext === 'svelte') return 'svelte';
  if (ext === 'py') return 'py';
  if (ext === 'json') return 'json';
  if (['css', 'scss', 'sass'].includes(ext)) return 'css';
  if (['html', 'htm'].includes(ext)) return 'html';
  if (['md', 'mdx'].includes(ext)) return 'md';
  if (['toml', 'yaml', 'yml', 'ini', 'env'].includes(ext)) return 'config';
  return 'file';
});
</script>

{#if resolved === 'terminal'}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#6EE7B7" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <polyline points="4 17 10 11 4 5"></polyline>
    <line x1="12" y1="19" x2="20" y2="19"></line>
  </svg>
{:else if resolved === 'js'}
  <svg class={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <polygon points="12 2 21 7 21 17 12 22 3 17 3 7" stroke="#68A063" stroke-width="2" fill="#68A063" fill-opacity="0.16" stroke-linejoin="round"></polygon>
    <text x="12" y="15.5" text-anchor="middle" font-size="8.5" font-weight="700" font-family="monospace" fill="#8CC84B">JS</text>
  </svg>
{:else if resolved === 'ts'}
  <svg class={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <rect x="2.5" y="2.5" width="19" height="19" rx="3.5" fill="#3178C6" fill-opacity="0.22" stroke="#3178C6" stroke-width="1.8"></rect>
    <text x="12" y="15.5" text-anchor="middle" font-size="8.5" font-weight="700" font-family="monospace" fill="#60A5FA">TS</text>
  </svg>
{:else if resolved === 'rs'}
  <svg class={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <circle cx="12" cy="12" r="8.5" fill="#F97316" fill-opacity="0.16" stroke="#F97316" stroke-width="1.8" stroke-dasharray="3 1.5"></circle>
    <text x="12" y="15" text-anchor="middle" font-size="8.5" font-weight="700" font-family="monospace" fill="#FB923C">RS</text>
  </svg>
{:else if resolved === 'svelte'}
  <svg class={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <path d="M17.5 6.5c-1.5-2.3-4.6-2.9-6.9-1.4L7.2 7.3C5.3 8.5 4.5 11 5.3 13.1c.3.7.7 1.3 1.3 1.8-.3.9-.2 2 .4 2.9 1.5 2.3 4.6 2.9 6.9 1.4l3.4-2.2c1.9-1.2 2.7-3.7 1.9-5.8-.3-.7-.7-1.3-1.3-1.8.3-.9.2-2-.4-2.9z" fill="#FF3E00" fill-opacity="0.2" stroke="#FF3E00" stroke-width="1.8"></path>
  </svg>
{:else if resolved === 'py'}
  <svg class={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <rect x="3" y="3" width="18" height="18" rx="4" fill="#3B82F6" fill-opacity="0.16" stroke="#60A5FA" stroke-width="1.7"></rect>
    <text x="12" y="15.2" text-anchor="middle" font-size="8.5" font-weight="700" font-family="monospace" fill="#FACC15">PY</text>
  </svg>
{:else if resolved === 'json'}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#FBBF24" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M8 4H6a2 2 0 0 0-2 2v4a2 2 0 0 1-2 2 2 2 0 0 1 2 2v4a2 2 0 0 0 2 2h2"></path>
    <path d="M16 4h2a2 2 0 0 1 2 2v4a2 2 0 0 0 2 2 2 2 0 0 0-2 2v4a2 2 0 0 1-2 2h-2"></path>
  </svg>
{:else if resolved === 'css'}
  <svg class={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <rect x="3" y="3" width="18" height="18" rx="3.5" fill="#38BDF8" fill-opacity="0.18" stroke="#38BDF8" stroke-width="1.7"></rect>
    <text x="12" y="15" text-anchor="middle" font-size="7.5" font-weight="700" font-family="monospace" fill="#7DD3FC">CSS</text>
  </svg>
{:else if resolved === 'html'}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#FB923C" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <polyline points="8 7 3 12 8 17"></polyline>
    <polyline points="16 7 21 12 16 17"></polyline>
    <line x1="14" y1="5" x2="10" y2="19"></line>
  </svg>
{:else if resolved === 'md'}
  <svg class={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
    <rect x="2.5" y="4.5" width="19" height="15" rx="2.5" fill="#A1A1AA" fill-opacity="0.12" stroke="#A1A1AA" stroke-width="1.6"></rect>
    <text x="12" y="15" text-anchor="middle" font-size="8.5" font-weight="700" font-family="monospace" fill="#E4E4E7">MD</text>
  </svg>
{:else if resolved === 'config'}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#C084FC" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <circle cx="12" cy="12" r="3"></circle>
    <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
  </svg>
{:else if resolved === 'search'}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#93C5FD" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <circle cx="11" cy="11" r="7"></circle>
    <line x1="20" y1="20" x2="16" y2="16"></line>
  </svg>
{:else if resolved === 'globe'}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#67E8F9" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <circle cx="12" cy="12" r="9"></circle>
    <line x1="3" y1="12" x2="21" y2="12"></line>
    <path d="M12 3a14 14 0 0 1 4 9 14 14 0 0 1-4 9 14 14 0 0 1-4-9 14 14 0 0 1 4-9z"></path>
  </svg>
{:else if resolved === 'desktop'}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#C4B5FD" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <rect x="2" y="3" width="20" height="14" rx="2"></rect>
    <line x1="8" y1="21" x2="16" y2="21"></line>
    <line x1="12" y1="17" x2="12" y2="21"></line>
  </svg>
{:else if resolved === 'chat'}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#A7F3D0" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"></path>
  </svg>
{:else}
  <svg class={className} viewBox="0 0 24 24" fill="none" stroke="#9CA3AF" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"></path>
    <polyline points="14 2 14 8 20 8"></polyline>
  </svg>
{/if}

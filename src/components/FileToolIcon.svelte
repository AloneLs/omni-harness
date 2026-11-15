<script lang="ts">
let {
  name = '',
  toolName = '',
  class: className = 'w-3.5 h-3.5'
}: {
  name?: string;
  toolName?: string;
  class?: string;
} = $props();

let kind = $derived.by(() => {
  const n = (name || '').toLowerCase();
  const t = (toolName || '').toLowerCase();
  if (n.endsWith('.ts') || n.endsWith('.tsx')) return 'ts';
  if (n.endsWith('.js') || n.endsWith('.jsx') || n.endsWith('.mjs') || n.endsWith('.cjs')) return 'js';
  if (n.endsWith('.svelte')) return 'svelte';
  if (n.endsWith('.rs')) return 'rs';
  if (n.endsWith('.py')) return 'py';
  if (n.endsWith('.json') || n.endsWith('.toml') || n.endsWith('.yaml') || n.endsWith('.yml')) return 'json';
  if (n.endsWith('.css') || n.endsWith('.scss')) return 'css';
  if (n.endsWith('.html') || n.endsWith('.htm')) return 'html';
  if (n.endsWith('.md') || n.endsWith('.txt')) return 'md';
  if (t.includes('terminal') || t.includes('pty') || t.includes('run_command')) return 'terminal';
  if (t.includes('web_research') || t.includes('browser') || t.includes('inspect_preview')) return 'web';
  if (t.includes('grep') || t.includes('glob') || t.includes('search')) return 'search';
  if (t.includes('cross_chat')) return 'chat';
  if (t.includes('computer_use') || t.includes('uia')) return 'desktop';
  if (t.includes('task_board')) return 'board';
  if (t.includes('read') || t.includes('edit') || t.includes('write') || t.includes('fs_ops')) return 'file';
  return 'tool';
});
</script>

{#if kind === 'js'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <polygon points="12,2 21,7 21,17 12,22 3,17 3,7" stroke="#83CD29" stroke-width="1.8" fill="#83CD29" fill-opacity="0.14" />
    <text x="12" y="15.2" text-anchor="middle" font-size="8.5" font-weight="700" font-family="monospace" fill="#83CD29">JS</text>
  </svg>
{:else if kind === 'ts'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="3" y="3" width="18" height="18" rx="3.5" stroke="#3178C6" stroke-width="1.7" fill="#3178C6" fill-opacity="0.16" />
    <text x="12" y="15.5" text-anchor="middle" font-size="8.5" font-weight="700" font-family="monospace" fill="#60A5FA">TS</text>
  </svg>
{:else if kind === 'svelte'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M17.5 6.5c-1.8-2.4-5.2-2.9-7.6-1.1L6.6 7.9c-2 1.5-2.6 4.2-1.4 6.4.4.7 1 1.3 1.7 1.7-.3.9-.2 2 .4 2.9 1.3 1.9 3.9 2.3 5.8 1l3.3-2.5c2-1.5 2.6-4.2 1.4-6.4-.4-.7-1-1.3-1.7-1.7.3-.9.2-2-.4-2.8z" stroke="#FF3E00" stroke-width="1.7" fill="#FF3E00" fill-opacity="0.15" />
  </svg>
{:else if kind === 'rs'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <circle cx="12" cy="12" r="8.5" stroke="#DEA584" stroke-width="1.7" stroke-dasharray="3 2" fill="#DEA584" fill-opacity="0.12" />
    <text x="12" y="15" text-anchor="middle" font-size="8" font-weight="700" font-family="monospace" fill="#DEA584">RS</text>
  </svg>
{:else if kind === 'py'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="4" y="4" width="16" height="16" rx="4" stroke="#3776AB" stroke-width="1.7" fill="#3776AB" fill-opacity="0.14" />
    <text x="12" y="15" text-anchor="middle" font-size="8" font-weight="700" font-family="monospace" fill="#FFD43B">PY</text>
  </svg>
{:else if kind === 'json'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#FBBF24" stroke-width="1.8" stroke-linecap="round">
    <path d="M8 4c-2 0-2 2-2 4v2c0 1.2-1 2-2 2 1 0 2 .8 2 2v2c0 2 0 4 2 4" />
    <path d="M16 4c2 0 2 2 2 4v2c0 1.2 1 2 2 2-1 0-2 .8-2 2v2c0 2 0 4-2 4" />
  </svg>
{:else if kind === 'css'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="3" y="3" width="18" height="18" rx="3.5" stroke="#A855F7" stroke-width="1.7" fill="#A855F7" fill-opacity="0.14" />
    <text x="12" y="15" text-anchor="middle" font-size="7.5" font-weight="700" font-family="monospace" fill="#C084FC">CSS</text>
  </svg>
{:else if kind === 'html'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#F97316" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <polyline points="8 7 3 12 8 17" />
    <polyline points="16 7 21 12 16 17" />
    <line x1="14" y1="5" x2="10" y2="19" />
  </svg>
{:else if kind === 'md'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#94A3B8" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
    <line x1="8" y1="13" x2="16" y2="13" />
    <line x1="8" y1="17" x2="13" y2="17" />
  </svg>
{:else if kind === 'terminal'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" fill="#D4D4D8" fill-opacity="0.1" />
    <polyline points="7 15 11 11 7 7" />
    <line x1="13" y1="15" x2="17" y2="15" />
  </svg>
{:else if kind === 'web'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.7">
    <circle cx="12" cy="12" r="9" fill="#D4D4D8" fill-opacity="0.1" />
    <line x1="3" y1="12" x2="21" y2="12" />
    <path d="M12 3a14 14 0 0 1 3.5 9 14 14 0 0 1-3.5 9 14 14 0 0 1-3.5-9A14 14 0 0 1 12 3z" />
  </svg>
{:else if kind === 'search'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.8" stroke-linecap="round">
    <circle cx="11" cy="11" r="7" />
    <line x1="20" y1="20" x2="16.2" y2="16.2" />
  </svg>
{:else if kind === 'chat'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
  </svg>
{:else if kind === 'desktop'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <rect x="2" y="3" width="20" height="14" rx="2" />
    <line x1="8" y1="21" x2="16" y2="21" />
    <line x1="12" y1="17" x2="12" y2="21" />
  </svg>
{:else if kind === 'board'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <polyline points="9 11 12 14 22 4" />
    <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
  </svg>
{:else}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
  </svg>
{/if}

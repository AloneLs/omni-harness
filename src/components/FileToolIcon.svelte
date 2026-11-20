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
  if (t.includes('tool_hub') || t.includes('skill')) return 'skill';
  if (n.endsWith('.cpp') || n.endsWith('.cc') || n.endsWith('.cxx') || n.endsWith('.hpp') || n.endsWith('.hh') || n.endsWith('.hxx')) return 'cpp';
  if (n.endsWith('.c') || n.endsWith('.h')) return 'c';
  if (n.endsWith('.cs')) return 'cs';
  if (n.endsWith('.rs')) return 'rs';
  if (n.endsWith('.go')) return 'go';
  if (n.endsWith('.java')) return 'java';
  if (n.endsWith('.kt') || n.endsWith('.kts')) return 'kt';
  if (n.endsWith('.swift')) return 'swift';
  if (n.endsWith('.rb')) return 'rb';
  if (n.endsWith('.php')) return 'php';
  if (n.endsWith('.lua')) return 'lua';
  if (n.endsWith('.sql')) return 'sql';
  if (n.endsWith('.ps1') || n.endsWith('.sh') || n.endsWith('.bash') || n.endsWith('.zsh') || n.endsWith('.bat') || n.endsWith('.cmd')) return 'shell';
  if (n.endsWith('.ts') || n.endsWith('.tsx')) return 'ts';
  if (n.endsWith('.js') || n.endsWith('.jsx') || n.endsWith('.mjs') || n.endsWith('.cjs')) return 'js';
  if (n.endsWith('.svelte')) return 'svelte';
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

{#if kind === 'cpp'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M12 2L20.5 6.9V17.1L12 22L3.5 17.1V6.9L12 2Z" fill="#00599C" fill-opacity="0.22" stroke="#3B82F6" stroke-width="1.5" />
    <text x="12" y="14.7" text-anchor="middle" font-size="8" font-weight="800" font-family="sans-serif" fill="#60A5FA">C++</text>
  </svg>
{:else if kind === 'c'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M12 2L20.5 6.9V17.1L12 22L3.5 17.1V6.9L12 2Z" fill="#283593" fill-opacity="0.25" stroke="#5C8DBC" stroke-width="1.5" />
    <text x="12" y="15.2" text-anchor="middle" font-size="10.5" font-weight="800" font-family="sans-serif" fill="#93C5FD">C</text>
  </svg>
{:else if kind === 'cs'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M12 2L20.5 6.9V17.1L12 22L3.5 17.1V6.9L12 2Z" fill="#68217A" fill-opacity="0.25" stroke="#A855F7" stroke-width="1.5" />
    <text x="12" y="14.8" text-anchor="middle" font-size="8.5" font-weight="800" font-family="sans-serif" fill="#D8B4FE">C#</text>
  </svg>
{:else if kind === 'rs'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <circle cx="12" cy="12" r="7.5" stroke="#DEA584" stroke-width="1.8" fill="#DEA584" fill-opacity="0.12" />
    <path d="M12 2.2v2.2M12 19.6v2.2M2.2 12h2.2M19.6 12h2.2M5.1 5.1l1.6 1.6M17.3 17.3l1.6 1.6M18.9 5.1l-1.6 1.6M6.7 17.3l-1.6 1.6" stroke="#DEA584" stroke-width="1.8" stroke-linecap="round" />
    <text x="12" y="15.1" text-anchor="middle" font-size="9.5" font-weight="800" font-family="serif" fill="#F3C6A5">R</text>
  </svg>
{:else if kind === 'go'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="2.5" y="5" width="19" height="14" rx="4" fill="#00ADD8" fill-opacity="0.18" stroke="#00ADD8" stroke-width="1.5" />
    <text x="12" y="15" text-anchor="middle" font-size="9" font-weight="800" font-family="sans-serif" fill="#22D3EE">GO</text>
  </svg>
{:else if kind === 'java'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke-linecap="round" stroke-linejoin="round">
    <path d="M10 4c0 1.2-1.5 1.8-1.5 3M14 3.5c0 1.3-1.5 2-1.5 3.3" stroke="#F89820" stroke-width="1.6" />
    <path d="M6 10h10v4a4 4 0 0 1-4 4h-2a4 4 0 0 1-4-4v-4z" stroke="#5382A1" stroke-width="1.7" fill="#5382A1" fill-opacity="0.18" />
    <path d="M16 11h1.8a1.7 1.7 0 0 1 0 3.4H16" stroke="#5382A1" stroke-width="1.6" />
    <path d="M5 20.5h12" stroke="#F89820" stroke-width="1.7" />
  </svg>
{:else if kind === 'kt'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <polygon points="4,4 20,4 12,12 20,20 4,20" fill="#7F52FF" fill-opacity="0.22" stroke="#A78BFA" stroke-width="1.6" stroke-linejoin="round" />
  </svg>
{:else if kind === 'swift'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="3" y="3" width="18" height="18" rx="4.5" fill="#F05138" fill-opacity="0.2" stroke="#F05138" stroke-width="1.5" />
    <path d="M7.5 15.5c3.5 1.8 7 .5 9-3-1-2.5-3.2-4.5-6-5.5 1.5 1.8 2 3.5 1.2 5-1.5-.8-3-2-4.2-3.5 1 2.2 2.2 3.8 3.6 4.8-1.3.4-2.5.3-3.6-.2z" fill="#FB923C" />
  </svg>
{:else if kind === 'rb'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <polygon points="7,4 17,4 21,9 12,20 3,9" fill="#CC342D" fill-opacity="0.22" stroke="#F87171" stroke-width="1.6" stroke-linejoin="round" />
    <line x1="3" y1="9" x2="21" y2="9" stroke="#F87171" stroke-width="1.2" />
  </svg>
{:else if kind === 'php'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <ellipse cx="12" cy="12" rx="10" ry="6.5" fill="#777BB4" fill-opacity="0.22" stroke="#818CF8" stroke-width="1.5" />
    <text x="12" y="14.5" text-anchor="middle" font-size="7" font-weight="800" font-family="sans-serif" fill="#A5B4FC">PHP</text>
  </svg>
{:else if kind === 'lua'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <circle cx="12" cy="12" r="8" fill="#000080" fill-opacity="0.25" stroke="#60A5FA" stroke-width="1.6" />
    <circle cx="15" cy="9" r="2" fill="#93C5FD" />
  </svg>
{:else if kind === 'sql'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#38BDF8" stroke-width="1.6">
    <ellipse cx="12" cy="6" rx="7" ry="2.8" fill="#38BDF8" fill-opacity="0.16" />
    <path d="M5 6v12c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8V6" />
    <path d="M5 12c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8" />
  </svg>
{:else if kind === 'shell'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#4ADE80" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="3" fill="#22C55E" fill-opacity="0.12" />
    <polyline points="7 15 11 11 7 7" />
    <line x1="12.5" y1="15" x2="17" y2="15" />
  </svg>
{:else if kind === 'js'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="3" y="3" width="18" height="18" rx="3.5" stroke="#F7DF1E" stroke-width="1.6" fill="#F7DF1E" fill-opacity="0.16" />
    <text x="12" y="15.5" text-anchor="middle" font-size="8.5" font-weight="800" font-family="sans-serif" fill="#FDE047">JS</text>
  </svg>
{:else if kind === 'ts'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="3" y="3" width="18" height="18" rx="3.5" stroke="#3178C6" stroke-width="1.6" fill="#3178C6" fill-opacity="0.2" />
    <text x="12" y="15.5" text-anchor="middle" font-size="8.5" font-weight="800" font-family="sans-serif" fill="#60A5FA">TS</text>
  </svg>
{:else if kind === 'svelte'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M17.5 6.5c-1.8-2.4-5.2-2.9-7.6-1.1L6.6 7.9c-2 1.5-2.6 4.2-1.4 6.4.4.7 1 1.3 1.7 1.7-.3.9-.2 2 .4 2.9 1.3 1.9 3.9 2.3 5.8 1l3.3-2.5c2-1.5 2.6-4.2 1.4-6.4-.4-.7-1-1.3-1.7-1.7.3-.9.2-2-.4-2.8z" stroke="#FF3E00" stroke-width="1.7" fill="#FF3E00" fill-opacity="0.18" />
  </svg>
{:else if kind === 'py'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M12 3c-3.3 0-3 1.4-3 3v2h6v1H7.5C5.6 9 4 10.6 4 13s1.6 4 3.5 4H9v-2c0-1.7 1.3-3 3-3h4c1.4 0 2.5-1.1 2.5-2.5V6c0-1.7-1.3-3-3-3H12z" fill="#3776AB" fill-opacity="0.28" stroke="#60A5FA" stroke-width="1.3" />
    <path d="M12 21c3.3 0 3-1.4 3-3v-2H9v-1h7.5c1.9 0 3.5-1.6 3.5-4s-1.6-4-3.5-4H15v2c0 1.7-1.3 3-3 3H8c-1.4 0-2.5 1.1-2.5 2.5V18c0 1.7 1.3 3 3 3h3.5z" fill="#FFD43B" fill-opacity="0.22" stroke="#FACC15" stroke-width="1.3" />
  </svg>
{:else if kind === 'json'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#FBBF24" stroke-width="1.8" stroke-linecap="round">
    <path d="M8 4c-2 0-2 2-2 4v2c0 1.2-1 2-2 2 1 0 2 .8 2 2v2c0 2 0 4 2 4" />
    <path d="M16 4c2 0 2 2 2 4v2c0 1.2 1 2 2 2-1 0-2 .8-2 2v2c0 2 0 4-2 4" />
  </svg>
{:else if kind === 'css'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="3" y="3" width="18" height="18" rx="3.5" stroke="#A855F7" stroke-width="1.6" fill="#A855F7" fill-opacity="0.16" />
    <text x="12" y="15" text-anchor="middle" font-size="7.2" font-weight="800" font-family="sans-serif" fill="#C084FC">CSS</text>
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
{:else if kind === 'skill'}
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#C084FC" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
    <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" fill="#A855F7" fill-opacity="0.14" />
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

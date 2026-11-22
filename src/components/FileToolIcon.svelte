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

  if (t === 'multi_actions') return 'multi_actions';
  if (t === 'generate_image') return 'generate_image';
  if (t.includes('tool_hub') || t.includes('skill')) return 'skill';
  if (t === 'tg_send_file') return 'telegram';
  if (t === 'memory_tool') return 'memory';
  if (t === 'project_summary') return 'summary';
  if (t === 'artifact_read') return 'artifact';
  if (t === 'wait') return 'wait';
  if (t === 'subagent_builder' || t === 'agent') return 'agent';
  if (t === 'code_intel') return 'code_intel';

  if (
    n.endsWith('.png') ||
    n.endsWith('.jpg') ||
    n.endsWith('.jpeg') ||
    n.endsWith('.webp') ||
    n.endsWith('.gif') ||
    n.endsWith('.bmp') ||
    n.endsWith('.ico') ||
    n.endsWith('.svg')
  ) {
    return 'image_file';
  }

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
  if (n.endsWith('.json')) return 'json';
  if (n.endsWith('.toml') || n.endsWith('.yaml') || n.endsWith('.yml')) return 'yaml';
  if (n.endsWith('.css') || n.endsWith('.scss')) return 'css';
  if (n.endsWith('.html') || n.endsWith('.htm')) return 'html';
  if (n.endsWith('.md') || n.endsWith('.txt')) return 'md';

  if (t.includes('terminal') || t.includes('pty') || t.includes('run_command')) return 'terminal';
  if (t.includes('web_research') || t.includes('browser') || t.includes('inspect_preview')) return 'web';
  if (t.includes('grep') || t.includes('glob') || t.includes('search')) return 'search';
  if (t.includes('cross_chat')) return 'chat';
  if (t.includes('computer_use') || t.includes('uia')) return 'desktop';
  if (t.includes('task_board') || t.includes('manage_task')) return 'board';
  if (t.includes('write')) return 'write';
  if (t.includes('edit') || t.includes('patch')) return 'edit';
  if (t.includes('read')) return 'read';
  if (t.includes('fs_ops')) return 'fs';
  return 'file';
});
</script>

{#if kind === 'multi_actions'}
  <!-- Multi-Actions: Sequential fast forward steps -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M4 6h16M4 12h10M4 18h7" stroke="#A855F7" stroke-width="2" stroke-linecap="round" />
    <path d="M15 13l4 3-4 3" stroke="#C084FC" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
    <circle cx="20" cy="16" r="1.5" fill="#E9D5FF" />
  </svg>
{:else if kind === 'generate_image'}
  <!-- Material Icon Theme: Image / Palette generator -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M12 2L14.2 7.8L20 10L14.2 12.2L12 18L9.8 12.2L4 10L9.8 7.8L12 2Z" fill="#C084FC" />
    <path d="M19 16L20.2 19.2L23.4 20.4L20.2 21.6L19 24.8L17.8 21.6L14.6 20.4L17.8 19.2L19 16Z" fill="#F472B6" />
    <path d="M4 17L4.8 19.1L6.9 19.9L4.8 20.7L4 22.8L3.2 20.7L1.1 19.9L3.2 19.1L4 17Z" fill="#60A5FA" />
  </svg>
{:else if kind === 'image_file'}
  <!-- Material Icon Theme: Image -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="3" y="3" width="18" height="18" rx="3.5" fill="#0284C7" fill-opacity="0.15" stroke="#38BDF8" stroke-width="1.6" />
    <circle cx="8.5" cy="8.5" r="1.8" fill="#38BDF8" />
    <path d="M21 15l-5.5-5.5L6.5 18.5" stroke="#38BDF8" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{:else if kind === 'py'}
  <!-- Material Icon Theme: Python -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M11.9 2C8.6 2 6.7 3.4 6.7 6.1v2.1h5.3v.7H4.4C2.3 8.9 1 10.7 1 13.8c0 3.3 1.5 5.1 4.5 5.1h1.5v-2.3c0-2.3 1.9-4.2 4.2-4.2h5.2c1.7 0 3-1.4 3-3V6.1c0-2.7-2.6-4.1-7.5-4.1zm-2.1 2.3c.7 0 1.2.5 1.2 1.2 0 .7-.5 1.2-1.2 1.2-.7 0-1.2-.5-1.2-1.2 0-.7.5-1.2 1.2-1.2z" fill="#387EB8" />
    <path d="M12.1 22c3.3 0 5.2-1.4 5.2-4.1v-2.1H12v-.7h7.6c2.1 0 3.4-1.8 3.4-4.9 0-3.3-1.5-5.1-4.5-5.1H17v2.3c0 2.3-1.9 4.2-4.2 4.2H7.6c-1.7 0-3 1.4-3 3v3.3c0 2.7 2.6 4.1 7.5 4.1zm2.1-2.3c-.7 0-1.2-.5-1.2-1.2 0-.7.5-1.2 1.2-1.2.7 0 1.2.5 1.2 1.2 0 .7-.5 1.2-1.2 1.2z" fill="#FFE873" />
  </svg>
{:else if kind === 'ts'}
  <!-- Material Icon Theme: TypeScript -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect width="24" height="24" rx="4" fill="#3178C6" />
    <path d="M5.5 8.2h7.8v2.2H10.5v8.1H7.8v-8.1H5.5V8.2zm9.1 7.3c.6.5 1.4.8 2.3.8 1 0 1.6-.4 1.6-1 0-.6-.4-.9-1.5-1.3-1.8-.6-2.9-1.3-2.9-2.7 0-1.7 1.4-2.8 3.5-2.8 1.4 0 2.5.4 3.2 1l-1 1.9c-.6-.4-1.3-.7-2.1-.7-.8 0-1.4.4-1.4.9 0 .5.4.8 1.4 1.2 1.9.7 3 1.4 3 2.8 0 1.8-1.4 2.9-3.7 2.9-1.6 0-2.8-.5-3.6-1.2l1.2-1.8z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'js'}
  <!-- Material Icon Theme: JavaScript -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect width="24" height="24" rx="4" fill="#F7DF1E" />
    <path d="M6 18.5l2.2-1.3c.5.8.9 1.4 1.7 1.4.8 0 1.3-.3 1.3-1.5v-7.8h2.7v7.9c0 2.5-1.5 3.6-3.8 3.6-2.1 0-3.3-1-4.1-2.3zm8.5-.2l2.2-1.3c.6.9 1.4 1.6 2.6 1.6 1.1 0 1.8-.5 1.8-1.3 0-.9-.7-1.3-2-1.8-1.8-.8-3-1.8-3-3.6 0-1.8 1.4-3.2 3.6-3.2 1.6 0 2.7.6 3.5 1.9l-2.1 1.4c-.4-.8-1-1.2-1.6-1.2-.7 0-1.2.4-1.2 1 0 .7.5 1 1.7 1.5 2.1.9 3.3 1.8 3.3 3.8 0 2.2-1.7 3.4-4.2 3.4-2.4 0-3.8-1-4.6-2.2z" fill="#000000" />
  </svg>
{:else if kind === 'rs'}
  <!-- Material Icon Theme: Rust -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <circle cx="12" cy="12" r="9.5" fill="#DEA584" fill-opacity="0.2" stroke="#DEA584" stroke-width="1.4" />
    <path d="M12 1.5l1 1.8 2-.6.4 2.1 2 .2-.2 2.1 1.8.9-.8 1.9 1.4 1.5-1.3 1.6.8 1.9-1.8.9.2 2.1-2 .2-.4 2.1-2-.6-1 1.8-1-1.8-2 .6-.4-2.1-2-.2.2-2.1-1.8-.9.8-1.9-1.4-1.5 1.3-1.6-.8-1.9 1.8-.9-.2-2.1 2-.2.4-2.1 2 .6 1-1.8z" fill="#E59966" />
    <circle cx="12" cy="12" r="7.5" fill="#18181B" />
    <path d="M8.5 7.5h4.2c1.8 0 2.9 1 2.9 2.5 0 1.2-.7 2-1.7 2.3l2.2 4.2h-2.3l-1.9-3.8H10.5v3.8H8.5V7.5zm2 3.5h2.1c.7 0 1.1-.4 1.1-.9 0-.6-.4-.9-1.1-.9h-2.1V11z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'go'}
  <!-- Material Icon Theme: Go -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M1.5 10.5h4.5M1 12.5h5.5M2 14.5h4" stroke="#00ADD8" stroke-width="1.3" stroke-linecap="round" />
    <path d="M10.8 11.2c-.3-.5-.7-.8-1.4-.8-1.3 0-2.2 1.1-2.2 2.5 0 1.5.9 2.6 2.3 2.6.9 0 1.5-.4 1.8-.9v-.8H9.7v-1.1h3v2.8h-.8l-.2-.5c-.5.5-1.2.8-2.2.8-2.1 0-3.6-1.6-3.6-3.8 0-2.2 1.6-3.8 3.6-3.8 1.3 0 2.2.5 2.8 1.3l-1.5.7zm7.5 1.1c0 1.4-1 2.5-2.4 2.5-1.4 0-2.4-1.1-2.4-2.5s1-2.5 2.4-2.5c1.4 0 2.4 1.1 2.4 2.5zm-3.6 0c0 .9.5 1.5 1.2 1.5.7 0 1.2-.6 1.2-1.5 0-.8-.5-1.5-1.2-1.5-.7 0-1.2.7-1.2 1.5z" fill="#00ADD8" />
  </svg>
{:else if kind === 'java'}
  <!-- Material Icon Theme: Java -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M10 2.5c1.5 1.8-1 3.2 0 4.8M13.5 2c2 1.8-1 3.2 0 4.8" stroke="#F89820" stroke-width="1.5" stroke-linecap="round" />
    <path d="M6 9.5h9.5c.8 0 1.5.7 1.5 1.5v3.5c0 2.8-2.2 5-5 5H9.5c-2.8 0-5-2.2-5-5V11c0-.8.7-1.5 1.5-1.5z" fill="#5382A1" />
    <path d="M16 11.5h1.8c1.2 0 2.2 1 2.2 2.2s-1 2.2-2.2 2.2H16" stroke="#5382A1" stroke-width="1.6" stroke-linecap="round" />
    <path d="M4 21.5h14" stroke="#F89820" stroke-width="1.8" stroke-linecap="round" />
  </svg>
{:else if kind === 'cpp'}
  <!-- Material Icon Theme: C++ -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M12 2L20.6 7V17L12 22L3.4 17V7L12 2Z" fill="#00599C" stroke="#659AD2" stroke-width="1.2" />
    <path d="M10.2 14.8c-.8.5-1.7.6-2.5.2-1-.5-1.4-1.7-1.4-2.8s.5-2.4 1.5-2.8c.8-.4 1.7-.3 2.4.2l-.7 1.2c-.5-.3-1-.4-1.5-.1-.5.3-.8.9-.8 1.5s.3 1.3.8 1.5c.5.2 1 .1 1.4-.1l.8 1.2zm3.3-2.1h1.1v-1.1h.9v1.1h1.1v.9h-1.1v1.1h-.9v-1.1h-1.1v-.9zm4.2 0h1.1v-1.1h.9v1.1h1.1v.9h-1.1v1.1h-.9v-1.1h-1.1v-.9z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'c'}
  <!-- Material Icon Theme: C -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M12 2L20.6 7V17L12 22L3.4 17V7L12 2Z" fill="#283593" stroke="#5C8DBC" stroke-width="1.2" />
    <path d="M14.5 15.2c-1 .7-2.2.8-3.3.3-1.4-.7-2-2.2-2-3.8s.7-3.1 2-3.8c1.1-.6 2.3-.4 3.2.3l-1 1.5c-.6-.5-1.3-.6-2-.2-.8.4-1.1 1.2-1.1 2.2s.3 1.8 1.1 2.2c.7.4 1.4.3 2-.2l1.1 1.5z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'cs'}
  <!-- Material Icon Theme: C# -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M12 2L20.6 7V17L12 22L3.4 17V7L12 2Z" fill="#68217A" stroke="#A855F7" stroke-width="1.2" />
    <path d="M9.8 14.8c-.8.5-1.7.6-2.5.2-1-.5-1.4-1.7-1.4-2.8s.5-2.4 1.5-2.8c.8-.4 1.7-.3 2.4.2l-.7 1.2c-.5-.3-1-.4-1.5-.1-.5.3-.8.9-.8 1.5s.3 1.3.8 1.5c.5.2 1 .1 1.4-.1l.8 1.2zm3.7-4.5h.9l-.3 1.5h1.2l.3-1.5h.9l-.3 1.5h.8v.8h-.9l-.3 1.4h.9v.8h-1l-.4 1.6h-.9l.4-1.6h-1.2l-.4 1.6h-.9l.4-1.6h-.8v-.8h.9l.3-1.4h-.9v-.8h1l.3-1.5zm.9 2.3l-.3 1.4h1.2l.3-1.4h-1.2z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'kt'}
  <!-- Material Icon Theme: Kotlin -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M22 2H2v20h20L12 12l10-10z" fill="url(#kt-grad)" />
    <defs>
      <linearGradient id="kt-grad" x1="2" y1="2" x2="22" y2="22" gradientUnits="userSpaceOnUse">
        <stop offset="0%" stop-color="#7F52FF" />
        <stop offset="50%" stop-color="#C711E1" />
        <stop offset="100%" stop-color="#F88909" />
      </linearGradient>
    </defs>
  </svg>
{:else if kind === 'swift'}
  <!-- Material Icon Theme: Swift -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect width="24" height="24" rx="4" fill="#F05138" />
    <path d="M6 18c5.8 2.2 11.2-.5 13.5-5.5-1.5-2.8-4.5-5.2-8.5-6.5 2 2.2 2.5 4.5 1.5 6.2-2-1-4-2.5-5.5-4.5 1.2 2.8 2.8 4.8 4.8 6-1.8.5-3.5.3-5-1-.5 1.8-.7 3.5-.8 5.3z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'rb'}
  <!-- Material Icon Theme: Ruby -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <polygon points="6,3 18,3 22,9 12,22 2,9" fill="#CC342D" />
    <polygon points="6,3 12,9 2,9" fill="#E53935" />
    <polygon points="18,3 12,9 22,9" fill="#B71C1C" />
    <polygon points="6,3 12,9 18,3" fill="#EF5350" />
    <polygon points="2,9 12,9 12,22" fill="#D32F2F" />
    <polygon points="22,9 12,9 12,22" fill="#880E4F" />
  </svg>
{:else if kind === 'php'}
  <!-- Material Icon Theme: PHP -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <ellipse cx="12" cy="12" rx="11" ry="7" fill="#777BB4" />
    <path d="M6.2 14.5l.8-4.8h2c1 0 1.6.4 1.4 1.4-.2 1.1-.9 1.6-1.8 1.6H7.3l-.4 1.8H6.2zm1.4-2.6h.9c.5 0 .8-.2.9-.8.1-.5-.1-.7-.6-.7h-.8l-.4 1.5zm4.8 2.6l.8-4.8h1.1l-.3 1.8h.1c.4-.6.9-.9 1.6-.9.9 0 1.3.5 1.1 1.5l-.4 2.4h-1.1l.4-2.2c.1-.5-.1-.8-.6-.8-.5 0-.9.4-1.1 1.2l-.3 1.8h-1.4zm5.5 0l.8-4.8h2c1 0 1.6.4 1.4 1.4-.2 1.1-.9 1.6-1.8 1.6h-1.3l-.4 1.8h-1.1zm1.4-2.6h.9c.5 0 .8-.2.9-.8.1-.5-.1-.7-.6-.7h-.8l-.4 1.5z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'lua'}
  <!-- Material Icon Theme: Lua -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <circle cx="12" cy="12" r="8.5" fill="#000080" />
    <circle cx="16.5" cy="7.5" r="2.2" fill="#0000C8" stroke="#FFFFFF" stroke-width="0.8" />
    <circle cx="8" cy="15" r="1.5" fill="#FFFFFF" />
  </svg>
{:else if kind === 'html'}
  <!-- Material Icon Theme: HTML5 -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <polygon points="3,2 21,2 19.3,19.5 12,22 4.7,19.5" fill="#E34F26" />
    <polygon points="12,3.8 17.7,3.8 16.3,18.2 12,19.8" fill="#EF652A" />
    <path d="M7.8 7h8.4l-.3 3.5h-5.4l.3 3.5H16l-.5 4.8-3.5 1-3.5-1-.2-2.3H6.5l.3 3.8 5.2 1.5 5.2-1.5 1-8.8H6.2L7.8 7z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'css'}
  <!-- Material Icon Theme: CSS3 -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <polygon points="3,2 21,2 19.3,19.5 12,22 4.7,19.5" fill="#1572B6" />
    <polygon points="12,3.8 17.7,3.8 16.3,18.2 12,19.8" fill="#33A9DC" />
    <path d="M7.8 7h8.4l-.3 3.5h-5.4l.3 3.5H16l-.5 4.8-3.5 1-3.5-1-.2-2.3H6.5l.3 3.8 5.2 1.5 5.2-1.5 1-8.8H6.2L7.8 7z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'svelte'}
  <!-- Material Icon Theme: Svelte -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M19.3 5.4c-1.8-2.5-5.3-3.1-7.8-1.3L7 7.2c-2 1.5-2.7 4.3-1.4 6.5.4.7 1 1.3 1.7 1.7-.3 1-.2 2.1.4 3 1.4 2 4.1 2.5 6.1 1.1l4.4-3.1c2-1.5 2.7-4.3 1.4-6.5-.4-.7-1-1.3-1.7-1.7.3-1 .2-2.1-.4-3z" fill="#FF3E00" />
  </svg>
{:else if kind === 'json'}
  <!-- Material Icon Theme: JSON -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#F59E0B" stroke-width="2" stroke-linecap="round">
    <path d="M8 4c-2 0-2 2-2 4v2c0 1.2-1 2-2 2 1 0 2 .8 2 2v2c0 2 0 4 2 4" />
    <path d="M16 4c2 0 2 2 2 4v2c0 1.2 1 2 2 2-1 0-2 .8-2 2v2c0 2 0 4-2 4" />
  </svg>
{:else if kind === 'yaml'}
  <!-- Material Icon Theme: YAML / Config -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M4 4h16v16H4z" rx="3" fill="#DC2626" fill-opacity="0.12" stroke="#EF4444" stroke-width="1.6" />
    <path d="M7 8l3 4.5V17h2v-4.5L15 8h-2.1l-1.9 3.2L9.1 8H7z" fill="#EF4444" />
  </svg>
{:else if kind === 'sql'}
  <!-- Material Icon Theme: SQL / Database -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#38BDF8" stroke-width="1.6">
    <ellipse cx="12" cy="6" rx="7" ry="2.8" fill="#38BDF8" fill-opacity="0.2" />
    <path d="M5 6v12c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8V6" />
    <path d="M5 12c0 1.5 3.1 2.8 7 2.8s7-1.3 7-2.8" />
  </svg>
{:else if kind === 'shell'}
  <!-- Material Icon Theme: Shell -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#4ADE80" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="3" fill="#22C55E" fill-opacity="0.12" />
    <polyline points="7 15 11 11 7 7" />
    <line x1="12.5" y1="15" x2="17" y2="15" />
  </svg>
{:else if kind === 'md'}
  <!-- Material Icon Theme: Markdown -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <rect x="2" y="4" width="20" height="16" rx="3" stroke="#94A3B8" stroke-width="1.6" />
    <path d="M5 16V8l3 3.5L11 8v8M14 12l2.5 3.5L19 12M16.5 8v6" stroke="#94A3B8" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" />
  </svg>
{:else if kind === 'write'}
  <!-- Material Icon Theme: Write File -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" stroke="#60A5FA" stroke-width="1.6" />
    <polyline points="14 2 14 8 20 8" stroke="#60A5FA" stroke-width="1.6" />
    <line x1="16" y1="13" x2="8" y2="13" stroke="#60A5FA" stroke-width="1.6" stroke-linecap="round" />
    <line x1="16" y1="17" x2="8" y2="17" stroke="#60A5FA" stroke-width="1.6" stroke-linecap="round" />
  </svg>
{:else if kind === 'edit'}
  <!-- Material Icon Theme: Edit Pen File -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#A78BFA" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
    <path d="M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7" />
    <path d="M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z" />
  </svg>
{:else if kind === 'read'}
  <!-- Material Icon Theme: Read Document -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#94A3B8" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
    <line x1="12" y1="18" x2="12" y2="12" />
    <polyline points="9 15 12 18 15 15" />
  </svg>
{:else if kind === 'skill'}
  <!-- Material Icon Theme: Extension Plugin / Skill -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#C084FC" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
    <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" fill="#A855F7" fill-opacity="0.14" />
  </svg>
{:else if kind === 'terminal'}
  <!-- Material Icon Theme: Terminal Exec -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <rect x="3" y="4" width="18" height="16" rx="2.5" fill="#D4D4D8" fill-opacity="0.1" />
    <polyline points="7 15 11 11 7 7" />
    <line x1="13" y1="15" x2="17" y2="15" />
  </svg>
{:else if kind === 'web'}
  <!-- Material Icon Theme: Web Research / Browser -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#38BDF8" stroke-width="1.7">
    <circle cx="12" cy="12" r="9" fill="#38BDF8" fill-opacity="0.1" />
    <line x1="3" y1="12" x2="21" y2="12" />
    <path d="M12 3a14 14 0 0 1 3.5 9 14 14 0 0 1-3.5 9 14 14 0 0 1-3.5-9A14 14 0 0 1 12 3z" />
  </svg>
{:else if kind === 'search'}
  <!-- Material Icon Theme: Search / Grep / Glob -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.8" stroke-linecap="round">
    <circle cx="11" cy="11" r="7" />
    <line x1="20" y1="20" x2="16.2" y2="16.2" />
  </svg>
{:else if kind === 'chat'}
  <!-- Material Icon Theme: Discussion / Cross Chat -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" />
  </svg>
{:else if kind === 'desktop'}
  <!-- Material Icon Theme: Desktop Window / Computer Use -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <rect x="2" y="3" width="20" height="14" rx="2" fill="#A855F7" fill-opacity="0.08" />
    <line x1="8" y1="21" x2="16" y2="21" />
    <line x1="12" y1="17" x2="12" y2="21" />
  </svg>
{:else if kind === 'board'}
  <!-- Material Icon Theme: Task Board / Manage Task -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#D4D4D8" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
    <polyline points="9 11 12 14 22 4" />
    <path d="M21 12v7a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11" />
  </svg>
{:else if kind === 'telegram'}
  <!-- Material Icon Theme: Telegram -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none">
    <circle cx="12" cy="12" r="10" fill="#2AABEE" />
    <path d="M6 11.5l11-4.5-2 11-3.5-3-2 1.5v-2.5l5.5-5-7 4.5z" fill="#FFFFFF" />
  </svg>
{:else if kind === 'memory'}
  <!-- Memory Tool: Microchip / Brain -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#38BDF8" stroke-width="1.7" stroke-linecap="round">
    <rect x="5" y="5" width="14" height="14" rx="2" fill="#0284C7" fill-opacity="0.12" />
    <line x1="9" y1="2" x2="9" y2="5" />
    <line x1="15" y1="2" x2="15" y2="5" />
    <line x1="9" y1="19" x2="9" y2="22" />
    <line x1="15" y1="19" x2="15" y2="22" />
    <line x1="2" y1="9" x2="5" y2="9" />
    <line x1="2" y1="15" x2="5" y2="15" />
    <line x1="19" y1="9" x2="22" y2="9" />
    <line x1="19" y1="15" x2="22" y2="15" />
    <circle cx="12" cy="12" r="2.5" fill="#38BDF8" />
  </svg>
{:else if kind === 'summary'}
  <!-- Project Summary: Scroll / Documentation -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#F59E0B" stroke-width="1.7" stroke-linecap="round">
    <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
    <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" fill="#F59E0B" fill-opacity="0.12" />
    <line x1="8" y1="7" x2="16" y2="7" />
    <line x1="8" y1="11" x2="14" y2="11" />
  </svg>
{:else if kind === 'artifact'}
  <!-- Artifact Read: Box / Package -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#E879F9" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M16.5 9.4L7.55 4.24a1.8 1.8 0 0 0-1.8 0L3.1 5.76a1.8 1.8 0 0 0-.9 1.56v9.36a1.8 1.8 0 0 0 .9 1.56l7.65 4.42a1.8 1.8 0 0 0 1.8 0l7.65-4.42a1.8 1.8 0 0 0 .9-1.56V7.32a1.8 1.8 0 0 0-.9-1.56L16.5 9.4z" />
    <polyline points="3.3 6.5 12 11.5 20.7 6.5" />
    <line x1="12" y1="22.5" x2="12" y2="11.5" />
  </svg>
{:else if kind === 'wait'}
  <!-- Wait Tool: Hourglass / Clock -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#A3A3AA" stroke-width="1.7" stroke-linecap="round">
    <circle cx="12" cy="12" r="9" />
    <polyline points="12 7 12 12 15.5 14" />
  </svg>
{:else if kind === 'agent'}
  <!-- Subagent: Autonomous Core -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#C084FC" stroke-width="1.7" stroke-linecap="round">
    <circle cx="12" cy="12" r="8" fill="#A855F7" fill-opacity="0.15" />
    <circle cx="9" cy="10" r="1.5" fill="#C084FC" />
    <circle cx="15" cy="10" r="1.5" fill="#C084FC" />
    <path d="M9 15c1 1 5 1 6 0" />
  </svg>
{:else if kind === 'code_intel'}
  <!-- Code Intel: Symbol Tree -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#38BDF8" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <circle cx="6" cy="6" r="3" />
    <circle cx="6" cy="18" r="3" />
    <circle cx="18" cy="9" r="3" />
    <path d="M6 9v6" />
    <path d="M9 6h4a4 4 0 0 1 4 4v-1" />
  </svg>
{:else if kind === 'fs'}
  <!-- Material Icon Theme: Filesystem -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#F59E0B" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" fill="#F59E0B" fill-opacity="0.14" />
  </svg>
{:else}
  <!-- Material Icon Theme: Default File -->
  <svg class="{className} shrink-0" viewBox="0 0 24 24" fill="none" stroke="#94A3B8" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <polyline points="14 2 14 8 20 8" />
  </svg>
{/if}

<script lang="ts">
import { themeStore } from '../../lib/stores/theme.svelte';

let importEl: HTMLInputElement | undefined = $state();
let statusMsg = $state<string | null>(null);

async function handleImport(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0];
  if (!file) return;
  try {
    const raw = await file.text();
    const t = await themeStore.importThemeJson(raw);
    await themeStore.setActiveTheme(t.id);
    statusMsg = `Imported and applied "${t.name}"`;
  } catch (err: any) {
    statusMsg = err?.message || 'Invalid theme file';
  } finally {
    if (importEl) importEl.value = '';
  }
}

function exportActive() {
  const t = themeStore.activeTheme;
  if (!t) return;
  const blob = new Blob([themeStore.exportThemeJson(t)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `${t.name.toLowerCase().replace(/[^a-z0-9]+/g, '-') || 'theme'}.omnitheme.json`;
  a.click();
  URL.revokeObjectURL(url);
}
</script>

<div class="space-y-5 text-[13px] text-[#E5E5E8]">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h2 class="text-sm font-medium text-[#EDEDEF]">Appearance & CSS Studio</h2>
      <p class="text-xs text-[#73737A] mt-0.5">Customize layout, CSS, fonts, SVG icons, animations, and UI language.</p>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        onclick={() => importEl?.click()}
        class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#D4D4D8]"
      >
        Import
      </button>
      <input bind:this={importEl} type="file" accept=".json,.omnitheme" onchange={handleImport} class="hidden" />
      {#if themeStore.activeTheme}
        <button
          type="button"
          onclick={exportActive}
          class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#D4D4D8]"
        >
          Export
        </button>
      {/if}
      <button
        type="button"
        onclick={() => themeStore.openStudio()}
        class="h-8 px-3.5 rounded-[6px] bg-[#8B5CF6] hover:bg-[#7C3AED] text-xs font-medium text-white transition-colors"
      >
        Open Theme Studio
      </button>
    </div>
  </div>

  {#if statusMsg}
    <div class="px-3 py-2 rounded-[6px] bg-[#121214] border border-white/[0.08] text-xs text-[#A3A3AA]">
      {statusMsg}
    </div>
  {/if}

  <div class="rounded-[6px] bg-[#18181B]/70 border border-white/[0.07] divide-y divide-white/[0.06]">
    <button
      type="button"
      onclick={() => themeStore.setActiveTheme(null)}
      class="w-full px-3.5 py-2.5 flex items-center justify-between text-left hover:bg-white/[0.03] transition-colors"
    >
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">Off (Default OmniHarness)</div>
        <div class="text-[11px] text-[#686870]">Standard built-in dark theme with zero overrides</div>
      </div>
      <span class="px-2 py-0.5 rounded-[4px] font-mono text-[10px] {themeStore.activeThemeId === null ? 'bg-[#8B5CF6]/20 text-[#C4B5FD]' : 'bg-white/[0.05] text-[#73737A]'}">
        {themeStore.activeThemeId === null ? 'ACTIVE' : 'Select'}
      </span>
    </button>

    {#each themeStore.themes as t (t.id)}
      <div class="px-3.5 py-2.5 flex items-center justify-between gap-3 hover:bg-white/[0.03] transition-colors">
        <button
          type="button"
          onclick={() => themeStore.setActiveTheme(t.id)}
          class="flex-1 text-left min-w-0"
        >
          <div class="text-xs font-medium text-[#EDEDEF] truncate">{t.name}</div>
          <div class="text-[11px] text-[#686870] truncate">
            {t.css.length} chars CSS · {Object.keys(t.iconOverrides).length} icons · {Object.keys(t.assets).length} assets
          </div>
        </button>
        <div class="flex items-center gap-1.5 shrink-0">
          <button
            type="button"
            onclick={() => themeStore.openStudio(t.id)}
            class="h-7 px-2.5 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.07] text-[11px] text-[#D4D4D8]"
          >
            Edit
          </button>
          <button
            type="button"
            onclick={() => themeStore.setActiveTheme(t.id)}
            class="h-7 px-2.5 rounded-[5px] text-[11px] font-mono {themeStore.activeThemeId === t.id ? 'bg-[#8B5CF6]/20 text-[#C4B5FD]' : 'bg-white/[0.05] text-[#A3A3AA] hover:text-white'}"
          >
            {themeStore.activeThemeId === t.id ? 'ACTIVE' : 'Apply'}
          </button>
        </div>
      </div>
    {/each}
  </div>

  <div class="text-[11px] text-[#686870]">
    Emergency reset shortcut: <span class="font-mono text-[#A3A3AA]">Ctrl + Shift + R</span> resets active theme to Off.
  </div>
</div>

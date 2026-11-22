<script lang="ts">
import { themeStore, createEmptyTheme } from '../../lib/stores/theme.svelte';
import { omniBridge } from '../../lib/runtime/omniBridge';

let importEl: HTMLInputElement | undefined = $state();
let statusMsg = $state<string | null>(null);

let activeSubTab = $state<'themes' | 'custom_css' | 'custom_js' | 'safe_mode'>('themes');
let rawCssValue = $state<string>(localStorage.getItem('omni_raw_css') || '');
let customJsValue = $state<string>(localStorage.getItem('omni_custom_js') || '');

const rawCssPlaceholder = '/* [data-omni-taskboard] { background: rgba(10, 10, 12, 0.95); border-radius: 12px; } */';
const customJsPlaceholder = `// Example:
// omni.ui.mountWidget('taskboard-top', (container, omni) => {
//   container.innerHTML = '<div style="padding: 4px 8px; font-size: 11px; color: #6EE7B7;">Active Tasks: ' + omni.state.tasks.length + '</div>';
// });`;

function saveRawCss() {
  localStorage.setItem('omni_raw_css', rawCssValue);
  if (themeStore.activeTheme) {
    themeStore.activeTheme.rawCss = rawCssValue;
    void themeStore.saveTheme(themeStore.activeTheme);
  }
  omniBridge.injectRawCssToDom(rawCssValue);
  statusMsg = 'Raw CSS saved and applied.';
}

function saveCustomJs() {
  localStorage.setItem('omni_custom_js', customJsValue);
  if (themeStore.activeTheme) {
    themeStore.activeTheme.customJs = customJsValue;
    void themeStore.saveTheme(themeStore.activeTheme);
  }
  omniBridge.executeCustomJs(customJsValue);
  statusMsg = 'Custom JS saved and reloaded.';
}

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
        onclick={async () => {
          const created = await themeStore.saveTheme(createEmptyTheme(`Theme ${themeStore.themes.length + 1}`));
          themeStore.openStudio(created.id);
        }}
        class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#E5E5E8] transition-colors"
      >
        + New Theme
      </button>
      <button
        type="button"
        onclick={() => themeStore.openStudio()}
        class="h-8 px-3.5 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-xs font-medium text-[#111113] transition-colors"
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

  <div class="flex items-center gap-1 border-b border-white/[0.06] pb-2 text-xs">
    <button
      type="button"
      onclick={() => (activeSubTab = 'themes')}
      class="px-3 py-1.5 rounded-[5px] transition-colors {activeSubTab === 'themes' ? 'bg-white/[0.1] text-[#EDEDEF] font-medium' : 'text-[#8E8E95] hover:text-[#D4D4D8]'}"
    >
      Themes
    </button>
    <button
      type="button"
      onclick={() => (activeSubTab = 'custom_css')}
      class="px-3 py-1.5 rounded-[5px] transition-colors {activeSubTab === 'custom_css' ? 'bg-white/[0.1] text-[#EDEDEF] font-medium' : 'text-[#8E8E95] hover:text-[#D4D4D8]'}"
    >
      Raw CSS (Unscoped)
    </button>
    <button
      type="button"
      onclick={() => (activeSubTab = 'custom_js')}
      class="px-3 py-1.5 rounded-[5px] transition-colors {activeSubTab === 'custom_js' ? 'bg-white/[0.1] text-[#EDEDEF] font-medium' : 'text-[#8E8E95] hover:text-[#D4D4D8]'}"
    >
      Custom JS & Widgets
    </button>
    <button
      type="button"
      onclick={() => (activeSubTab = 'safe_mode')}
      class="px-3 py-1.5 rounded-[5px] transition-colors flex items-center gap-1.5 {activeSubTab === 'safe_mode' ? 'bg-white/[0.1] text-[#EDEDEF] font-medium' : 'text-[#8E8E95] hover:text-[#D4D4D8]'}"
    >
      <span>Safe Mode & Binds</span>
      {#if omniBridge.safeMode || omniBridge.errorLogs.length > 0}
        <span class="w-2 h-2 rounded-full {omniBridge.safeMode ? 'bg-[#FBBF24]' : 'bg-[#F87171]'}"></span>
      {/if}
    </button>
  </div>

  {#if activeSubTab === 'themes'}
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
      <span class="px-2 py-0.5 rounded-[4px] font-mono text-[10px] {themeStore.activeThemeId === null ? 'bg-white/[0.12] text-[#EDEDEF]' : 'bg-white/[0.05] text-[#73737A]'}">
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
            class="h-7 px-2.5 rounded-[5px] text-[11px] font-mono {themeStore.activeThemeId === t.id ? 'bg-white/[0.12] text-[#EDEDEF]' : 'bg-white/[0.05] text-[#A3A3AA] hover:text-white'}"
          >
            {themeStore.activeThemeId === t.id ? 'ACTIVE' : 'Apply'}
          </button>
          <button
            type="button"
            onclick={() => themeStore.deleteTheme(t.id)}
            class="h-7 px-2 rounded-[5px] bg-[#1E1E22] hover:bg-[#EF4444]/20 border border-white/[0.07] text-[11px] text-[#73737A] hover:text-[#FCA5A5]"
          >
            Delete
          </button>
        </div>
      </div>
    {/each}
  </div>

  {:else if activeSubTab === 'custom_css'}
  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">Raw Global CSS</div>
        <div class="text-[11px] text-[#73737A]">Unscoped styles directly applied to the DOM. Full access to TaskBoard, Settings, Modals, Toolbars.</div>
      </div>
      <button
        type="button"
        onclick={saveRawCss}
        class="h-7 px-3 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[11px] font-medium text-[#111113] transition-colors"
      >
        Save & Apply CSS
      </button>
    </div>

    <textarea
      bind:value={rawCssValue}
      placeholder={rawCssPlaceholder}
      class="w-full h-72 font-mono text-xs bg-[#101012] border border-white/[0.08] rounded-[6px] p-3 text-[#E5E5E8] placeholder:text-[#52525A] focus:outline-none focus:border-white/[0.2] resize-y"
      spellcheck="false"
    ></textarea>

    <div class="p-2.5 rounded-[6px] bg-[#121214] border border-white/[0.06] text-[11px] text-[#8E8E95] space-y-1">
      <div class="font-medium text-[#D4D4D8]">Available selectors:</div>
      <div class="font-mono text-[10.5px] text-[#A3A3AA] flex flex-wrap gap-2">
        <span class="bg-white/[0.05] px-1.5 py-0.5 rounded">[data-omni-taskboard]</span>
        <span class="bg-white/[0.05] px-1.5 py-0.5 rounded">[data-omni-task-card]</span>
        <span class="bg-white/[0.05] px-1.5 py-0.5 rounded">[data-omni-task-title]</span>
        <span class="bg-white/[0.05] px-1.5 py-0.5 rounded">[data-omni-task-status]</span>
        <span class="bg-white/[0.05] px-1.5 py-0.5 rounded">[data-omni-settings-modal]</span>
        <span class="bg-white/[0.05] px-1.5 py-0.5 rounded">[data-omni-titlebar]</span>
      </div>
    </div>
  </div>

  {:else if activeSubTab === 'custom_js'}
  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">User Runtime Script & Widgets</div>
        <div class="text-[11px] text-[#73737A]">Run custom JS with sandboxed execution and access to <code class="text-[#EDEDEF]">window.omni</code>.</div>
      </div>
      <button
        type="button"
        onclick={saveCustomJs}
        class="h-7 px-3 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[11px] font-medium text-[#111113] transition-colors"
      >
        Save & Run JS
      </button>
    </div>

    <textarea
      bind:value={customJsValue}
      placeholder={customJsPlaceholder}
      class="w-full h-72 font-mono text-xs bg-[#101012] border border-white/[0.08] rounded-[6px] p-3 text-[#E5E5E8] placeholder:text-[#52525A] focus:outline-none focus:border-white/[0.2] resize-y"
      spellcheck="false"
    ></textarea>

    <div class="p-2.5 rounded-[6px] bg-[#121214] border border-white/[0.06] text-[11px] text-[#8E8E95] space-y-1.5">
      <div class="font-medium text-[#D4D4D8]">Frontend omni API:</div>
      <div class="font-mono text-[10.5px] text-[#A3A3AA] space-y-1">
        <div><span class="text-[#EDEDEF]">omni.state</span>: activeChat, tasks, activeModel, workspace, isStreaming, safeMode</div>
        <div><span class="text-[#EDEDEF]">omni.on(event, handler)</span>: 'slot:registered', 'safemode:changed'</div>
        <div><span class="text-[#EDEDEF]">omni.ui.mountWidget(slotId, (container, omni) =&gt; &#123; ... &#125;)</span></div>
        <div>Slots: <span class="text-[#EDEDEF]">'taskboard-top'</span>, <span class="text-[#EDEDEF]">'taskboard-bottom'</span></div>
      </div>
    </div>
  </div>

  {:else if activeSubTab === 'safe_mode'}
  <div class="space-y-4">
    <div class="p-3.5 rounded-[6px] bg-[#121214] border border-white/[0.08] space-y-2">
      <div class="flex items-center justify-between">
        <div>
          <div class="text-xs font-medium text-[#EDEDEF]">Emergency Binds (Аварийные хоткеи)</div>
          <div class="text-[11px] text-[#73737A]">Комбинации для мгновенного восстановления управления при сбоях интерфейса</div>
        </div>
        <button
          type="button"
          onclick={() => omniBridge.toggleSafeMode()}
          class="h-7 px-3 rounded-[5px] text-[11px] font-medium transition-colors {omniBridge.safeMode ? 'bg-[#FBBF24] text-black font-semibold' : 'bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[#D4D4D8]'}"
        >
          {omniBridge.safeMode ? 'Exit Safe Mode' : 'Enable Safe Mode'}
        </button>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-2 pt-1 font-mono text-[11px]">
        <div class="p-2 rounded bg-white/[0.03] border border-white/[0.04]">
          <span class="text-[#FBBF24] font-semibold">Ctrl + Alt + Shift + Esc</span>
          <div class="font-sans text-[10.5px] text-[#8E8E95] mt-0.5">Emergency Safe Mode toggle (мгновенно отключает весь кастомный CSS и JS)</div>
        </div>
        <div class="p-2 rounded bg-white/[0.03] border border-white/[0.04]">
          <span class="text-[#EDEDEF] font-semibold">Ctrl + Shift + R</span>
          <div class="font-sans text-[10.5px] text-[#8E8E95] mt-0.5">Reset Active Theme to Off (сброс темы на дефолт)</div>
        </div>
      </div>
    </div>

    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <div class="text-xs font-medium text-[#EDEDEF]">Crash Monitor & Error Logs</div>
        {#if omniBridge.errorLogs.length > 0}
          <button
            type="button"
            onclick={() => omniBridge.clearErrors()}
            class="text-[11px] text-[#A3A3AA] hover:text-white underline"
          >
            Clear Logs
          </button>
        {/if}
      </div>

      {#if omniBridge.errorLogs.length === 0}
        <div class="p-3 rounded-[6px] bg-[#121214] border border-white/[0.06] text-xs text-[#73737A]">
          No errors recorded. Custom runtime is operating normally.
        </div>
      {:else}
        <div class="space-y-1.5 max-h-60 overflow-y-auto">
          {#each omniBridge.errorLogs as err (err.id)}
            <div class="p-2.5 rounded-[5px] bg-[#161618] border border-[#F87171]/20 text-[11.5px] font-mono text-[#FCA5A5] space-y-0.5">
              <div class="flex items-center justify-between text-[10px] text-[#A3A3AA]">
                <span class="uppercase font-semibold text-[#F87171]">{err.source}</span>
                <span>{new Date(err.timestamp).toLocaleTimeString()}</span>
              </div>
              <div class="break-words">{err.message}</div>
              {#if err.stack}
                <pre class="text-[9.5px] text-[#73737A] overflow-x-auto pt-1">{err.stack.slice(0, 300)}</pre>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
  {/if}

  <div class="text-[11px] text-[#686870]">
    Emergency binds: <span class="font-mono text-[#A3A3AA]">Ctrl + Alt + Shift + Esc</span> (Safe Mode), <span class="font-mono text-[#A3A3AA]">Ctrl + Shift + R</span> (Theme reset).
  </div>
</div>

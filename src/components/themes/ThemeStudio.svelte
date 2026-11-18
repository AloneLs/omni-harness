<script lang="ts">
import { onMount, onDestroy } from 'svelte';
import { themeStore, createEmptyTheme, type OmniTheme } from '../../lib/stores/theme.svelte';
import { chatStore } from '../../lib/stores/chat.svelte';
import CssCodeEditor from './CssCodeEditor.svelte';
import MockHarnessPreview from './MockHarnessPreview.svelte';
import MessageItem from '../MessageItem.svelte';
import Composer from '../Composer.svelte';
import Icon from '../Icon.svelte';

let rightTab = $state<'css' | 'agent' | 'tokens'>('css');
let saveStatus = $state<string | null>(null);
let importInputEl: HTMLInputElement | undefined = $state();
let assetInputEl: HTMLInputElement | undefined = $state();
let prevActiveChatId: string | null = null;

const TEXT_KEYS: Array<{ key: string; label: string; def: string }> = [
  { key: 'new_task', label: 'New Task Button', def: 'New task' },
  { key: 'task_list', label: 'Sidebar Header', def: 'Task list' },
  { key: 'search_chats', label: 'Search Placeholder', def: 'Search chats...' },
  { key: 'ask_placeholder', label: 'Composer Placeholder', def: 'Ask OmniHarness anything...' },
  { key: 'worked_for', label: 'Worked For Label', def: 'Worked for' },
  { key: 'working', label: 'Working Label', def: 'Working' },
  { key: 'send', label: 'Send Button', def: 'Send' },
  { key: 'settings', label: 'Settings Button', def: 'Settings' }
];

const ICON_KEYS = [
  'send',
  'plus',
  'plus-circle',
  'search',
  'settings',
  'sidebar',
  'minimize',
  'maximize',
  'close',
  'pin',
  'edit',
  'trash',
  'archive',
  'copy',
  'check',
  'terminal',
  'folder',
  'user'
];

onMount(() => {
  prevActiveChatId = chatStore.activeChatId;
});

onDestroy(() => {
  if (prevActiveChatId !== undefined) {
    chatStore.activeChatId = prevActiveChatId;
  }
});

$effect(() => {
  if (rightTab === 'agent' && themeStore.editingTheme) {
    chatStore.ensureThemeStudioSession(themeStore.editingTheme.id, themeStore.editingTheme.name);
  } else if (rightTab !== 'agent' && prevActiveChatId !== undefined && chatStore.activeSession?.isThemeStudio) {
    chatStore.activeChatId = prevActiveChatId;
  }
});

function ensureEditing(): OmniTheme {
  if (!themeStore.editingTheme) {
    themeStore.editingTheme = createEmptyTheme('Custom Theme');
  }
  return themeStore.editingTheme;
}

function handleThemeSelect(e: Event) {
  const val = (e.target as HTMLSelectElement).value;
  if (val === '__off__') {
    themeStore.setActiveTheme(null);
    return;
  }
  const found = themeStore.themes.find((t) => t.id === val);
  if (found) {
    themeStore.editingTheme = JSON.parse(JSON.stringify(found));
    if (rightTab === 'agent') {
      chatStore.ensureThemeStudioSession(found.id, found.name);
    }
  }
}

async function createNewTheme() {
  const created = await themeStore.saveTheme(createEmptyTheme(`Theme ${themeStore.themes.length + 1}`));
  themeStore.editingTheme = JSON.parse(JSON.stringify(created));
  if (rightTab === 'agent') {
    chatStore.ensureThemeStudioSession(created.id, created.name);
  }
}

async function saveCurrentTheme() {
  const t = ensureEditing();
  const saved = await themeStore.saveTheme(t);
  themeStore.editingTheme = JSON.parse(JSON.stringify(saved));
  saveStatus = 'Saved';
  setTimeout(() => (saveStatus = null), 1800);
}

async function applyCurrentTheme() {
  const t = ensureEditing();
  const saved = await themeStore.saveTheme(t);
  await themeStore.setActiveTheme(saved.id);
  saveStatus = 'Applied';
  setTimeout(() => (saveStatus = null), 2000);
}

function exportTheme(t: OmniTheme) {
  const blob = new Blob([themeStore.exportThemeJson(t)], { type: 'application/json' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  a.href = url;
  a.download = `${t.name.toLowerCase().replace(/[^a-z0-9]+/g, '-') || 'theme'}.omnitheme.json`;
  a.click();
  URL.revokeObjectURL(url);
}

async function handleImportFile(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0];
  if (!file) return;
  try {
    const raw = await file.text();
    const imported = await themeStore.importThemeJson(raw);
    themeStore.editingTheme = JSON.parse(JSON.stringify(imported));
    saveStatus = `Imported "${imported.name}"`;
    setTimeout(() => (saveStatus = null), 2200);
  } catch (err: any) {
    saveStatus = err?.message || 'Import failed';
  } finally {
    if (importInputEl) importInputEl.value = '';
  }
}

async function handleAssetUpload(e: Event) {
  const file = (e.target as HTMLInputElement).files?.[0];
  if (!file) return;
  const reader = new FileReader();
  reader.onload = () => {
    const dataUri = typeof reader.result === 'string' ? reader.result : '';
    if (!dataUri) return;
    const t = ensureEditing();
    const safeName = file.name.replace(/[^a-zA-Z0-9._-]/g, '_');
    t.assets = { ...t.assets, [safeName]: dataUri };
    const ext = safeName.split('.').pop()?.toLowerCase() || '';
    if (['woff2', 'woff', 'ttf', 'otf'].includes(ext)) {
      const fontName = safeName.replace(/\.[^.]+$/, '');
      t.css = `@font-face {\n  font-family: "${fontName}";\n  src: url("omni-asset://${safeName}");\n}\n` + t.css;
    }
    themeStore.editingTheme = { ...t };
    saveStatus = `Added ${safeName}`;
    setTimeout(() => (saveStatus = null), 1800);
  };
  reader.readAsDataURL(file);
  if (assetInputEl) assetInputEl.value = '';
}

function removeAsset(name: string) {
  const t = ensureEditing();
  const next = { ...t.assets };
  delete next[name];
  t.assets = next;
  themeStore.editingTheme = { ...t };
}

let uploadedAssetNames = $derived(Object.keys(themeStore.editingTheme?.assets || {}));
</script>

<div data-omni-studio-ui="" class="w-full h-full flex flex-col bg-[#101012] text-[#E5E5E8] select-none overflow-hidden">
  <header class="h-11 shrink-0 border-b border-white/[0.06] px-3 flex items-center justify-between gap-2 bg-[#121214]">
    <div class="flex items-center gap-2 min-w-0">
      {#if themeStore.editingTheme}
        <button
          type="button"
          onclick={() => (themeStore.editingTheme = null)}
          class="h-7 px-2 rounded-[5px] bg-[#18181B] hover:bg-[#232328] border border-white/[0.08] text-[11px] text-[#A3A3AA] hover:text-[#EDEDEF]"
        >
          ← Themes
        </button>
      {/if}
      <select
        value={themeStore.editingTheme?.id || '__off__'}
        onchange={handleThemeSelect}
        class="h-7 px-2 rounded-[5px] bg-[#18181B] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none max-w-[160px]"
      >
        <option value="__off__">Select theme...</option>
        {#each themeStore.themes as t (t.id)}
          <option value={t.id}>{t.name}</option>
        {/each}
      </select>

      {#if themeStore.editingTheme}
        <input
          type="text"
          bind:value={themeStore.editingTheme.name}
          class="h-7 px-2 rounded-[5px] bg-[#18181B] border border-white/[0.08] text-xs text-[#EDEDEF] outline-none focus:border-white/[0.2] w-36"
          placeholder="Theme name"
        />
      {/if}

      <button type="button" onclick={createNewTheme} class="h-7 px-2 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#D4D4D8]">+ New</button>
      <button type="button" onclick={() => importInputEl?.click()} class="h-7 px-2 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#A3A3AA]">Import</button>
      <input bind:this={importInputEl} type="file" accept=".json,.omnitheme" onchange={handleImportFile} class="hidden" />
      {#if themeStore.editingTheme}
        <button type="button" onclick={() => themeStore.editingTheme && exportTheme(themeStore.editingTheme)} class="h-7 px-2 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#A3A3AA]">Export</button>
        {#if themeStore.themes.some((x) => x.id === themeStore.editingTheme?.id)}
          <button type="button" onclick={() => themeStore.editingTheme && themeStore.deleteTheme(themeStore.editingTheme.id)} class="h-7 px-2 rounded-[5px] bg-[#1E1E22] hover:bg-[#EF4444]/20 border border-white/[0.08] text-[11px] text-[#A3A3AA] hover:text-[#FCA5A5]">Del</button>
        {/if}
      {/if}

      {#if saveStatus}
        <span class="font-mono text-[11px] text-[#4ADE80] ml-1">{saveStatus}</span>
      {/if}
    </div>

    <div class="flex items-center gap-1.5 shrink-0">
      <button
        type="button"
        onclick={() => (themeStore.isFullscreenPreview = true)}
        class="h-7 px-2.5 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#E5E5E8] transition-colors"
      >
        Fullscreen Preview
      </button>
      <button
        type="button"
        onclick={saveCurrentTheme}
        class="h-7 px-2.5 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#D4D4D8] transition-colors"
      >
        Save
      </button>
      <button
        type="button"
        onclick={applyCurrentTheme}
        class="h-7 px-3 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[11px] font-medium text-[#111113] transition-colors"
      >
        Apply
      </button>
      <button
        type="button"
        onclick={() => {
          if (prevActiveChatId !== undefined && chatStore.activeSession?.isThemeStudio) {
            chatStore.activeChatId = prevActiveChatId;
          }
          themeStore.closeStudio();
        }}
        class="h-7 px-2.5 rounded-[5px] bg-[#18181B] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#A3A3AA] hover:text-[#EDEDEF] transition-colors"
      >
        Exit
      </button>
    </div>
  </header>

  {#if !themeStore.editingTheme}
    <div class="flex-1 min-h-0 overflow-y-auto p-6">
      <div class="max-w-3xl mx-auto space-y-5">
        <div class="flex items-center justify-between gap-3">
          <div>
            <h2 class="text-sm font-medium text-[#EDEDEF]">Themes</h2>
            <p class="text-xs text-[#73737A] mt-0.5">Create a new theme from the default OmniHarness UI or edit an existing one.</p>
          </div>
          <div class="flex items-center gap-2 shrink-0">
            <button
              type="button"
              onclick={() => importInputEl?.click()}
              class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#D4D4D8]"
            >
              Import
            </button>
            <button
              type="button"
              onclick={createNewTheme}
              class="h-8 px-3.5 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-xs font-medium text-[#111113] transition-colors"
            >
              + Create New Theme
            </button>
          </div>
        </div>

        <div class="rounded-[8px] bg-[#18181B]/70 border border-white/[0.07] divide-y divide-white/[0.06]">
          <div class="px-4 py-3 flex items-center justify-between gap-3">
            <div>
              <div class="text-xs font-medium text-[#EDEDEF]">Default OmniHarness (Off)</div>
              <div class="text-[11px] text-[#686870]">Built-in dark theme with zero CSS overrides</div>
            </div>
            <button
              type="button"
              onclick={() => themeStore.setActiveTheme(null)}
              class="h-7 px-2.5 rounded-[5px] text-[11px] font-mono {themeStore.activeThemeId === null ? 'bg-white/[0.12] text-[#EDEDEF]' : 'bg-white/[0.05] text-[#A3A3AA] hover:text-white'}"
            >
              {themeStore.activeThemeId === null ? 'ACTIVE' : 'Use Default'}
            </button>
          </div>

          {#each themeStore.themes as t (t.id)}
            <div class="px-4 py-3 flex items-center justify-between gap-3 hover:bg-white/[0.02] transition-colors">
              <button
                type="button"
                onclick={() => (themeStore.editingTheme = JSON.parse(JSON.stringify(t)))}
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
                  onclick={() => (themeStore.editingTheme = JSON.parse(JSON.stringify(t)))}
                  class="h-7 px-2.5 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#E5E5E8]"
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
                  onclick={() => exportTheme(t)}
                  class="h-7 px-2 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#A3A3AA]"
                >
                  Export
                </button>
                <button
                  type="button"
                  onclick={() => themeStore.deleteTheme(t.id)}
                  class="h-7 px-2 rounded-[5px] bg-[#1E1E22] hover:bg-[#EF4444]/20 border border-white/[0.08] text-[11px] text-[#73737A] hover:text-[#FCA5A5]"
                >
                  Del
                </button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    </div>
  {:else}
    <div class="flex-1 flex min-h-0 overflow-hidden">
      <section class="flex-1 min-w-0 p-2.5 bg-[#101012] flex flex-col">
        <div class="flex-1 min-h-0 rounded-[6px] border border-white/[0.06] overflow-hidden">
          <MockHarnessPreview theme={themeStore.editingTheme} />
        </div>
      </section>

      <aside class="w-[50%] min-w-[460px] shrink-0 border-l border-white/[0.06] bg-[#121214] flex flex-col min-h-0">
        <div class="h-10 shrink-0 border-b border-white/[0.06] px-2.5 flex items-center justify-between gap-2">
          <div class="flex items-center gap-1">
            {#each [['css', 'CSS Editor'], ['agent', 'Theme Agent'], ['tokens', 'Icons & Texts']] as [id, label]}
              <button
                type="button"
                onclick={() => (rightTab = id as any)}
                class="px-2.5 py-1 rounded-[5px] text-xs transition-colors {rightTab === id ? 'bg-white/[0.1] text-[#EDEDEF] font-medium' : 'text-[#8E8E95] hover:text-[#D4D4D8]'}"
              >
                {label}
              </button>
            {/each}
          </div>

          <div class="flex items-center gap-1.5">
            <button
              type="button"
              onclick={() => assetInputEl?.click()}
              class="h-6 px-2.5 rounded-[4px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#D4D4D8]"
              title="Upload font (.woff2/.ttf/.otf) or image (.png/.jpg/.webp/.svg)"
            >
              + Font / Image
            </button>
            <input bind:this={assetInputEl} type="file" accept=".woff2,.woff,.ttf,.otf,.png,.jpg,.jpeg,.webp,.svg,.gif,.ico" onchange={handleAssetUpload} class="hidden" />
          </div>
        </div>

        {#if themeStore.editingTheme && Object.keys(themeStore.editingTheme.assets).length > 0 && rightTab === 'css'}
          <div class="px-2.5 py-1.5 border-b border-white/[0.06] flex flex-wrap gap-1.5 bg-[#101012]">
            {#each Object.keys(themeStore.editingTheme.assets) as name}
              <span class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-[4px] bg-[#18181B] border border-white/[0.07] font-mono text-[10.5px] text-[#D4D4D8]">
                <span>omni-asset://{name}</span>
                <button type="button" onclick={() => removeAsset(name)} class="text-[#73737A] hover:text-[#F87171]">×</button>
              </span>
            {/each}
          </div>
        {/if}

        <div class="flex-1 min-h-0 overflow-hidden flex flex-col">
          {#if rightTab === 'css' && themeStore.editingTheme}
            <div class="flex-1 min-h-0 p-2">
              <CssCodeEditor bind:value={themeStore.editingTheme.css} />
            </div>
          {:else if rightTab === 'agent' && themeStore.editingTheme}
            <div class="flex-1 min-h-0 flex flex-col bg-[#101012]">
              <div class="flex-1 min-h-0 overflow-y-auto px-4 py-3 space-y-2">
                {#if chatStore.messages.length === 0}
                  <div class="p-3 rounded-[6px] bg-[#161618] border border-white/[0.06] text-xs text-[#A3A3AA] leading-relaxed">
                    Theme Agent uses the full OmniHarness agent pipeline with isolated design tools (theme_get_dom_spec, theme_read_current, theme_apply_css, theme_set_icon, theme_set_texts). Select any provider/model below and attach screenshots or ask for layout/CSS changes.
                  </div>
                {/if}
                {#each chatStore.messages as message (message.id)}
                  <MessageItem
                    {message}
                    onEditUser={(id: string, newText: string, rollback: boolean) => chatStore.editUserMessage(id, newText, rollback)}
                    onRegenerate={(id: string) => chatStore.regenerateAssistantMessage(id)}
                    onContinue={(id: string) => chatStore.continueAssistantMessage(id)}
                  />
                {/each}
              </div>
              <div class="shrink-0 px-3 pb-3 pt-1">
                <Composer />
              </div>
            </div>
          {:else if rightTab === 'tokens' && themeStore.editingTheme}
            <div class="flex-1 overflow-y-auto p-3 space-y-4">
              <div class="space-y-2">
                <div class="font-mono text-[10.5px] uppercase text-[#73737A]">Text & Language Overrides</div>
                {#each TEXT_KEYS as tk}
                  <label class="block space-y-1">
                    <span class="text-[11px] text-[#A3A3AA]">{tk.label} ({tk.key})</span>
                    <input
                      type="text"
                      value={themeStore.editingTheme.textOverrides[tk.key] || ''}
                      placeholder={tk.def}
                      oninput={(e) => {
                        if (!themeStore.editingTheme) return;
                        const v = (e.target as HTMLInputElement).value;
                        themeStore.editingTheme.textOverrides = { ...themeStore.editingTheme.textOverrides, [tk.key]: v };
                      }}
                      class="w-full h-7 px-2.5 rounded-[5px] bg-[#161618] border border-white/[0.08] text-xs text-[#EDEDEF] placeholder-[#52525B] outline-none"
                    />
                  </label>
                {/each}
              </div>

              <div class="space-y-2 pt-2 border-t border-white/[0.06]">
                <div class="font-mono text-[10.5px] uppercase text-[#73737A]">Icon Overrides (SVG, Path, or Image URL / omni-asset://)</div>
                {#each ICON_KEYS as ik}
                  <div class="space-y-1">
                    <div class="flex items-center justify-between text-[11px]">
                      <span class="font-mono text-[#A3A3AA]">{ik}</span>
                      {#if themeStore.editingTheme.iconOverrides[ik]}
                        <button
                          type="button"
                          onclick={() => {
                            if (!themeStore.editingTheme) return;
                            const next = { ...themeStore.editingTheme.iconOverrides };
                            delete next[ik];
                            themeStore.editingTheme.iconOverrides = next;
                          }}
                          class="font-mono text-[10px] text-[#F87171] hover:underline"
                        >
                          Clear
                        </button>
                      {/if}
                    </div>
                    <div class="flex items-center gap-2">
                      <div class="w-8 h-8 rounded-[5px] bg-[#161618] border border-white/[0.08] flex items-center justify-center shrink-0">
                        <Icon name={ik as any} class="w-4 h-4 text-[#EDEDEF]" previewOnly={true} />
                      </div>
                      <input
                        type="text"
                        value={themeStore.editingTheme.iconOverrides[ik] || ''}
                        placeholder="Paste <svg>...</svg>, path d, or omni-asset://..."
                        oninput={(e) => {
                          if (!themeStore.editingTheme) return;
                          const v = (e.target as HTMLInputElement).value;
                          themeStore.editingTheme.iconOverrides = { ...themeStore.editingTheme.iconOverrides, [ik]: v };
                        }}
                        class="flex-1 h-8 px-2.5 rounded-[5px] bg-[#161618] border border-white/[0.08] font-mono text-[11px] text-[#EDEDEF] placeholder-[#52525B] outline-none"
                      />
                      {#if uploadedAssetNames.length > 0}
                        <select
                          onchange={(e) => {
                            const val = (e.target as HTMLSelectElement).value;
                            if (val && themeStore.editingTheme) {
                              themeStore.editingTheme.iconOverrides = {
                                ...themeStore.editingTheme.iconOverrides,
                                [ik]: `omni-asset://${val}`
                              };
                              (e.target as HTMLSelectElement).value = '';
                            }
                          }}
                          class="h-8 px-1.5 rounded-[5px] bg-[#18181B] border border-white/[0.08] text-[10px] text-[#A3A3AA] outline-none max-w-[90px]"
                        >
                          <option value="">Asset...</option>
                          {#each uploadedAssetNames as aname}
                            <option value={aname}>{aname}</option>
                          {/each}
                        </select>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      </aside>
    </div>
  {/if}
</div>
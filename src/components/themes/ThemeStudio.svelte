<script lang="ts">
import { invoke, Channel } from '@tauri-apps/api/core';
import { themeStore, createEmptyTheme, type OmniTheme } from '../../lib/stores/theme.svelte';
import { settingsStore } from '../../lib/stores/settings.svelte';
import CssCodeEditor from './CssCodeEditor.svelte';
import MockHarnessPreview from './MockHarnessPreview.svelte';
import Icon from '../Icon.svelte';

let rightTab = $state<'css' | 'assets' | 'tokens' | 'agent'>('css');
let saveStatus = $state<string | null>(null);
let importInputEl: HTMLInputElement | undefined = $state();
let assetInputEl: HTMLInputElement | undefined = $state();

interface AgentMsg {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  tools?: string[];
  streaming?: boolean;
}

let agentMessages = $state<AgentMsg[]>([]);
let agentPrompt = $state('');
let agentBusy = $state(false);

const TEXT_KEYS: Array<{ key: string; label: string; def: string }> = [
  { key: 'new_task', label: 'New Task Button', def: 'New task' },
  { key: 'task_list', label: 'Sidebar Header', def: 'Task list' },
  { key: 'search_chats', label: 'Search Placeholder', def: 'Search chats...' },
  { key: 'ask_placeholder', label: 'Composer Placeholder', def: 'Ask OmniHarness anything...' },
  { key: 'worked_for', label: 'Worked For Label', def: 'Worked for' },
  { key: 'working', label: 'Working Label', def: 'Working' },
  { key: 'send', label: 'Send Button', def: 'Send' }
];

const ICON_KEYS = ['send', 'plus', 'settings', 'sidebar', 'search', 'copy', 'check', 'edit', 'trash', 'terminal', 'folder'];

function ensureEditing(): OmniTheme {
  if (!themeStore.editingTheme) {
    themeStore.editingTheme = createEmptyTheme('Custom Theme');
  }
  return themeStore.editingTheme;
}

function selectTheme(t: OmniTheme) {
  themeStore.editingTheme = JSON.parse(JSON.stringify(t));
}

function createNewTheme() {
  const t = createEmptyTheme(`Theme ${themeStore.themes.length + 1}`);
  themeStore.editingTheme = t;
  themeStore.saveTheme(t).catch(() => {});
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
  saveStatus = 'Applied to App';
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

async function sendToThemeAgent() {
  const prompt = agentPrompt.trim();
  if (!prompt || agentBusy) return;
  const prov = settingsStore.currentProvider;
  const model = settingsStore.data.activeModel;
  if (!prov || !model) {
    agentMessages = [
      ...agentMessages,
      { id: `err-${Date.now()}`, role: 'assistant', content: 'Select an active provider & model in Settings -> Models & Providers first.' }
    ];
    return;
  }

  const t = ensureEditing();
  await themeStore.saveTheme(t);
  agentPrompt = '';
  agentBusy = true;

  const history = agentMessages
    .filter((m) => !m.streaming && m.content.trim())
    .map((m) => ({ role: m.role, content: m.content }));

  agentMessages = [
    ...agentMessages,
    { id: `u-${Date.now()}`, role: 'user', content: prompt },
    { id: `a-${Date.now() + 1}`, role: 'assistant', content: '', tools: [], streaming: true }
  ];

  const onEvent = new Channel<any>();
  onEvent.onmessage = (ev) => {
    const last = agentMessages[agentMessages.length - 1];
    if (!last || last.role !== 'assistant') return;
    if (ev?.type === 'delta' && ev.content) {
      last.content += ev.content;
      agentMessages = [...agentMessages];
    } else if (ev?.type === 'tool_call_complete' && ev.name) {
      last.tools = [...(last.tools || []), ev.name];
      agentMessages = [...agentMessages];
    } else if (ev?.type === 'error') {
      last.content += `\n[Error: ${ev.error}]`;
      last.streaming = false;
      agentBusy = false;
      agentMessages = [...agentMessages];
    } else if (ev?.type === 'done') {
      last.streaming = false;
      agentBusy = false;
      agentMessages = [...agentMessages];
    }
  };

  try {
    await invoke('start_theme_agent_stream', {
      themeId: t.id,
      prompt,
      modelId: model,
      reasoningEffort: settingsStore.data.reasoningLevel.toLowerCase(),
      providerName: prov.name,
      baseUrl: prov.baseUrl,
      apiKey: prov.apiKey,
      apiProtocol: prov.apiProtocol || null,
      proxyEnabled: Boolean(prov.proxyEnabled),
      proxyUrl: prov.proxyUrl || null,
      history,
      onEvent
    });
  } catch (err: any) {
    const last = agentMessages[agentMessages.length - 1];
    if (last) {
      last.content = err?.message || String(err);
      last.streaming = false;
    }
    agentBusy = false;
    agentMessages = [...agentMessages];
  }
}
</script>

<div data-omni-studio-ui="" class="w-full h-full flex flex-col bg-[#08080B] text-[#E5E5E8] select-none overflow-hidden">
  <header class="h-12 shrink-0 border-b border-white/[0.08] px-4 flex items-center justify-between gap-3 bg-[#0C0C10]">
    <div class="flex items-center gap-3 min-w-0">
      <span class="font-mono text-xs tracking-wider uppercase text-[#C4B5FD]">Theme Studio</span>
      {#if themeStore.editingTheme}
        <input
          type="text"
          bind:value={themeStore.editingTheme.name}
          class="px-2.5 py-1 rounded-[5px] bg-[#121216] border border-white/[0.08] text-xs font-medium text-[#EDEDEF] outline-none focus:border-[#8B5CF6]/60 w-48"
        />
      {/if}
      {#if saveStatus}
        <span class="font-mono text-[11px] text-[#4ADE80]">{saveStatus}</span>
      {/if}
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        onclick={() => (themeStore.isFullscreenPreview = true)}
        class="h-8 px-3 rounded-[6px] bg-[#1E1E24] hover:bg-[#272730] border border-white/[0.08] text-xs text-[#E5E5E8] transition-colors"
      >
        Fullscreen Preview
      </button>
      <button
        type="button"
        onclick={saveCurrentTheme}
        class="h-8 px-3 rounded-[6px] bg-[#1E1E24] hover:bg-[#272730] border border-white/[0.08] text-xs text-[#D4D4D8] transition-colors"
      >
        Save
      </button>
      <button
        type="button"
        onclick={applyCurrentTheme}
        class="h-8 px-3 rounded-[6px] bg-[#8B5CF6] hover:bg-[#7C3AED] text-xs font-medium text-white transition-colors"
      >
        Apply to App
      </button>
      <button
        type="button"
        onclick={() => themeStore.closeStudio()}
        class="h-8 px-3 rounded-[6px] bg-white/[0.06] hover:bg-white/[0.12] text-xs text-[#EDEDEF] transition-colors"
      >
        Exit Studio
      </button>
    </div>
  </header>

  <div class="flex-1 flex min-h-0 overflow-hidden">
    <aside class="w-56 shrink-0 border-r border-white/[0.08] bg-[#0B0B0F] flex flex-col p-3 gap-2">
      <div class="flex items-center justify-between">
        <span class="font-mono text-[10.5px] uppercase tracking-wider text-[#73737A]">Themes</span>
        <div class="flex items-center gap-1">
          <button
            type="button"
            onclick={createNewTheme}
            class="px-2 py-0.5 rounded bg-white/[0.06] hover:bg-white/[0.12] text-[11px] text-[#EDEDEF]"
          >
            + New
          </button>
          <button
            type="button"
            onclick={() => importInputEl?.click()}
            class="px-2 py-0.5 rounded bg-white/[0.04] hover:bg-white/[0.1] text-[11px] text-[#A3A3AA]"
          >
            Import
          </button>
        </div>
      </div>
      <input bind:this={importInputEl} type="file" accept=".json,.omnitheme" onchange={handleImportFile} class="hidden" />

      <button
        type="button"
        onclick={() => themeStore.setActiveTheme(null)}
        class="w-full text-left px-2.5 py-1.5 rounded-[5px] text-xs flex items-center justify-between border {themeStore.activeThemeId === null ? 'bg-white/[0.08] border-white/[0.15] text-[#EDEDEF]' : 'border-transparent text-[#A3A3AA] hover:bg-white/[0.04]'}"
      >
        <span>Off (Default)</span>
        {#if themeStore.activeThemeId === null}
          <span class="w-1.5 h-1.5 rounded-full bg-[#4ADE80]"></span>
        {/if}
      </button>

      <div class="flex-1 overflow-y-auto space-y-1 pt-1">
        {#each themeStore.themes as t (t.id)}
          <div class="group flex items-center gap-1 rounded-[5px] border {themeStore.editingTheme?.id === t.id ? 'bg-[#8B5CF6]/15 border-[#8B5CF6]/40' : 'border-transparent hover:bg-white/[0.04]'}">
            <button
              type="button"
              onclick={() => selectTheme(t)}
              class="flex-1 text-left px-2.5 py-1.5 text-xs truncate text-[#E5E5E8] flex items-center gap-1.5"
            >
              {#if themeStore.activeThemeId === t.id}
                <span class="w-1.5 h-1.5 rounded-full bg-[#8B5CF6] shrink-0"></span>
              {/if}
              <span class="truncate">{t.name}</span>
            </button>
            <button
              type="button"
              onclick={() => exportTheme(t)}
              class="px-1.5 py-1 text-[10px] text-[#73737A] hover:text-[#EDEDEF] opacity-0 group-hover:opacity-100"
              title="Export JSON"
            >
              Exp
            </button>
            <button
              type="button"
              onclick={() => themeStore.deleteTheme(t.id)}
              class="px-1.5 py-1 text-[10px] text-[#73737A] hover:text-[#F87171] opacity-0 group-hover:opacity-100"
              title="Delete theme"
            >
              ×
            </button>
          </div>
        {/each}
      </div>
    </aside>

    <section class="flex-1 min-w-0 p-3 bg-[#08080B] flex flex-col">
      <div class="flex-1 min-h-0 rounded-[8px] border border-white/[0.08] overflow-hidden">
        <MockHarnessPreview theme={themeStore.editingTheme} />
      </div>
    </section>

    <aside class="w-[440px] shrink-0 border-l border-white/[0.08] bg-[#0B0B0F] flex flex-col min-h-0">
      <div class="h-10 shrink-0 border-b border-white/[0.08] px-2 flex items-center gap-1">
        {#each [['css', 'CSS Editor'], ['assets', 'Fonts & Media'], ['tokens', 'Icons & Texts'], ['agent', 'Theme Agent']] as [id, label]}
          <button
            type="button"
            onclick={() => (rightTab = id as any)}
            class="px-2.5 py-1 rounded-[5px] text-xs transition-colors {rightTab === id ? 'bg-white/[0.1] text-[#EDEDEF] font-medium' : 'text-[#8E8E95] hover:text-[#D4D4D8]'}"
          >
            {label}
          </button>
        {/each}
      </div>

      <div class="flex-1 min-h-0 overflow-hidden p-2.5">
        {#if rightTab === 'css' && themeStore.editingTheme}
          <CssCodeEditor bind:value={themeStore.editingTheme.css} />
        {:else if rightTab === 'assets' && themeStore.editingTheme}
          <div class="h-full flex flex-col gap-3 overflow-y-auto">
            <div class="flex items-center justify-between">
              <span class="text-xs text-[#A3A3AA]">Upload fonts (.woff2, .ttf) or images</span>
              <button
                type="button"
                onclick={() => assetInputEl?.click()}
                class="h-7 px-2.5 rounded-[5px] bg-[#27272A] hover:bg-[#323236] text-xs text-[#EDEDEF]"
              >
                + Upload File
              </button>
              <input bind:this={assetInputEl} type="file" accept=".woff2,.woff,.ttf,.otf,.png,.jpg,.jpeg,.webp,.svg,.gif" onchange={handleAssetUpload} class="hidden" />
            </div>
            <div class="space-y-1.5">
              {#each Object.keys(themeStore.editingTheme.assets) as name}
                <div class="flex items-center justify-between px-2.5 py-1.5 rounded-[5px] bg-[#121216] border border-white/[0.06] text-xs">
                  <span class="font-mono text-[#D4D4D8] truncate">omni-asset://{name}</span>
                  <button type="button" onclick={() => removeAsset(name)} class="text-[11px] text-[#F87171]">Remove</button>
                </div>
              {/each}
            </div>
          </div>
        {:else if rightTab === 'tokens' && themeStore.editingTheme}
          <div class="h-full overflow-y-auto space-y-4 pr-1">
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
                    class="w-full h-7 px-2.5 rounded-[5px] bg-[#121216] border border-white/[0.08] text-xs text-[#EDEDEF] placeholder-[#52525B] outline-none"
                  />
                </label>
              {/each}
            </div>

            <div class="space-y-2 pt-2 border-t border-white/[0.06]">
              <div class="font-mono text-[10.5px] uppercase text-[#73737A]">SVG Icon Overrides</div>
              {#each ICON_KEYS as ik}
                <label class="block space-y-1">
                  <span class="text-[11px] font-mono text-[#A3A3AA]">{ik}</span>
                  <input
                    type="text"
                    value={themeStore.editingTheme.iconOverrides[ik] || ''}
                    placeholder="Paste <svg>...</svg> or SVG path d"
                    oninput={(e) => {
                      if (!themeStore.editingTheme) return;
                      const v = (e.target as HTMLInputElement).value;
                      themeStore.editingTheme.iconOverrides = { ...themeStore.editingTheme.iconOverrides, [ik]: v };
                    }}
                    class="w-full h-7 px-2.5 rounded-[5px] bg-[#121216] border border-white/[0.08] font-mono text-[11px] text-[#EDEDEF] placeholder-[#52525B] outline-none"
                  />
                </label>
              {/each}
            </div>
          </div>
        {:else if rightTab === 'agent'}
          <div class="h-full flex flex-col gap-2">
            <div class="flex-1 overflow-y-auto space-y-2 pr-1">
              {#if agentMessages.length === 0}
                <div class="p-3 rounded-[6px] bg-[#121216] border border-white/[0.06] text-xs text-[#8E8E95] leading-relaxed">
                  Theme Agent has isolated access only to CSS, icons, texts, and DOM anchors. Describe the look, layout, fonts, animations, or language you want.
                </div>
              {/if}
              {#each agentMessages as m (m.id)}
                <div class="p-2.5 rounded-[6px] text-xs {m.role === 'user' ? 'bg-[#1E1E24] text-[#EDEDEF] ml-6' : 'bg-[#121216] border border-white/[0.06] text-[#D4D4D8] mr-2'}">
                  {#if m.tools && m.tools.length > 0}
                    <div class="flex flex-wrap gap-1 mb-1.5">
                      {#each m.tools as tl}
                        <span class="px-1.5 py-0.5 rounded bg-[#8B5CF6]/15 font-mono text-[10px] text-[#C4B5FD]">{tl}</span>
                      {/each}
                    </div>
                  {/if}
                  <div class="whitespace-pre-wrap leading-relaxed">{m.content || (m.streaming ? 'Designing...' : '')}</div>
                </div>
              {/each}
            </div>
            <div class="flex items-center gap-1.5 pt-1 border-t border-white/[0.06]">
              <input
                type="text"
                bind:value={agentPrompt}
                onkeydown={(e) => {
                  if (e.key === 'Enter') sendToThemeAgent();
                }}
                placeholder="Ask Theme Agent to restyle..."
                class="flex-1 h-8 px-2.5 rounded-[6px] bg-[#121216] border border-white/[0.08] text-xs text-[#EDEDEF] placeholder-[#52525B] outline-none"
              />
              <button
                type="button"
                disabled={agentBusy || !agentPrompt.trim()}
                onclick={sendToThemeAgent}
                class="h-8 px-3 rounded-[6px] bg-[#8B5CF6] hover:bg-[#7C3AED] text-xs font-medium text-white disabled:opacity-40"
              >
                {agentBusy ? '...' : 'Run'}
              </button>
            </div>
          </div>
        {/if}
      </div>
    </aside>
  </div>
</div>

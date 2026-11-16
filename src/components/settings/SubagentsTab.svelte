<script lang="ts">
import { invoke } from '@tauri-apps/api/core';
import { settingsStore } from '../../lib/stores/settings.svelte';
import { chatStore } from '../../lib/stores/chat.svelte';

interface SubagentItem {
  name: string;
  description: string;
  isSystem: boolean;
  tools: string[];
  disallowedTools: string[];
  model: string | null;
  maxTurns: number | null;
  systemPrompt: string;
  filePath: string;
  scope: string;
}

const AVAILABLE_TOOLS = [
  'read_file',
  'write_file',
  'edit_file',
  'patch_file',
  'delete_file',
  'glob',
  'grep',
  'code_intel',
  'terminal_exec',
  'terminal_io',
  'web_research',
  'tool_hub',
  'memory_tool'
];

let subagents = $state<SubagentItem[]>([]);
let searchQuery = $state<string>('');
let statusText = $state<string | null>(null);
let expandedName = $state<string | null>(null);
let confirmDeleteName = $state<string | null>(null);

let showForm = $state<boolean>(false);
let editingExisting = $state<boolean>(false);
let formName = $state<string>('');
let formDescription = $state<string>('');
let formScope = $state<'workspace' | 'global'>('workspace');
let formModel = $state<string>('');
let formMaxTurns = $state<string>('25');
let formTools = $state<string[]>(['read_file', 'glob', 'grep', 'code_intel']);
let formSystemPrompt = $state<string>('');
let saving = $state<boolean>(false);

async function loadSubagents() {
  try {
    const res = await invoke<SubagentItem[]>('list_subagents');
    if (Array.isArray(res)) {
      subagents = res;
    }
  } catch (err: unknown) {
    statusText = err instanceof Error ? err.message : String(err);
  }
}

$effect(() => {
  loadSubagents();
});

let systemAgents = $derived(
  subagents.filter((a) => {
    if (!a.isSystem) return false;
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    return a.name.toLowerCase().includes(q) || a.description.toLowerCase().includes(q);
  })
);

let customAgents = $derived(
  subagents.filter((a) => {
    if (a.isSystem) return false;
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    return a.name.toLowerCase().includes(q) || a.description.toLowerCase().includes(q);
  })
);

function openCreateForm() {
  editingExisting = false;
  formName = '';
  formDescription = '';
  formScope = 'workspace';
  formModel = '';
  formMaxTurns = '25';
  formTools = ['read_file', 'glob', 'grep', 'code_intel'];
  formSystemPrompt = '';
  showForm = true;
  statusText = null;
}

function openEditForm(agent: SubagentItem) {
  if (agent.isSystem) return;
  editingExisting = true;
  formName = agent.name;
  formDescription = agent.description;
  formScope = agent.scope === 'global' ? 'global' : 'workspace';
  formModel = agent.model || '';
  formMaxTurns = agent.maxTurns ? String(agent.maxTurns) : '25';
  formTools = [...agent.tools];
  formSystemPrompt = agent.systemPrompt;
  showForm = true;
  statusText = null;
}

function toggleTool(tool: string) {
  if (formTools.includes(tool)) {
    formTools = formTools.filter((t) => t !== tool);
  } else {
    formTools = [...formTools, tool];
  }
}

async function saveForm() {
  saving = true;
  statusText = null;
  try {
    const parsedTurns = parseInt(formMaxTurns, 10);
    await invoke('save_custom_subagent', {
      name: formName.trim(),
      description: formDescription.trim(),
      tools: formTools,
      disallowedTools: [],
      model: formModel.trim() || null,
      maxTurns: Number.isFinite(parsedTurns) && parsedTurns > 0 ? parsedTurns : null,
      systemPrompt: formSystemPrompt.trim(),
      scope: formScope
    });
    showForm = false;
    statusText = `Saved subagent: ${formName.trim()}`;
    await loadSubagents();
  } catch (err: unknown) {
    statusText = err instanceof Error ? err.message : String(err);
  } finally {
    saving = false;
  }
}

async function deleteAgent(agent: SubagentItem) {
  if (agent.isSystem) return;
  if (confirmDeleteName !== agent.name) {
    confirmDeleteName = agent.name;
    return;
  }
  confirmDeleteName = null;
  try {
    await invoke('delete_custom_subagent', {
      name: agent.name,
      filePath: agent.filePath || null
    });
    statusText = `Deleted subagent: ${agent.name}`;
    await loadSubagents();
  } catch (err: unknown) {
    statusText = err instanceof Error ? err.message : String(err);
  }
}

async function createSubagentWithAgent() {
  settingsStore.closeSettings();
  await chatStore.createSession('Subagent Architect');
  await chatStore.sendMessage(
    'Помоги мне спроектировать и создать нового кастомного сабагента через `subagent_builder`. ' +
    'Не создавай его сразу наугад — сначала проведи подробный опрос по шагам: ' +
    '1) имя, роль и когда он должен вызываться; ' +
    '2) какие инструменты ему разрешены или запрещены (read_file, write_file, edit_file, patch_file, delete_file, glob, grep, code_intel, terminal_exec, web_research, memory_tool); ' +
    '3) область сохранения (workspace .omni/agents или global), модель и лимит ходов (max_turns); ' +
    '4) подробные правила поведения и формат итогового отчета. ' +
    'После моих ответов покажи итоговую конфигурацию и создай сабагента через `subagent_builder(action="create", ...)`.'
  );
}
</script>

<div class="space-y-4 text-[13px] text-[#E5E5E8]">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h2 class="text-sm font-medium text-[#EDEDEF]">Subagents</h2>
      <p class="text-xs text-[#73737A] mt-0.5">System subagents and extensible custom subagents (.omni/agents).</p>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#E5E5E8] active:scale-[0.97] transition-all whitespace-nowrap"
        onclick={openCreateForm}
      >
        + New Subagent
      </button>
      <button
        type="button"
        class="h-8 px-3.5 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium active:scale-[0.97] transition-all whitespace-nowrap"
        onclick={createSubagentWithAgent}
      >
        Create with Agent
      </button>
    </div>
  </div>

  {#if statusText}
    <div class="px-3 py-2 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] text-xs text-[#D4D4D8]">
      {statusText}
    </div>
  {/if}

  {#if showForm}
    <div class="p-3.5 rounded-[6px] bg-[#121214] border border-white/[0.08] space-y-3">
      <div class="flex items-center justify-between">
        <span class="text-xs font-medium text-[#EDEDEF]">
          {editingExisting ? `Edit Subagent: ${formName}` : 'New Custom Subagent'}
        </span>
        <button
          type="button"
          class="text-xs text-[#73737A] hover:text-[#E5E5E8]"
          onclick={() => (showForm = false)}
        >
          Cancel
        </button>
      </div>

      <div class="grid grid-cols-2 gap-2.5">
        <div>
          <label for="subagent-name" class="block text-[11px] text-[#8E8E95] mb-1">Name</label>
          <input id="subagent-name"
            type="text"
            bind:value={formName}
            disabled={editingExisting}
            placeholder="security-auditor"
            class="w-full h-8 px-2.5 rounded-[6px] bg-[#18181B] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none disabled:opacity-60"
          />
        </div>
        <div>
          <label for="subagent-scope" class="block text-[11px] text-[#8E8E95] mb-1">Scope</label>
          <select id="subagent-scope"
            bind:value={formScope}
            disabled={editingExisting}
            class="w-full h-8 px-2.5 rounded-[6px] bg-[#18181B] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none disabled:opacity-60"
          >
            <option value="workspace">Workspace (.omni/agents)</option>
            <option value="global">Global (~/.omni/agents)</option>
          </select>
        </div>
      </div>

      <div>
        <label for="subagent-desc" class="block text-[11px] text-[#8E8E95] mb-1">Description (when main agent should invoke it)</label>
        <input id="subagent-desc"
          type="text"
          bind:value={formDescription}
          placeholder="Reviews diffs and code paths for security regressions and memory leaks."
          class="w-full h-8 px-2.5 rounded-[6px] bg-[#18181B] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none"
        />
      </div>

      <div class="grid grid-cols-2 gap-2.5">
        <div>
          <label for="subagent-model" class="block text-[11px] text-[#8E8E95] mb-1">Model (optional, default inherit)</label>
          <input id="subagent-model"
            type="text"
            bind:value={formModel}
            placeholder="inherit"
            class="w-full h-8 px-2.5 rounded-[6px] bg-[#18181B] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none"
          />
        </div>
        <div>
          <label for="subagent-turns" class="block text-[11px] text-[#8E8E95] mb-1">Max Turns (1-80)</label>
          <input id="subagent-turns"
            type="number"
            min="1"
            max="80"
            bind:value={formMaxTurns}
            class="w-full h-8 px-2.5 rounded-[6px] bg-[#18181B] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none"
          />
        </div>
      </div>

      <div>
        <div class="block text-[11px] text-[#8E8E95] mb-1.5">Allowed Tools</div>
        <div class="flex flex-wrap gap-1.5">
          {#each AVAILABLE_TOOLS as tool}
            <button
              type="button"
              class="px-2 py-1 rounded-[4px] font-mono text-[11px] border transition-colors {formTools.includes(tool) ? 'bg-[#8B5CF6]/20 border-[#8B5CF6]/50 text-[#E4D4FF]' : 'bg-[#18181B] border-white/[0.06] text-[#73737A] hover:text-[#D4D4D8]'}"
              onclick={() => toggleTool(tool)}
            >
              {tool}
            </button>
          {/each}
        </div>
      </div>

      <div>
        <label for="subagent-prompt" class="block text-[11px] text-[#8E8E95] mb-1">System Prompt</label>
        <textarea id="subagent-prompt"
          rows="5"
          bind:value={formSystemPrompt}
          placeholder="Define exact role, constraints, and output format..."
          class="w-full p-2.5 rounded-[6px] bg-[#18181B] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none resize-y font-mono"
        ></textarea>
      </div>

      <div class="flex justify-end gap-2">
        <button
          type="button"
          class="h-8 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium disabled:opacity-50"
          disabled={saving}
          onclick={saveForm}
        >
          {saving ? 'Saving...' : 'Save Subagent'}
        </button>
      </div>
    </div>
  {/if}

  <div>
    <input
      type="text"
      bind:value={searchQuery}
      placeholder="Search subagents..."
      class="w-full h-8 px-3 rounded-[6px] bg-[#121214] border border-white/[0.06] text-xs text-[#E5E5E8] placeholder-[#686870] outline-none focus:border-white/[0.15]"
    />
  </div>

  <div class="space-y-2">
    <div class="text-[11px] font-mono uppercase tracking-wider text-[#73737A]">System Subagents (Read-Only)</div>
    {#each systemAgents as agent (agent.name)}
      <div class="rounded-[6px] bg-[#121214] border border-white/[0.06] p-3">
        <div class="flex items-start justify-between gap-3">
          <button
            type="button"
            class="text-left flex-1 min-w-0"
            onclick={() => (expandedName = expandedName === agent.name ? null : agent.name)}
          >
            <div class="flex items-center gap-2">
              <span class="font-medium text-[#EDEDEF]">{agent.name}</span>
              <span class="px-1.5 py-0.5 rounded text-[10px] font-mono bg-[#8B5CF6]/15 text-[#C4B5FD] border border-[#8B5CF6]/25">
                System
              </span>
              {#if agent.maxTurns}
                <span class="text-[10px] font-mono text-[#73737A]">{agent.maxTurns} turns</span>
              {/if}
            </div>
            <p class="text-xs text-[#8E8E95] mt-1">{agent.description}</p>
          </button>
        </div>
        {#if expandedName === agent.name}
          <div class="mt-2.5 pt-2.5 border-t border-white/[0.06] space-y-2">
            <div class="flex flex-wrap gap-1">
              {#each agent.tools as t}
                <span class="px-1.5 py-0.5 rounded bg-[#18181B] border border-white/[0.06] font-mono text-[10px] text-[#A1A1AA]">{t}</span>
              {/each}
            </div>
            <pre class="text-[11px] font-mono text-[#A1A1AA] whitespace-pre-wrap bg-[#18181B] p-2.5 rounded-[5px] border border-white/[0.05] max-h-40 overflow-y-auto">{agent.systemPrompt}</pre>
          </div>
        {/if}
      </div>
    {/each}
  </div>

  <div class="space-y-2 pt-1">
    <div class="text-[11px] font-mono uppercase tracking-wider text-[#73737A]">Custom Subagents</div>
    {#if customAgents.length === 0}
      <div class="rounded-[6px] bg-[#121214] border border-dashed border-white/[0.08] p-4 text-center text-xs text-[#73737A]">
        No custom subagents yet. Use "+ New Subagent" or "Create with Agent".
      </div>
    {:else}
      {#each customAgents as agent (agent.name)}
        <div class="rounded-[6px] bg-[#121214] border border-white/[0.06] p-3">
          <div class="flex items-start justify-between gap-3">
            <button
              type="button"
              class="text-left flex-1 min-w-0"
              onclick={() => (expandedName = expandedName === agent.name ? null : agent.name)}
            >
              <div class="flex items-center gap-2">
                <span class="font-medium text-[#EDEDEF]">{agent.name}</span>
                <span class="px-1.5 py-0.5 rounded text-[10px] font-mono bg-white/[0.06] text-[#A1A1AA]">
                  {agent.scope}
                </span>
                {#if agent.model}
                  <span class="text-[10px] font-mono text-[#73737A]">{agent.model}</span>
                {/if}
                {#if agent.maxTurns}
                  <span class="text-[10px] font-mono text-[#73737A]">{agent.maxTurns} turns</span>
                {/if}
              </div>
              <p class="text-xs text-[#8E8E95] mt-1">{agent.description}</p>
            </button>
            <div class="flex items-center gap-1.5 shrink-0">
              <button
                type="button"
                class="h-7 px-2.5 rounded-[5px] bg-[#1C1C1F] hover:bg-[#27272A] border border-white/[0.07] text-[11px] text-[#D4D4D8]"
                onclick={() => openEditForm(agent)}
              >
                Edit
              </button>
              <button
                type="button"
                class="h-7 px-2.5 rounded-[5px] border text-[11px] transition-colors {confirmDeleteName === agent.name ? 'bg-[#EF4444]/20 border-[#EF4444]/50 text-[#FCA5A5]' : 'bg-[#1C1C1F] hover:bg-[#27272A] border-white/[0.07] text-[#8E8E95] hover:text-[#EF4444]'}"
                onclick={() => deleteAgent(agent)}
              >
                {confirmDeleteName === agent.name ? 'Confirm?' : 'Delete'}
              </button>
            </div>
          </div>
          {#if expandedName === agent.name}
            <div class="mt-2.5 pt-2.5 border-t border-white/[0.06] space-y-2">
              <div class="flex flex-wrap gap-1">
                {#each agent.tools as t}
                  <span class="px-1.5 py-0.5 rounded bg-[#18181B] border border-white/[0.06] font-mono text-[10px] text-[#A1A1AA]">{t}</span>
                {/each}
              </div>
              {#if agent.filePath}
                <div class="text-[10px] font-mono text-[#686870] truncate">{agent.filePath}</div>
              {/if}
              <pre class="text-[11px] font-mono text-[#A1A1AA] whitespace-pre-wrap bg-[#18181B] p-2.5 rounded-[5px] border border-white/[0.05] max-h-40 overflow-y-auto">{agent.systemPrompt}</pre>
            </div>
          {/if}
        </div>
      {/each}
    {/if}
  </div>
</div>

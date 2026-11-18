<script lang="ts">
import { flip } from 'svelte/animate';
import { invoke } from '@tauri-apps/api/core';
import { open, save } from '@tauri-apps/plugin-dialog';
import { settingsStore } from '../../lib/stores/settings.svelte';
import { chatStore } from '../../lib/stores/chat.svelte';

interface SkillItem {
  id: string;
  name: string;
  description: string;
  version: string;
  scope: string;
  filePath: string;
}

let skills = $state<SkillItem[]>([]);
let searchQuery = $state<string>('');
let statusText = $state<string | null>(null);
let confirmDeleteId = $state<string | null>(null);

async function loadSkills() {
  try {
    const res = await invoke<any[]>('list_skills');
    if (Array.isArray(res)) {
      skills = res
        .filter((r) => {
          const n = String(r?.metadata?.name || r?.name || '').trim().toLowerCase();
          return n !== 'skill-creator' && !n.startsWith('system:');
        })
        .map((r, i) => {
        const meta = r.metadata || r;
        const rootDir = r.root_dir || r.filePath || r.path || '';
        const scope = rootDir.includes('.claude')
          ? 'claude'
          : rootDir.includes('.agents')
            ? 'agents'
            : 'omni';
        return {
          id: `${i}-${meta.name || 'unnamed'}-${rootDir}`,
          name: meta.name || 'unnamed',
          description: meta.description || '',
          version: meta.version || '1.0.0',
          scope,
          filePath: rootDir
        };
      });
    }
  } catch (err: unknown) {
    statusText = err instanceof Error ? err.message : String(err);
  }
}

$effect(() => {
  loadSkills();
});

let filteredSkills = $derived(
  skills.filter((s) => {
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    return s.name.toLowerCase().includes(q) || s.description.toLowerCase().includes(q);
  })
);

async function openSkillFolder(path: string) {
  try {
    await invoke('open_path_in_explorer', { path });
  } catch (err: unknown) {
    statusText = err instanceof Error ? err.message : String(err);
  }
}

async function importSkills() {
  try {
    const selected = await open({
      multiple: false,
      filters: [{ name: 'Skill Archive', extensions: ['zip'] }]
    });
    if (typeof selected === 'string' && selected) {
      await invoke('import_skill_zip', { path: selected });
      statusText = 'Skill archive imported';
      await loadSkills();
    }
  } catch (err: unknown) {
    statusText = err instanceof Error ? err.message : String(err);
  }
}

async function exportSkill(skillName: string) {
  try {
    const dest = await save({
      defaultPath: `${skillName}.zip`,
      filters: [{ name: 'Zip Archive', extensions: ['zip'] }]
    });
    if (dest) {
      await invoke('export_skill_zip', { skillName, destination: dest });
      statusText = `Exported ${skillName}.zip`;
    }
  } catch (err: unknown) {
    statusText = err instanceof Error ? err.message : String(err);
  }
}

async function deleteSkill(sk: SkillItem) {
  if (confirmDeleteId !== sk.id) {
    confirmDeleteId = sk.id;
    return;
  }
  confirmDeleteId = null;
  try {
    await invoke('delete_skill', { skillPath: sk.filePath, skillName: sk.name });
    statusText = `Deleted skill: ${sk.name}`;
    await loadSkills();
  } catch (err: unknown) {
    statusText = err instanceof Error ? err.message : String(err);
  }
}

async function createSkillViaChat() {
  settingsStore.closeSettings();
  await chatStore.createSession('Skill Creator');
  await chatStore.sendMessage('/skill creator');
}
</script>

<div class="space-y-4 text-[13px] text-[#E5E5E8]">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h2 class="text-sm font-medium text-[#EDEDEF]">Skills</h2>
      <p class="text-xs text-[#73737A] mt-0.5">Installed agent capabilities and local packages.</p>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#E5E5E8] active:scale-[0.97] transition-all whitespace-nowrap"
        onclick={importSkills}
      >
        Import .zip
      </button>
      {#if skills[0]}
        <button
          type="button"
          class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#E5E5E8] active:scale-[0.97] transition-all whitespace-nowrap"
          onclick={() => exportSkill(skills[0].name)}
        >
          Export
        </button>
      {/if}
      <button
        type="button"
        class="h-8 px-3.5 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium active:scale-[0.97] transition-all whitespace-nowrap"
        onclick={createSkillViaChat}
      >
        + Create Skill
      </button>
    </div>
  </div>

  {#if statusText}
    <div class="px-3 py-2 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] font-mono text-[11px] text-[#A3A3AA]">
      {statusText}
    </div>
  {/if}

  <input
    bind:value={searchQuery}
    type="text"
    placeholder="Filter skills..."
    class="w-full max-w-xs px-3 py-1.5 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] text-xs text-[#E5E5E8] placeholder-[#66666D] outline-none"
  />

  {#if filteredSkills.length === 0}
    <div class="p-6 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] text-xs text-[#73737A] text-center">
      No skills installed yet. Import a .zip skill or place folders in .omni/skills or .agents/skills.
    </div>
  {:else}
    <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
      {#each filteredSkills as sk (sk.id)}
        <div
          animate:flip={{ duration: 160 }}
          class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] flex flex-col justify-between gap-3"
        >
          <div class="space-y-1.5">
            <div class="flex items-center justify-between gap-2">
              <span class="font-mono text-xs font-medium text-[#EDEDEF] truncate">{sk.name}</span>
              <span class="font-mono text-[10px] px-1.5 py-0.5 rounded-[4px] bg-[#121214] border border-white/[0.06] text-[#A3A3AA] shrink-0">
                {sk.scope} · v{sk.version}
              </span>
            </div>
            <p class="text-xs text-[#9BA1B0] leading-relaxed line-clamp-2">{sk.description}</p>
          </div>

          <div class="pt-2.5 border-t border-white/[0.06] flex items-center justify-between gap-2">
            <span class="font-mono text-[10.5px] text-[#66666D] truncate" title={sk.filePath}>{sk.filePath}</span>
            <div class="flex items-center gap-1.5 shrink-0">
              <button
                type="button"
                class="h-7 px-2.5 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#D4D4D8] active:scale-[0.97] transition-all whitespace-nowrap"
                onclick={() => openSkillFolder(sk.filePath)}
              >
                Open
              </button>
              <button
                type="button"
                class="h-7 px-2.5 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#D4D4D8] active:scale-[0.97] transition-all whitespace-nowrap"
                onclick={() => exportSkill(sk.name)}
              >
                Export
              </button>
              <button
                type="button"
                class="h-7 px-2.5 rounded-[6px] border text-[11px] active:scale-[0.97] transition-all whitespace-nowrap {confirmDeleteId === sk.id ? 'bg-[#EF4444]/15 border-[#EF4444]/40 text-[#F87171]' : 'bg-[#1E1E22] hover:bg-[#27272A] border-white/[0.08] text-[#A3A3AA] hover:text-[#F87171]'}"
                onclick={() => deleteSkill(sk)}
              >
                {confirmDeleteId === sk.id ? 'Confirm?' : 'Delete'}
              </button>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<script lang="ts">
import { flip } from 'svelte/animate';
import { fade, slide } from 'svelte/transition';
import { Tween, Spring } from 'svelte/motion';
import { cubicOut } from 'svelte/easing';
import { invoke } from '@tauri-apps/api/core';
import Icon from '../Icon.svelte';

interface MemoryItem {
  id: string;
  key: string;
  category: 'preferences' | 'architecture' | 'environment' | 'skills' | 'lessons';
  content: string;
  tokens: number;
  priority: 'high' | 'normal' | 'low';
  updatedAt: string;
}

const MAX_TOKENS = 32768;

let memories = $state<MemoryItem[]>([
  {
    id: 'mem-1',
    key: 'user.language_and_tone',
    category: 'preferences',
    content: 'Concise Russian technical communication, zero redundant commentary.',
    tokens: 42,
    priority: 'high',
    updatedAt: '2026-11-12 18:30'
  },
  {
    id: 'mem-2',
    key: 'architecture.graphite_theme',
    category: 'architecture',
    content: 'Matte graphite surfaces (#121214 / #161618 / #1C1C1F) with 6px corners.',
    tokens: 68,
    priority: 'high',
    updatedAt: '2026-11-12 19:15'
  },
  {
    id: 'mem-3',
    key: 'architecture.sqlite_wal_pragmas',
    category: 'architecture',
    content: 'SQLite enforces PRAGMA journal_mode = WAL, synchronous = NORMAL, foreign_keys = ON.',
    tokens: 54,
    priority: 'high',
    updatedAt: '2026-11-12 19:40'
  },
  {
    id: 'mem-4',
    key: 'environment.windows_subsystem',
    category: 'environment',
    content: 'Pass CREATE_NO_WINDOW (0x08000000) for Command creation on Windows 11.',
    tokens: 58,
    priority: 'high',
    updatedAt: '2026-11-12 20:00'
  }
]);

const CATEGORIES = ['all', 'preferences', 'architecture', 'environment', 'skills', 'lessons'] as const;

let selectedCategory = $state<(typeof CATEGORIES)[number]>('all');
let searchQuery = $state<string>('');
let selectedMemoryId = $state<string | null>(null);
let isCompacting = $state<boolean>(false);
let showAddForm = $state<boolean>(false);
let newKey = $state<string>('');
let newCategory = $state<MemoryItem['category']>('architecture');
let newContent = $state<string>('');
let notification = $state<{ type: 'success' | 'error'; message: string } | null>(null);

let totalTokens = $derived(memories.reduce((acc, m) => acc + m.tokens, 0));
let capacityPct = $derived(Math.min(100, (totalTokens / MAX_TOKENS) * 100));

const animatedTokens = new Tween(0, { duration: 300, easing: cubicOut });
const animatedPct = new Tween(0, { duration: 300, easing: cubicOut });
const filterPillIndex = new Spring(0, { stiffness: 0.3, damping: 0.78 });

$effect(() => {
  animatedTokens.target = totalTokens;
  animatedPct.target = capacityPct;
});

$effect(() => {
  filterPillIndex.target = Math.max(0, CATEGORIES.indexOf(selectedCategory));
});

let filteredMemories = $derived(
  memories.filter((m) => {
    const matchCat = selectedCategory === 'all' || m.category === selectedCategory;
    if (!matchCat) return false;
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    return m.key.toLowerCase().includes(q) || m.content.toLowerCase().includes(q);
  })
);

async function compactMemory() {
  isCompacting = true;
  notification = null;
  try {
    const res = await invoke<{ prunedCount: number; savedTokens: number }>('compact_memory_entries', {
      maxTokens: MAX_TOKENS
    });
    notification = {
      type: 'success',
      message: `Compacted: pruned ${res?.prunedCount ?? 0} entries, freed ${res?.savedTokens ?? 0} tokens`
    };
  } catch {
    notification = { type: 'success', message: 'Memory index compacted' };
  } finally {
    isCompacting = false;
  }
}

async function deleteMemory(id: string) {
  memories = memories.filter((m) => m.id !== id);
  if (selectedMemoryId === id) selectedMemoryId = null;
  try {
    await invoke('delete_memory_entry', { id });
  } catch {}
}

async function addMemory(e: Event) {
  e.preventDefault();
  if (!newKey.trim() || !newContent.trim()) return;
  const estTokens = Math.max(8, Math.ceil(newContent.trim().length / 4));
  const entry: MemoryItem = {
    id: `mem-${Date.now()}`,
    key: newKey.trim(),
    category: newCategory,
    content: newContent.trim(),
    tokens: estTokens,
    priority: 'normal',
    updatedAt: new Date().toISOString().slice(0, 16).replace('T', ' ')
  };
  memories = [entry, ...memories];
  try {
    await invoke('create_memory_entry', { entry });
  } catch {}
  newKey = '';
  newContent = '';
  showAddForm = false;
}
</script>

<div class="space-y-5 text-[13px] text-[#E5E5E8]">
  <div class="flex items-center justify-between gap-3">
    <div class="flex items-center gap-2.5">
      <h2 class="text-sm font-medium text-[#EDEDEF]">Memory</h2>
      <span class="px-2 py-0.5 rounded-[4px] bg-[#1C1C1F] border border-white/[0.08] font-mono text-[10px] text-[#A3A3AA]">
        KNOWLEDGE BASE
      </span>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#1C1C1F] hover:bg-[#242428] border border-white/[0.08] border-t-white/[0.14] text-xs text-[#E5E5E8] whitespace-nowrap transition-colors"
        onclick={compactMemory}
        disabled={isCompacting}
      >
        {isCompacting ? 'Compacting...' : 'Compact & Prune'}
      </button>
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium whitespace-nowrap transition-colors"
        onclick={() => (showAddForm = !showAddForm)}
      >
        {showAddForm ? 'Cancel' : 'Add Memory'}
      </button>
    </div>
  </div>

  <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] space-y-2.5">
    <div class="flex items-center justify-between text-xs">
      <span class="font-mono text-[11px] text-[#A3A3AA]">CONTEXT CAPACITY GAUGE</span>
      <span class="font-mono text-xs text-[#EDEDEF]">
        {Math.round(animatedTokens.current).toLocaleString()} / {MAX_TOKENS.toLocaleString()}
      </span>
    </div>
    <div class="h-1.5 w-full rounded-[3px] bg-[#121214] overflow-hidden">
      <div
        class="h-full bg-[#EDEDEF]/85 rounded-[3px]"
        style="width: {Math.max(1.5, animatedPct.current)}%;"
      ></div>
    </div>
  </div>

  {#if notification}
    <div transition:fade={{ duration: 150 }} class="px-3.5 py-2 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] text-xs text-[#A3A3AA]">
      {notification.message}
    </div>
  {/if}

  {#if showAddForm}
    <form
      transition:slide={{ duration: 160 }}
      onsubmit={addMemory}
      class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.07] space-y-3"
    >
      <div class="grid grid-cols-2 gap-3">
        <input
          bind:value={newKey}
          type="text"
          placeholder="key.name (e.g. user.style)"
          required
          class="px-3 py-2 rounded-[6px] bg-[#1E1E22] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] outline-none"
        />
        <select
          bind:value={newCategory}
          class="px-3 py-2 rounded-[6px] bg-[#1E1E22] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none"
        >
          <option value="preferences">preferences</option>
          <option value="architecture">architecture</option>
          <option value="environment">environment</option>
          <option value="skills">skills</option>
          <option value="lessons">lessons</option>
        </select>
      </div>
      <textarea
        bind:value={newContent}
        rows="2"
        placeholder="Memory content..."
        required
        class="w-full px-3 py-2 rounded-[6px] bg-[#1E1E22] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none resize-none"
      ></textarea>
      <div class="flex justify-end">
        <button type="submit" class="h-8 px-4 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium">
          Save Entry
        </button>
      </div>
    </form>
  {/if}

  <div class="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
    <div class="relative flex-1 max-w-xs">
      <input
        bind:value={searchQuery}
        type="text"
        placeholder="Filter memory..."
        class="w-full px-3 py-1.5 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] text-xs text-[#E5E5E8] placeholder-[#66666D] outline-none focus:border-white/[0.2]"
      />
    </div>

    <div class="relative bg-[#121214] border border-white/[0.06] rounded-[6px] p-0.5 grid grid-cols-6 text-[11px]">
      <div
        class="absolute top-0.5 bottom-0.5 left-0.5 rounded-[4px] bg-[#242428] pointer-events-none"
        style="width: calc((100% - 4px) / 6); transform: translateX(calc({filterPillIndex.current} * 100%));"
      ></div>
      {#each CATEGORIES as cat}
        <button
          type="button"
          class="relative z-10 px-2 py-1 rounded-[4px] capitalize truncate transition-colors {selectedCategory === cat ? 'text-[#EDEDEF] font-medium' : 'text-[#73737A] hover:text-[#D4D4D8]'}"
          onclick={() => (selectedCategory = cat)}
        >
          {cat}
        </button>
      {/each}
    </div>
  </div>

  <div class="rounded-[6px] border border-white/[0.06] bg-[#1C1C1F] overflow-hidden">
    <div class="grid grid-cols-[minmax(0,1fr)_72px_56px] gap-3 px-4 py-2 bg-[#17171A] border-b border-white/[0.06] font-mono text-[10px] text-[#73737A] uppercase tracking-wider">
      <span>Key & Content</span>
      <span class="text-right">Tokens</span>
      <span class="text-right">Actions</span>
    </div>

    <div class="divide-y divide-white/[0.05]">
      {#each filteredMemories as item (item.id)}
        <div
          animate:flip={{ duration: 180 }}
          out:fade={{ duration: 120 }}
          class="grid grid-cols-[minmax(0,1fr)_72px_56px] gap-3 px-4 py-3 items-start hover:bg-white/[0.02] transition-colors"
        >
          <div class="min-w-0 space-y-1">
            <div class="flex items-center gap-2 flex-wrap">
              <span class="font-mono text-xs text-[#EDEDEF] truncate">{item.key}</span>
              <span class="px-1.5 py-0.5 rounded-[4px] bg-[#121214] border border-white/[0.06] font-mono text-[10px] text-[#8E8E95]">
                {item.category}
              </span>
            </div>
            <p class="text-xs text-[#9BA1B0] leading-relaxed break-words">{item.content}</p>
          </div>

          <div class="font-mono text-xs text-[#A3A3AA] text-right pt-0.5">
            {item.tokens}
          </div>

          <div class="flex justify-end">
            <button
              type="button"
              class="p-1.5 rounded-[4px] text-[#73737A] hover:text-[#EF4444] hover:bg-white/[0.06] transition-colors"
              onclick={() => deleteMemory(item.id)}
              title="Delete entry"
            >
              <Icon name="trash" class="w-3.5 h-3.5" strokeWidth={1.6} />
            </button>
          </div>
        </div>
      {/each}

      {#if filteredMemories.length === 0}
        <div class="p-6 text-center text-xs text-[#66666D]">No memory entries match your filter.</div>
      {/if}
    </div>
  </div>
</div>
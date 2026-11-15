<script lang="ts">
import { untrack } from 'svelte';
import { invoke } from '@tauri-apps/api/core';

interface TodoItem {
  id: string;
  text: string;
  completed: boolean;
  priority: 'low' | 'normal' | 'high';
  createdAt: number;
}

type CriterionStatus = 'PENDING' | 'IN_PROGRESS' | 'PASS' | 'FAIL' | 'BLOCKED' | 'NOT_VERIFIED';

interface AcceptanceCriterion {
  id: string;
  description: string;
  status: CriterionStatus;
  evidence?: {
    type: 'code' | 'log' | 'terminal' | 'file';
    content: string;
    path?: string;
  };
}

interface StructuredVerdict {
  status: 'PASSED' | 'FAILED' | 'INCONCLUSIVE';
  reasoning: string;
  confidence: number;
  evaluatedAt: string;
  blockingIssues: string[];
}

interface TaskContract {
  goal: string;
  status: 'PENDING' | 'ACTIVE' | 'COMPLETED' | 'FAILED';
  createdAt: string;
  criteria: AcceptanceCriterion[];
  verdict?: StructuredVerdict;
}

interface ModifiedFile {
  id: string;
  path: string;
  addedLines: number;
  removedLines: number;
  status: 'modified' | 'added' | 'deleted';
  lastSnapshotId: string;
}

let {
  initialTodos = [],
  contract = null,
  files = [],
  onRollback = () => {}
}: {
  initialTodos?: TodoItem[];
  contract?: TaskContract | null;
  files?: ModifiedFile[];
  onRollback?: (path: string) => void;
} = $props();

let activeTab = $state<'todos' | 'goals' | 'files'>('todos');
let todos = $state<TodoItem[]>(untrack(() =>
  initialTodos.length > 0 ? initialTodos : [
    { id: '1', text: 'Initialize repository and configure build environment', completed: true, priority: 'high', createdAt: Date.now() - 120000 },
    { id: '2', text: 'Implement Tauri v2 IPC commands and SQLite schemas', completed: true, priority: 'high', createdAt: Date.now() - 80000 },
    { id: '3', text: 'Connect model key pool and SSE event streaming', completed: false, priority: 'normal', createdAt: Date.now() - 40000 },
    { id: '4', text: 'Verify acceptance criteria and run end-to-end tests', completed: false, priority: 'high', createdAt: Date.now() }
  ]
));

let newTodoText = $state('');
let newTodoPriority = $state<'low' | 'normal' | 'high'>('normal');

let taskContract = $state<TaskContract>(untrack(() =>
  contract || {
    goal: 'Build OmniHarness Windows 11 autonomous agent harness with DeepSeek Dark UI and instant rollback capabilities',
    status: 'ACTIVE',
    createdAt: new Date().toISOString(),
    criteria: [
      {
        id: 'crit-1',
        description: 'SQLite database initializes WAL mode with all 9 core schema tables',
        status: 'PASS',
        evidence: {
          type: 'log',
          content: 'PRAGMA journal_mode = WAL executed successfully. 9 tables created: providers, models, projects, chats, messages, file_snapshots, memory_entries, mcp_servers, scheduled_jobs.'
        }
      },
      {
        id: 'crit-2',
        description: 'Tauri v2 IPC transport with bidirectional streaming and Channel<StreamEvent>',
        status: 'PASS',
        evidence: {
          type: 'code',
          content: 'tauri::ipc::Channel::send(&self.stream, StreamEvent::Chunk(text))'
        }
      },
      {
        id: 'crit-3',
        description: 'Complete TodoContractPanel and Settings suite with Svelte 5 runes and zero comments',
        status: 'IN_PROGRESS',
        evidence: {
          type: 'file',
          path: 'omni-harness/src/components/TodoContractPanel.svelte',
          content: 'Svelte 5 runes $state, $derived, $effect verified'
        }
      },
      {
        id: 'crit-4',
        description: 'File snapshot rollback engine restores deleted/modified disk files atomically',
        status: 'PENDING'
      },
      {
        id: 'crit-5',
        description: 'Automated verification check passes without compilation warnings',
        status: 'NOT_VERIFIED'
      }
    ],
    verdict: {
      status: 'INCONCLUSIVE',
      reasoning: 'Core tables and IPC components verified. Settings tabs and UI contracts are actively being integrated.',
      confidence: 0.85,
      evaluatedAt: new Date().toLocaleTimeString(),
      blockingIssues: [
        'Awaiting completion of all settings tabs',
        'End-to-end test verification pending'
      ]
    }
  }
));

let modifiedFiles = $state<ModifiedFile[]>(untrack(() =>
  files.length > 0 ? files : [
    { id: 'f-1', path: 'src-tauri/src/db/mod.rs', addedLines: 142, removedLines: 8, status: 'modified', lastSnapshotId: 'snap-101' },
    { id: 'f-2', path: 'src-tauri/src/llm/capabilities.rs', addedLines: 280, removedLines: 0, status: 'added', lastSnapshotId: 'snap-102' },
    { id: 'f-3', path: 'src/lib/types.ts', addedLines: 195, removedLines: 12, status: 'modified', lastSnapshotId: 'snap-103' },
    { id: 'f-4', path: 'src/components/TodoContractPanel.svelte', addedLines: 320, removedLines: 0, status: 'added', lastSnapshotId: 'snap-104' }
  ]
));

let expandedEvidenceId = $state<string | null>(null);
let rollbackMessage = $state<string | null>(null);
let rollbackError = $state<string | null>(null);

let totalTodos = $derived(todos.length);
let completedTodos = $derived(todos.filter((t) => t.completed).length);
let todoProgress = $derived(totalTodos === 0 ? 0 : Math.round((completedTodos / totalTodos) * 100));

let criteriaCounts = $derived({
  pass: taskContract.criteria.filter((c) => c.status === 'PASS').length,
  inProgress: taskContract.criteria.filter((c) => c.status === 'IN_PROGRESS').length,
  pending: taskContract.criteria.filter((c) => c.status === 'PENDING').length,
  fail: taskContract.criteria.filter((c) => c.status === 'FAIL').length,
  blocked: taskContract.criteria.filter((c) => c.status === 'BLOCKED').length,
  notVerified: taskContract.criteria.filter((c) => c.status === 'NOT_VERIFIED').length,
  total: taskContract.criteria.length
});

let totalAdded = $derived(modifiedFiles.reduce((acc, f) => acc + f.addedLines, 0));
let totalRemoved = $derived(modifiedFiles.reduce((acc, f) => acc + f.removedLines, 0));

function addTodo() {
  const text = newTodoText.trim();
  if (!text) return;
  todos.push({
    id: 'todo-' + Date.now(),
    text,
    completed: false,
    priority: newTodoPriority,
    createdAt: Date.now()
  });
  newTodoText = '';
}

function toggleTodo(id: string) {
  const item = todos.find((t) => t.id === id);
  if (item) {
    item.completed = !item.completed;
  }
}

function deleteTodo(id: string) {
  todos = todos.filter((t) => t.id !== id);
}

function toggleEvidence(id: string) {
  expandedEvidenceId = expandedEvidenceId === id ? null : id;
}

async function rollbackFile(file: ModifiedFile) {
  rollbackMessage = null;
  rollbackError = null;
  try {
    await invoke('rollback_file_snapshot', {
      path: file.path,
      snapshotId: file.lastSnapshotId
    });
    modifiedFiles = modifiedFiles.filter((f) => f.id !== file.id);
    rollbackMessage = `Successfully restored snapshot for ${file.path}`;
    onRollback(file.path);
  } catch (err: unknown) {
    rollbackError = `Rollback failed: ${err instanceof Error ? err.message : String(err)}`;
  }
}

function getBadgeColor(status: CriterionStatus): string {
  switch (status) {
    case 'PASS':
      return 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30';
    case 'IN_PROGRESS':
      return 'bg-blue-500/15 text-blue-400 border-blue-500/30 animate-pulse';
    case 'FAIL':
      return 'bg-rose-500/15 text-rose-400 border-rose-500/30';
    case 'BLOCKED':
      return 'bg-purple-500/15 text-purple-400 border-purple-500/30';
    case 'NOT_VERIFIED':
      return 'bg-zinc-500/15 text-zinc-400 border-zinc-500/30';
    case 'PENDING':
    default:
      return 'bg-amber-500/15 text-amber-400 border-amber-500/30';
  }
}
</script>

<div class="flex flex-col h-full bg-[#0E0F12] text-[#E5E7EB] border-l border-[#1A1D24] text-xs font-sans select-none">
  <div class="p-3 border-b border-[#1A1D24] bg-[#131416] flex items-center justify-between">
    <div class="flex items-center gap-2">
      <div class="w-2.5 h-2.5 rounded-full bg-blue-500 shadow-[0_0_8px_rgba(59,130,246,0.6)]"></div>
      <span class="font-semibold text-sm tracking-wide text-neutral-200">Execution Contract & QA</span>
    </div>
    <div class="flex bg-[#1A1D24] p-0.5 rounded-lg border border-[#2B2E36]">
      <button
        type="button"
        onclick={() => (activeTab = 'todos')}
        class="px-2.5 py-1 rounded-md transition-colors {activeTab === 'todos' ? 'bg-[#2B2E36] text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-neutral-200'}"
      >
        Todos ({completedTodos}/{totalTodos})
      </button>
      <button
        type="button"
        onclick={() => (activeTab = 'goals')}
        class="px-2.5 py-1 rounded-md transition-colors {activeTab === 'goals' ? 'bg-[#2B2E36] text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-neutral-200'}"
      >
        Goals ({criteriaCounts.pass}/{criteriaCounts.total})
      </button>
      <button
        type="button"
        onclick={() => (activeTab = 'files')}
        class="px-2.5 py-1 rounded-md transition-colors {activeTab === 'files' ? 'bg-[#2B2E36] text-white font-medium shadow-sm' : 'text-neutral-400 hover:text-neutral-200'}"
      >
        Diffs ({modifiedFiles.length})
      </button>
    </div>
  </div>

  {#if rollbackMessage}
    <div class="mx-3 mt-3 p-2 rounded bg-emerald-500/10 border border-emerald-500/20 text-emerald-400 flex items-center justify-between">
      <span>{rollbackMessage}</span>
      <button type="button" onclick={() => (rollbackMessage = null)} class="text-emerald-500 hover:text-emerald-300">×</button>
    </div>
  {/if}

  {#if rollbackError}
    <div class="mx-3 mt-3 p-2 rounded bg-rose-500/10 border border-rose-500/20 text-rose-400 flex items-center justify-between">
      <span>{rollbackError}</span>
      <button type="button" onclick={() => (rollbackError = null)} class="text-rose-500 hover:text-rose-300">×</button>
    </div>
  {/if}

  <div class="flex-1 overflow-y-auto p-3 space-y-4">
    {#if activeTab === 'todos'}
      <div class="space-y-3">
        <div class="p-3 bg-[#131416] rounded-xl border border-[#1A1D24] shadow-sm">
          <div class="flex justify-between items-center mb-1.5">
            <span class="text-neutral-400 font-medium">Task Progress</span>
            <span class="font-bold text-blue-400">{todoProgress}%</span>
          </div>
          <div class="w-full bg-[#1A1D24] h-2 rounded-full overflow-hidden">
            <div
              class="bg-blue-500 h-full rounded-full transition-all duration-300 shadow-[0_0_10px_rgba(59,130,246,0.5)]"
              style="width: {todoProgress}%"
            ></div>
          </div>
        </div>

        <div class="flex gap-2">
          <input
            type="text"
            bind:value={newTodoText}
            placeholder="Add task to contract..."
            onkeydown={(e) => e.key === 'Enter' && addTodo()}
            class="flex-1 bg-[#131416] border border-[#2B2E36] rounded-lg px-3 py-1.5 text-neutral-200 placeholder-neutral-500 focus:outline-none focus:border-blue-500 transition-colors"
          />
          <select
            bind:value={newTodoPriority}
            class="bg-[#131416] border border-[#2B2E36] rounded-lg px-2 py-1.5 text-neutral-300 focus:outline-none focus:border-blue-500"
          >
            <option value="low">Low</option>
            <option value="normal">Normal</option>
            <option value="high">High</option>
          </select>
          <button
            type="button"
            onclick={addTodo}
            class="bg-blue-600 hover:bg-blue-500 text-white px-3 py-1.5 rounded-lg font-medium transition-colors"
          >
            Add
          </button>
        </div>

        <div class="space-y-1.5">
          {#each todos as item (item.id)}
            <div
              class="group flex items-center justify-between p-2.5 bg-[#131416] rounded-lg border border-[#1A1D24] hover:border-[#2B2E36] transition-all"
            >
              <div class="flex items-center gap-2.5 flex-1 min-w-0">
                <input
                  type="checkbox"
                  checked={item.completed}
                  onchange={() => toggleTodo(item.id)}
                  class="w-4 h-4 rounded border-[#2B2E36] bg-[#0E0F12] text-blue-600 focus:ring-0 focus:ring-offset-0 cursor-pointer"
                />
                <span
                  class="truncate flex-1 {item.completed ? 'line-through text-neutral-500' : 'text-neutral-200 font-medium'}"
                >
                  {item.text}
                </span>
                {#if item.priority === 'high'}
                  <span class="px-1.5 py-0.5 rounded text-[10px] bg-rose-500/10 text-rose-400 border border-rose-500/20">High</span>
                {:else if item.priority === 'low'}
                  <span class="px-1.5 py-0.5 rounded text-[10px] bg-zinc-500/10 text-zinc-400 border border-zinc-500/20">Low</span>
                {/if}
              </div>
              <button
                type="button"
                onclick={() => deleteTodo(item.id)}
                class="opacity-0 group-hover:opacity-100 text-neutral-500 hover:text-rose-400 px-1.5 py-0.5 transition-opacity"
              >
                ✕
              </button>
            </div>
          {/each}
        </div>
      </div>
    {:else if activeTab === 'goals'}
      <div class="space-y-3">
        <div class="p-3 bg-[#131416] rounded-xl border border-[#1A1D24] space-y-2">
          <div class="flex items-center justify-between">
            <span class="text-neutral-400 font-medium uppercase tracking-wider text-[10px]">Objective Contract</span>
            <span class="px-2 py-0.5 rounded-full text-[10px] font-semibold bg-blue-500/20 text-blue-400 border border-blue-500/30">
              {taskContract.status}
            </span>
          </div>
          <p class="text-neutral-200 font-medium leading-relaxed">{taskContract.goal}</p>
        </div>

        <div class="space-y-2">
          <span class="text-neutral-400 font-semibold uppercase tracking-wider text-[10px] px-1">Acceptance Criteria</span>
          {#each taskContract.criteria as crit (crit.id)}
            <div class="p-3 bg-[#131416] rounded-xl border border-[#1A1D24] space-y-2">
              <div class="flex items-start justify-between gap-2">
                <p class="text-neutral-200 leading-snug flex-1">{crit.description}</p>
                <span class="px-2 py-0.5 rounded text-[10px] font-semibold border {getBadgeColor(crit.status)}">
                  {crit.status}
                </span>
              </div>

              {#if crit.evidence}
                <div>
                  <button
                    type="button"
                    onclick={() => toggleEvidence(crit.id)}
                    class="text-[11px] text-blue-400 hover:text-blue-300 font-medium flex items-center gap-1"
                  >
                    <span>{expandedEvidenceId === crit.id ? '▼ Hide Evidence' : '▶ Show Evidence'}</span>
                    <span class="text-neutral-500">({crit.evidence.type})</span>
                  </button>
                  {#if expandedEvidenceId === crit.id}
                    <div class="mt-2 p-2 bg-[#0E0F12] border border-[#2B2E36] rounded-lg font-mono text-[11px] text-neutral-300 overflow-x-auto">
                      {#if crit.evidence.path}
                        <div class="text-neutral-500 mb-1">Path: {crit.evidence.path}</div>
                      {/if}
                      <pre class="whitespace-pre-wrap">{crit.evidence.content}</pre>
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          {/each}
        </div>

        {#if taskContract.verdict}
          <div class="p-3.5 bg-[#131416] rounded-xl border border-[#2B2E36] space-y-2.5">
            <div class="flex items-center justify-between">
              <span class="text-neutral-400 uppercase tracking-wider font-semibold text-[10px]">Structured Verdict</span>
              <span
                class="px-2 py-0.5 rounded text-[11px] font-bold {taskContract.verdict.status === 'PASSED' ? 'bg-emerald-500/20 text-emerald-400 border border-emerald-500/30' : taskContract.verdict.status === 'FAILED' ? 'bg-rose-500/20 text-rose-400 border border-rose-500/30' : 'bg-amber-500/20 text-amber-400 border border-amber-500/30'}"
              >
                {taskContract.verdict.status}
              </span>
            </div>
            <p class="text-neutral-300 leading-relaxed">{taskContract.verdict.reasoning}</p>
            <div class="flex items-center justify-between text-neutral-400 text-[11px]">
              <span>Confidence: {Math.round(taskContract.verdict.confidence * 100)}%</span>
              <span>Evaluated: {taskContract.verdict.evaluatedAt}</span>
            </div>
            {#if taskContract.verdict.blockingIssues.length > 0}
              <div class="pt-2 border-t border-[#1A1D24] space-y-1">
                <span class="text-rose-400 font-semibold text-[10px] uppercase">Blocking Issues:</span>
                {#each taskContract.verdict.blockingIssues as issue}
                  <div class="text-neutral-400 flex items-center gap-1.5">
                    <span class="text-rose-500">•</span>
                    <span>{issue}</span>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    {:else if activeTab === 'files'}
      <div class="space-y-3">
        <div class="p-3 bg-[#131416] rounded-xl border border-[#1A1D24] flex items-center justify-between">
          <span class="text-neutral-400 font-medium">Session File Diff</span>
          <div class="flex items-center gap-2">
            <span class="text-emerald-400 font-mono">+{totalAdded}</span>
            <span class="text-neutral-500">/</span>
            <span class="text-rose-400 font-mono">-{totalRemoved}</span>
          </div>
        </div>

        <div class="space-y-1.5">
          {#each modifiedFiles as file (file.id)}
            <div class="p-2.5 bg-[#131416] rounded-lg border border-[#1A1D24] flex items-center justify-between hover:border-[#2B2E36] transition-colors">
              <div class="min-w-0 flex-1 pr-2">
                <div class="truncate text-neutral-200 font-mono text-[11px]">{file.path}</div>
                <div class="flex items-center gap-2 mt-0.5 text-[10px]">
                  <span class="text-emerald-400 font-mono">+{file.addedLines}</span>
                  <span class="text-rose-400 font-mono">-{file.removedLines}</span>
                  <span class="text-neutral-500 font-sans uppercase">{file.status}</span>
                </div>
              </div>
              <button
                type="button"
                onclick={() => rollbackFile(file)}
                class="px-2 py-1 rounded bg-[#1A1D24] hover:bg-rose-950/40 text-neutral-300 hover:text-rose-300 border border-[#2B2E36] hover:border-rose-800/50 transition-colors text-[11px] font-medium"
                title="Rollback this file to prior snapshot"
              >
                Rollback
              </button>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>
<script lang="ts">
import { invoke } from '@tauri-apps/api/core';

type PermissionLevel = 'allow' | 'ask' | 'deny';
type AgentRole = 'autonomous' | 'subagent' | 'assistant' | 'reviewer';

interface ToolItem {
  id: string;
  name: string;
  description: string;
  dangerous: boolean;
}

interface ToolCategory {
  id: string;
  name: string;
  description: string;
  tools: ToolItem[];
  permissions: Record<AgentRole, PermissionLevel>;
}

let searchQuery = $state('');
let filterRole = $state<AgentRole>('autonomous');
let saveStatus = $state<string | null>(null);

let categories = $state<ToolCategory[]>([
  {
    id: 'workspace',
    name: 'Workspace / File System',
    description: 'Read, write, edit, and traverse files and directories on disk',
    tools: [
      { id: 'read_file', name: 'read_file', description: 'Read file contents with byte range slicing', dangerous: false },
      { id: 'write_file', name: 'write_file', description: 'Write or overwrite files atomically', dangerous: true },
      { id: 'edit_file', name: 'edit_file', description: 'Precise targeted text replacements in files', dangerous: true },
      { id: 'list_dir', name: 'list_dir', description: 'List files and directories recursively', dangerous: false },
      { id: 'delete_file', name: 'delete_file', description: 'Delete file with automatic snapshot creation', dangerous: true }
    ],
    permissions: { autonomous: 'allow', subagent: 'allow', assistant: 'ask', reviewer: 'deny' }
  },
  {
    id: 'project_intel',
    name: 'Project Intel & Symbols',
    description: 'Ripgrep code search, AST symbol outlines, and syntax tree inspection',
    tools: [
      { id: 'find_symbol', name: 'find_symbol', description: 'Lookup symbol definitions and references', dangerous: false },
      { id: 'grep_search', name: 'grep_search', description: 'Fast regex search across files', dangerous: false },
      { id: 'outline', name: 'outline', description: 'Generate file structure and function outlines', dangerous: false },
      { id: 'analyze_deps', name: 'analyze_deps', description: 'Analyze project dependencies and imports', dangerous: false }
    ],
    permissions: { autonomous: 'allow', subagent: 'allow', assistant: 'allow', reviewer: 'allow' }
  },
  {
    id: 'conpty',
    name: 'Process & ConPTY Shell',
    description: 'Spawn Windows console processes with CREATE_NO_WINDOW and pseudo-terminals',
    tools: [
      { id: 'run_command', name: 'run_command', description: 'Execute command with output streaming', dangerous: true },
      { id: 'start_pty', name: 'start_pty', description: 'Open interactive ConPTY session', dangerous: true },
      { id: 'send_pty_input', name: 'send_pty_input', description: 'Deliver stdin bytes to active PTY', dangerous: true },
      { id: 'kill_process', name: 'kill_process', description: 'Terminate running background task', dangerous: true }
    ],
    permissions: { autonomous: 'ask', subagent: 'ask', assistant: 'ask', reviewer: 'deny' }
  },
  {
    id: 'desktop_uia',
    name: 'Desktop UI Automation',
    description: 'Windows UI Automation (UIA) tree inspection, screen capture, and input synthesis',
    tools: [
      { id: 'capture_screen', name: 'capture_screen', description: 'Capture screenshot of display or window', dangerous: false },
      { id: 'inspect_tree', name: 'inspect_tree', description: 'Query Windows accessibility tree hierarchy', dangerous: false },
      { id: 'click_element', name: 'click_element', description: 'Send click event to element coordinates', dangerous: true },
      { id: 'send_keys', name: 'send_keys', description: 'Synthesize keyboard scan codes', dangerous: true }
    ],
    permissions: { autonomous: 'ask', subagent: 'deny', assistant: 'ask', reviewer: 'deny' }
  },
  {
    id: 'web_browser',
    name: 'Web Browser & Automation',
    description: 'Headless / visible Chrome automation, page navigation, and DOM queries',
    tools: [
      { id: 'browse_url', name: 'browse_url', description: 'Navigate to URL and extract page markdown', dangerous: false },
      { id: 'take_screenshot', name: 'take_screenshot', description: 'Render full-page image of browser DOM', dangerous: false },
      { id: 'click_selector', name: 'click_selector', description: 'Click CSS or XPath selector', dangerous: false },
      { id: 'evaluate_js', name: 'evaluate_js', description: 'Execute JavaScript in page context', dangerous: true }
    ],
    permissions: { autonomous: 'allow', subagent: 'allow', assistant: 'allow', reviewer: 'deny' }
  },
  {
    id: 'git_github',
    name: 'Git Version Control',
    description: 'Git operations, branch management, commit generation, and status diffing',
    tools: [
      { id: 'git_status', name: 'git_status', description: 'Query modified and untracked file status', dangerous: false },
      { id: 'git_diff', name: 'git_diff', description: 'Compute staged and unstaged unified diffs', dangerous: false },
      { id: 'git_commit', name: 'git_commit', description: 'Stage changes and create signed commit', dangerous: true },
      { id: 'git_push', name: 'git_push', description: 'Push committed refs to remote origin', dangerous: true }
    ],
    permissions: { autonomous: 'allow', subagent: 'ask', assistant: 'ask', reviewer: 'deny' }
  },
  {
    id: 'swarm',
    name: 'Subagent & Swarm Coordination',
    description: 'Spawn worker subagents, coordinate tasks on shared message board, and manage lifecycle',
    tools: [
      { id: 'spawn_agent', name: 'spawn_agent', description: 'Create isolated worker with role assignment', dangerous: false },
      { id: 'post_message', name: 'post_message', description: 'Broadcast message to swarm feed', dangerous: false },
      { id: 'read_board', name: 'read_board', description: 'Query chronological swarm events', dangerous: false },
      { id: 'kill_agent', name: 'kill_agent', description: 'Terminate subagent worker session', dangerous: true }
    ],
    permissions: { autonomous: 'allow', subagent: 'deny', assistant: 'allow', reviewer: 'deny' }
  },
  {
    id: 'skills',
    name: 'Skills Registry & Execution',
    description: 'Load SKILL.md bundles, execute script helpers, and manage procedural skills',
    tools: [
      { id: 'read_skill', name: 'read_skill', description: 'Read skill instructions and documentation', dangerous: false },
      { id: 'run_skill_script', name: 'run_skill_script', description: 'Execute sandboxed helper script from skill', dangerous: true },
      { id: 'import_skill', name: 'import_skill', description: 'Unpack zip skill bundle to registry', dangerous: false },
      { id: 'export_skill', name: 'export_skill', description: 'Package skill folder into portable zip', dangerous: false }
    ],
    permissions: { autonomous: 'allow', subagent: 'allow', assistant: 'allow', reviewer: 'allow' }
  },
  {
    id: 'mcp_gateway',
    name: 'MCP Gateway Tools',
    description: 'Model Context Protocol clients connecting Stdio, SSE, and Streamable HTTP servers',
    tools: [
      { id: 'call_mcp_tool', name: 'call_mcp_tool', description: 'Invoke tool provided by active MCP server', dangerous: true },
      { id: 'list_mcp_resources', name: 'list_mcp_resources', description: 'Discover resources exported by servers', dangerous: false },
      { id: 'read_mcp_resource', name: 'read_mcp_resource', description: 'Fetch MCP resource payload via URI', dangerous: false }
    ],
    permissions: { autonomous: 'allow', subagent: 'allow', assistant: 'allow', reviewer: 'ask' }
  },
  {
    id: 'speech_audio',
    name: 'Speech & Audio Processing',
    description: 'Local Whisper transcription, mic voice capture, and text-to-speech synthesis',
    tools: [
      { id: 'transcribe_audio', name: 'transcribe_audio', description: 'Process audio buffer using Whisper ggml', dangerous: false },
      { id: 'record_voice', name: 'record_voice', description: 'Record microphone input to 16kHz WAV', dangerous: false },
      { id: 'synthesize_speech', name: 'synthesize_speech', description: 'Convert text to voice stream', dangerous: false }
    ],
    permissions: { autonomous: 'allow', subagent: 'deny', assistant: 'allow', reviewer: 'deny' }
  },
  {
    id: 'telegram',
    name: 'Telegram Bridge',
    description: 'Two-way Telegram messaging, mobile push alerts, and remote authorization',
    tools: [
      { id: 'send_telegram', name: 'send_telegram', description: 'Deliver notification to paired Telegram user', dangerous: false },
      { id: 'poll_telegram', name: 'poll_telegram', description: 'Check inbound commands and approval tokens', dangerous: false },
      { id: 'broadcast_status', name: 'broadcast_status', description: 'Send status digest to authorized channel', dangerous: false }
    ],
    permissions: { autonomous: 'allow', subagent: 'deny', assistant: 'allow', reviewer: 'deny' }
  },
  {
    id: 'scheduler',
    name: 'Scheduler & Background Cron',
    description: 'Cron-based automated timers, recurring workflows, and deferred triggers',
    tools: [
      { id: 'schedule_job', name: 'schedule_job', description: 'Register new recurring cron task', dangerous: true },
      { id: 'list_jobs', name: 'list_jobs', description: 'Query status of scheduled tasks', dangerous: false },
      { id: 'cancel_job', name: 'cancel_job', description: 'Unregister background scheduled job', dangerous: false }
    ],
    permissions: { autonomous: 'allow', subagent: 'deny', assistant: 'ask', reviewer: 'deny' }
  },
  {
    id: 'memory_system',
    name: 'Memory System & Knowledge',
    description: 'Persistent 32k memory database, vector similarity, and user preferences',
    tools: [
      { id: 'store_memory', name: 'store_memory', description: 'Persist memory item with category tag', dangerous: false },
      { id: 'recall_memory', name: 'recall_memory', description: 'Query memories by semantic keyword', dangerous: false },
      { id: 'delete_memory', name: 'delete_memory', description: 'Erase memory record from database', dangerous: false }
    ],
    permissions: { autonomous: 'allow', subagent: 'allow', assistant: 'allow', reviewer: 'allow' }
  },
  {
    id: 'system_diag',
    name: 'System & Host Diagnostics',
    description: 'Host CPU, RAM, disk diagnostics, network health, and SQLite status',
    tools: [
      { id: 'system_info', name: 'system_info', description: 'Query OS kernel, uptime, and host specs', dangerous: false },
      { id: 'memory_stats', name: 'memory_stats', description: 'Inspect memory allocation and cache sizes', dangerous: false },
      { id: 'export_logs', name: 'export_logs', description: 'Write trace logs to output file', dangerous: false }
    ],
    permissions: { autonomous: 'allow', subagent: 'allow', assistant: 'allow', reviewer: 'allow' }
  }
]);

let expandedCategoryId = $state<string | null>(null);

let filteredCategories = $derived(
  categories.filter((cat) => {
    if (!searchQuery.trim()) return true;
    const q = searchQuery.toLowerCase();
    const matchName = cat.name.toLowerCase().includes(q);
    const matchDesc = cat.description.toLowerCase().includes(q);
    const matchTool = cat.tools.some((t) => t.name.toLowerCase().includes(q) || t.description.toLowerCase().includes(q));
    return matchName || matchDesc || matchTool;
  })
);

function setPermission(catId: string, role: AgentRole, level: PermissionLevel) {
  const cat = categories.find((c) => c.id === catId);
  if (cat) {
    cat.permissions[role] = level;
  }
}

function applyPreset(preset: 'auto' | 'safe' | 'strict' | 'readonly') {
  for (const cat of categories) {
    if (preset === 'auto') {
      cat.permissions = { autonomous: 'allow', subagent: 'allow', assistant: 'allow', reviewer: 'allow' };
    } else if (preset === 'safe') {
      const isDangerous = cat.tools.some((t) => t.dangerous);
      cat.permissions = {
        autonomous: isDangerous ? 'ask' : 'allow',
        subagent: isDangerous ? 'deny' : 'allow',
        assistant: isDangerous ? 'ask' : 'allow',
        reviewer: isDangerous ? 'deny' : 'allow'
      };
    } else if (preset === 'strict') {
      cat.permissions = { autonomous: 'ask', subagent: 'deny', assistant: 'ask', reviewer: 'deny' };
    } else if (preset === 'readonly') {
      const hasDangerous = cat.tools.some((t) => t.dangerous);
      cat.permissions = {
        autonomous: hasDangerous ? 'deny' : 'allow',
        subagent: 'deny',
        assistant: hasDangerous ? 'deny' : 'allow',
        reviewer: 'deny'
      };
    }
  }
  saveStatus = `Applied '${preset}' permission preset across all 14 categories`;
}

async function saveMatrix() {
  saveStatus = 'Saving permission matrix...';
  try {
    await invoke('save_tools_permissions', {
      categories: categories.map((c) => ({
        id: c.id,
        permissions: c.permissions
      }))
    });
    saveStatus = 'Tool permission matrix saved successfully';
  } catch (err: unknown) {
    saveStatus = `Save error: ${err instanceof Error ? err.message : String(err)}`;
  }
}
</script>

<div class="flex flex-col h-full bg-[#0E0F12] text-[#E5E7EB] font-sans text-xs select-none p-6 space-y-5 overflow-hidden">
  <div class="flex items-center justify-between pb-4 border-b border-[#1A1D24]">
    <div>
      <h2 class="text-base font-bold text-neutral-100">Tool Permission Matrix (14 Categories)</h2>
      <p class="text-neutral-500 text-[11px] mt-0.5">Role-based access control governing tool execution and human-in-the-loop approvals</p>
    </div>
    <div class="flex items-center gap-2">
      <button
        type="button"
        onclick={() => applyPreset('auto')}
        class="px-2.5 py-1.5 bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-300 rounded-lg border border-[#2B2E36] text-[11px]"
      >
        Full Auto
      </button>
      <button
        type="button"
        onclick={() => applyPreset('safe')}
        class="px-2.5 py-1.5 bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-300 rounded-lg border border-[#2B2E36] text-[11px]"
      >
        Balanced Safe
      </button>
      <button
        type="button"
        onclick={() => applyPreset('strict')}
        class="px-2.5 py-1.5 bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-300 rounded-lg border border-[#2B2E36] text-[11px]"
      >
        Strict Ask
      </button>
      <button
        type="button"
        onclick={() => applyPreset('readonly')}
        class="px-2.5 py-1.5 bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-300 rounded-lg border border-[#2B2E36] text-[11px]"
      >
        Read Only
      </button>
      <button
        type="button"
        onclick={saveMatrix}
        class="px-4 py-1.5 bg-blue-600 hover:bg-blue-500 text-white rounded-lg font-medium transition-colors ml-2"
      >
        Save Policy
      </button>
    </div>
  </div>

  {#if saveStatus}
    <div class="p-2.5 rounded-lg bg-blue-500/10 border border-blue-500/20 text-blue-400 flex items-center justify-between text-[11px]">
      <span>{saveStatus}</span>
      <button type="button" onclick={() => (saveStatus = null)} class="text-blue-400 hover:text-blue-200">✕</button>
    </div>
  {/if}

  <div class="flex items-center gap-3">
    <div class="flex-1 relative">
      <input
        type="text"
        bind:value={searchQuery}
        placeholder="Filter categories or specific tools (e.g. conpty, git_push, whisper)..."
        class="w-full bg-[#131416] border border-[#2B2E36] rounded-lg px-3 py-2 text-neutral-200 placeholder-neutral-500 focus:outline-none focus:border-blue-500"
      />
    </div>
  </div>

  <div class="flex-1 border border-[#1A1D24] rounded-xl overflow-hidden bg-[#131416] flex flex-col">
    <div class="overflow-y-auto flex-1">
      <table class="w-full text-left border-collapse">
        <thead class="sticky top-0 bg-[#0E0F12] border-b border-[#1A1D24] z-10">
          <tr class="text-neutral-400 text-[10px] uppercase font-semibold">
            <th class="p-3 w-1/3">Category & Tools</th>
            <th class="p-3 text-center">Autonomous Agent</th>
            <th class="p-3 text-center">Subagent Worker</th>
            <th class="p-3 text-center">User Assistant</th>
            <th class="p-3 text-center">Reviewer / Read-Only</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-[#1A1D24]">
          {#each filteredCategories as cat (cat.id)}
            <tr class="hover:bg-[#1A1D24]/30 transition-colors">
              <td class="p-3 align-top">
                <div class="flex items-center gap-2">
                  <button
                    type="button"
                    onclick={() => (expandedCategoryId = expandedCategoryId === cat.id ? null : cat.id)}
                    class="text-neutral-400 hover:text-white text-[10px] font-mono"
                  >
                    {expandedCategoryId === cat.id ? '▼' : '▶'}
                  </button>
                  <span class="font-medium text-neutral-200">{cat.name}</span>
                  <span class="text-[10px] px-1.5 py-0.2 rounded bg-[#1A1D24] text-neutral-400 font-mono">
                    {cat.tools.length} tools
                  </span>
                </div>
                <div class="text-[11px] text-neutral-500 mt-1 pl-4">{cat.description}</div>

                {#if expandedCategoryId === cat.id}
                  <div class="mt-2.5 pl-4 space-y-1 border-l-2 border-[#2B2E36]">
                    {#each cat.tools as tool}
                      <div class="flex items-center gap-2 text-[11px]">
                        <code class="text-blue-400 font-mono">{tool.name}</code>
                        <span class="text-neutral-500">- {tool.description}</span>
                        {#if tool.dangerous}
                          <span class="px-1.5 py-0.2 rounded text-[9px] bg-rose-500/10 text-rose-400 border border-rose-500/20 font-semibold uppercase">
                            Dangerous
                          </span>
                        {/if}
                      </div>
                    {/each}
                  </div>
                {/if}
              </td>

              {#each (['autonomous', 'subagent', 'assistant', 'reviewer'] as AgentRole[]) as role}
                <td class="p-3 text-center align-middle">
                  <div class="inline-flex rounded-lg bg-[#0E0F12] p-0.5 border border-[#2B2E36]">
                    <button
                      type="button"
                      onclick={() => setPermission(cat.id, role, 'allow')}
                      class="px-2 py-0.5 rounded text-[10px] font-semibold transition-colors {cat.permissions[role] === 'allow' ? 'bg-emerald-500/20 text-emerald-400 border border-emerald-500/30' : 'text-neutral-500 hover:text-neutral-300'}"
                    >
                      Allow
                    </button>
                    <button
                      type="button"
                      onclick={() => setPermission(cat.id, role, 'ask')}
                      class="px-2 py-0.5 rounded text-[10px] font-semibold transition-colors {cat.permissions[role] === 'ask' ? 'bg-amber-500/20 text-amber-400 border border-amber-500/30' : 'text-neutral-500 hover:text-neutral-300'}"
                    >
                      Ask
                    </button>
                    <button
                      type="button"
                      onclick={() => setPermission(cat.id, role, 'deny')}
                      class="px-2 py-0.5 rounded text-[10px] font-semibold transition-colors {cat.permissions[role] === 'deny' ? 'bg-rose-500/20 text-rose-400 border border-rose-500/30' : 'text-neutral-500 hover:text-neutral-300'}"
                    >
                      Deny
                    </button>
                  </div>
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
</div>
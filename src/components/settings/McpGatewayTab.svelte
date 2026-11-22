<script lang="ts">
import { onMount } from 'svelte';
import { slide } from 'svelte/transition';
import { invoke } from '@tauri-apps/api/core';
import Icon from '../Icon.svelte';

type McpTransportType = 'stdio' | 'sse' | 'streamable_http';
type ConnectionState = 'connected' | 'connecting' | 'disconnected' | 'error';

interface McpToolSchema {
  name: string;
  description?: string;
}

interface DbMcpServer {
  id: string;
  name: string;
  transportType: string;
  command?: string | null;
  args?: string | null;
  env?: string | null;
  url?: string | null;
  isEnabled: boolean;
  createdAt: string;
  updatedAt: string;
}

interface McpServerUi {
  id: string;
  name: string;
  transport: McpTransportType;
  command: string;
  args: string[];
  env: Record<string, string>;
  url: string;
  isEnabled: boolean;
  status: ConnectionState;
  latencyMs?: number;
  errorMessage?: string;
  tools: McpToolSchema[];
  createdAt: string;
}

let servers = $state<McpServerUi[]>([]);
let selectedServerId = $state<string>('');
let activeServer = $derived(servers.find((s) => s.id === selectedServerId) ?? servers[0] ?? null);
let visibleToolsLimit = $state<number>(60);

let showForm = $state<boolean>(false);
let editingId = $state<string | null>(null);
let showImport = $state<boolean>(false);
let importJsonText = $state<string>('');
let feedback = $state<{ type: 'ok' | 'err'; text: string } | null>(null);

let formName = $state<string>('');
let formTransport = $state<McpTransportType>('stdio');
let formCommand = $state<string>('');
let formArgsText = $state<string>('');
let formEnvText = $state<string>('{}');
let formUrl = $state<string>('');
let formEnabled = $state<boolean>(true);

$effect(() => {
  const _id = activeServer?.id;
  visibleToolsLimit = 60;
});

function parseArgsString(raw?: string | null): string[] {
  if (!raw || !raw.trim()) return [];
  try {
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) return parsed.map((x) => String(x));
  } catch {}
  return raw.split(/\s+/).filter(Boolean);
}

function parseEnvString(raw?: string | null): Record<string, string> {
  if (!raw || !raw.trim()) return {};
  try {
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === 'object' && !Array.isArray(parsed)) {
      const out: Record<string, string> = {};
      for (const [k, v] of Object.entries(parsed)) {
        out[k] = String(v);
      }
      return out;
    }
  } catch {}
  return {};
}

async function loadServers() {
  try {
    const [dbList, liveTools] = await Promise.all([
      invoke<DbMcpServer[]>('list_mcp_servers'),
      invoke<Array<{ serverName: string; tool: McpToolSchema }>>('list_mcp_tools').catch(() => [])
    ]);
    const toolsByServer: Record<string, McpToolSchema[]> = {};
    for (const item of liveTools) {
      if (!toolsByServer[item.serverName]) toolsByServer[item.serverName] = [];
      toolsByServer[item.serverName].push(item.tool);
    }

    const prevMap = new Map(servers.map((s) => [s.id, s]));
    servers = dbList.map((d) => {
      const prev = prevMap.get(d.id);
      const live = toolsByServer[d.name] || [];
      const t = (d.transportType || 'stdio').toLowerCase();
      const transport: McpTransportType =
        t === 'sse' ? 'sse' : t === 'streamable_http' || t === 'http' ? 'streamable_http' : 'stdio';
      return {
        id: d.id,
        name: d.name,
        transport,
        command: d.command || '',
        args: parseArgsString(d.args),
        env: parseEnvString(d.env),
        url: d.url || '',
        isEnabled: Boolean(d.isEnabled),
        status: live.length > 0 ? 'connected' : prev?.status || 'disconnected',
        latencyMs: prev?.latencyMs,
        errorMessage: prev?.errorMessage,
        tools: live.length > 0 ? live : prev?.tools || [],
        createdAt: d.createdAt
      };
    });
    if (!selectedServerId && servers.length > 0) {
      selectedServerId = servers[0].id;
    }
  } catch (err: unknown) {
    feedback = { type: 'err', text: err instanceof Error ? err.message : String(err) };
  }
}

onMount(() => {
  loadServers();
});

function openCreateForm() {
  editingId = null;
  formName = '';
  formTransport = 'stdio';
  formCommand = '';
  formArgsText = '';
  formEnvText = '{}';
  formUrl = '';
  formEnabled = true;
  showImport = false;
  showForm = true;
  feedback = null;
}

function openEditForm(srv: McpServerUi) {
  editingId = srv.id;
  formName = srv.name;
  formTransport = srv.transport;
  formCommand = srv.command;
  formArgsText = srv.args.join(' ');
  formEnvText = JSON.stringify(srv.env, null, 2);
  formUrl = srv.url;
  formEnabled = srv.isEnabled;
  showImport = false;
  showForm = true;
  feedback = null;
}

function closeForm() {
  showForm = false;
  editingId = null;
}

async function handleSaveServer(e: Event) {
  e.preventDefault();
  const cleanName = formName.trim();
  if (!cleanName) return;

  let parsedEnv: Record<string, string> = {};
  if (formEnvText.trim()) {
    try {
      const obj = JSON.parse(formEnvText);
      if (obj && typeof obj === 'object' && !Array.isArray(obj)) {
        for (const [k, v] of Object.entries(obj)) parsedEnv[k] = String(v);
      }
    } catch {
      feedback = { type: 'err', text: 'Env must be valid JSON object.' };
      return;
    }
  }

  const argsArr = formArgsText.trim() ? formArgsText.trim().split(/\s+/) : [];
  const now = new Date().toISOString();
  const existing = editingId ? servers.find((s) => s.id === editingId) : null;

  const payload: DbMcpServer = {
    id: editingId || `mcp-${Date.now().toString(36)}`,
    name: cleanName,
    transportType: formTransport,
    command: formTransport === 'stdio' ? formCommand.trim() || null : null,
    args: formTransport === 'stdio' ? JSON.stringify(argsArr) : null,
    env: JSON.stringify(parsedEnv),
    url: formTransport !== 'stdio' ? formUrl.trim() || null : null,
    isEnabled: formEnabled,
    createdAt: existing?.createdAt || now,
    updatedAt: now
  };

  try {
    if (editingId) {
      await invoke('update_mcp_server', { server: payload });
      feedback = { type: 'ok', text: `Updated "${cleanName}".` };
    } else {
      await invoke('create_mcp_server', { server: payload });
      selectedServerId = payload.id;
      feedback = { type: 'ok', text: `Added "${cleanName}".` };
    }
    closeForm();
    await loadServers();
  } catch (err: unknown) {
    feedback = { type: 'err', text: err instanceof Error ? err.message : String(err) };
  }
}

async function handleDeleteServer(srv: McpServerUi) {
  try {
    if (srv.status === 'connected') {
      await invoke('disconnect_mcp_server', { name: srv.name }).catch(() => {});
    }
    await invoke('delete_mcp_server', { id: srv.id });
    if (selectedServerId === srv.id) selectedServerId = '';
    feedback = { type: 'ok', text: `Removed "${srv.name}".` };
    await loadServers();
  } catch (err: unknown) {
    feedback = { type: 'err', text: err instanceof Error ? err.message : String(err) };
  }
}

async function handleImportJson() {
  const raw = importJsonText.trim();
  if (!raw) return;
  try {
    const parsed = JSON.parse(raw);
    const mapObj = parsed.mcpServers || parsed;
    if (!mapObj || typeof mapObj !== 'object') {
      feedback = { type: 'err', text: 'Invalid MCP JSON format.' };
      return;
    }
    const now = new Date().toISOString();
    let count = 0;
    for (const [name, cfg] of Object.entries<any>(mapObj)) {
      if (!cfg || typeof cfg !== 'object') continue;
      const transport = cfg.transport || (cfg.command ? 'stdio' : 'sse');
      const id = `mcp-${name.toLowerCase().replace(/[^a-z0-9]+/g, '-')}-${Date.now().toString(36)}`;
      await invoke('create_mcp_server', {
        server: {
          id,
          name,
          transportType: transport,
          command: cfg.command || null,
          args: Array.isArray(cfg.args) ? JSON.stringify(cfg.args) : null,
          env: cfg.env ? JSON.stringify(cfg.env) : '{}',
          url: cfg.url || null,
          isEnabled: cfg.enabled !== false,
          createdAt: now,
          updatedAt: now
        }
      });
      count++;
    }
    importJsonText = '';
    showImport = false;
    feedback = { type: 'ok', text: `Imported ${count} MCP server(s).` };
    await loadServers();
  } catch (err: unknown) {
    feedback = { type: 'err', text: err instanceof Error ? err.message : String(err) };
  }
}

function formatStatusLabel(srv: McpServerUi): string {
  if (srv.status === 'connected') {
    return srv.latencyMs !== undefined ? `online · ${srv.latencyMs}ms` : 'online';
  }
  if (srv.status === 'connecting') return 'connecting...';
  if (srv.status === 'error') return srv.errorMessage ? `error: ${srv.errorMessage}` : 'error';
  return 'stopped';
}

async function toggleConnection(server: McpServerUi) {
  if (server.status === 'connected') {
    try {
      await invoke('disconnect_mcp_server', { name: server.name });
    } catch {}
    server.status = 'disconnected';
    server.latencyMs = undefined;
    server.tools = [];
    return;
  }

  server.status = 'connecting';
  server.errorMessage = undefined;
  const start = performance.now();
  try {
    await invoke('connect_mcp_server', {
      server: {
        name: server.name,
        transport: server.transport,
        command: server.transport === 'stdio' ? server.command || null : null,
        args: server.args,
        env: server.env,
        url: server.transport !== 'stdio' ? server.url || null : null,
        enabled: server.isEnabled
      }
    });
    const allTools = await invoke<Array<{ serverName: string; tool: McpToolSchema }>>('list_mcp_tools').catch(() => []);
    server.tools = allTools.filter((t) => t.serverName === server.name).map((t) => t.tool);
    server.status = 'connected';
    server.latencyMs = Math.round(performance.now() - start);
  } catch (err: unknown) {
    server.status = 'error';
    server.errorMessage = err instanceof Error ? err.message : String(err);
  }
}
</script>

<div class="space-y-4 text-[13px] text-[#E5E5E8]">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h2 class="text-sm font-medium text-[#EDEDEF]">MCP Servers</h2>
      <p class="text-xs text-[#73737A] mt-0.5">Add, edit, connect and inspect Model Context Protocol bridges.</p>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#141416] hover:bg-[#1E1E22] border border-white/[0.08] text-xs text-[#A3A3AA] hover:text-[#EDEDEF] active:scale-[0.97] transition-all whitespace-nowrap"
        onclick={() => { showImport = !showImport; showForm = false; }}
      >
        Import JSON
      </button>
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium flex items-center gap-1.5 active:scale-[0.97] transition-all whitespace-nowrap"
        onclick={() => (showForm && !editingId ? closeForm() : openCreateForm())}
      >
        <Icon name={showForm && !editingId ? 'close' : 'plus'} class="w-3.5 h-3.5" strokeWidth={1.8} />
        <span>{showForm && !editingId ? 'Cancel' : 'Add Server'}</span>
      </button>
    </div>
  </div>

  {#if feedback}
    <div class="px-3.5 py-2 rounded-[6px] bg-[#1C1C1F] border border-white/[0.07] text-xs {feedback.type === 'err' ? 'text-[#F87171]' : 'text-[#A3A3AA]'}">
      {feedback.text}
    </div>
  {/if}

  {#if showImport}
    <div transition:slide={{ duration: 180 }} class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.1] space-y-3">
      <div class="text-xs font-medium text-[#EDEDEF]">Paste MCP JSON Config</div>
      <textarea
        bind:value={importJsonText}
        rows="5"
        placeholder={'{"mcpServers":{"filesystem":{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","C:/path"]}}}'}
        class="w-full px-3 py-2 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-[11.5px] text-[#E5E5E8] placeholder-[#55555C] outline-none"
      ></textarea>
      <div class="flex justify-end gap-2">
        <button
          type="button"
          class="h-7 px-3 rounded-[6px] bg-[#121214] hover:bg-[#242428] border border-white/[0.08] text-xs text-[#A3A3AA]"
          onclick={() => (showImport = false)}
        >
          Cancel
        </button>
        <button
          type="button"
          class="h-7 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium"
          onclick={handleImportJson}
        >
          Import
        </button>
      </div>
    </div>
  {/if}

  {#if showForm}
    <form
      onsubmit={handleSaveServer}
      transition:slide={{ duration: 180 }}
      class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.1] space-y-3"
    >
      <div class="text-xs font-medium text-[#EDEDEF]">{editingId ? 'Edit MCP Server' : 'New MCP Server'}</div>
      <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
        <label class="block space-y-1">
          <span class="text-[11px] text-[#A3A3AA]">Server Name</span>
          <input
            bind:value={formName}
            type="text"
            required
            placeholder="filesystem"
            class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none focus:border-white/[0.2]"
          />
        </label>
        <label class="block space-y-1">
          <span class="text-[11px] text-[#A3A3AA]">Transport</span>
          <select
            bind:value={formTransport}
            class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] text-xs text-[#E5E5E8] outline-none"
          >
            <option value="stdio">stdio</option>
            <option value="sse">sse</option>
            <option value="streamable_http">streamable_http</option>
          </select>
        </label>
      </div>

      {#if formTransport === 'stdio'}
        <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
          <label class="block space-y-1">
            <span class="text-[11px] text-[#A3A3AA]">Command</span>
            <input
              bind:value={formCommand}
              type="text"
              required
              placeholder="npx"
              class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] outline-none"
            />
          </label>
          <label class="block space-y-1">
            <span class="text-[11px] text-[#A3A3AA]">Arguments (space-separated)</span>
            <input
              bind:value={formArgsText}
              type="text"
              placeholder="-y @modelcontextprotocol/server-filesystem D:/Project"
              class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] outline-none"
            />
          </label>
        </div>
      {:else}
        <label class="block space-y-1">
          <span class="text-[11px] text-[#A3A3AA]">Server URL</span>
          <input
            bind:value={formUrl}
            type="url"
            required
            placeholder="http://localhost:3000/mcp"
            class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] outline-none"
          />
        </label>
      {/if}

      <label class="block space-y-1">
        <span class="text-[11px] text-[#A3A3AA]">Environment Variables (JSON)</span>
        <textarea
          bind:value={formEnvText}
          rows="2"
          class="w-full px-3 py-1.5 rounded-[6px] bg-[#121214] border border-white/[0.08] font-mono text-[11px] text-[#E5E5E8] outline-none"
        ></textarea>
      </label>

      <div class="flex items-center justify-between pt-1">
        <label class="flex items-center gap-2 text-xs text-[#A3A3AA] cursor-pointer">
          <input type="checkbox" bind:checked={formEnabled} class="accent-[#EDEDEF]" />
          <span>Enabled</span>
        </label>
        <div class="flex items-center gap-2">
          <button
            type="button"
            onclick={closeForm}
            class="h-8 px-3 rounded-[6px] bg-[#121214] hover:bg-[#242428] border border-white/[0.08] text-xs text-[#A3A3AA]"
          >
            Cancel
          </button>
          <button
            type="submit"
            class="h-8 px-4 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium"
          >
            {editingId ? 'Save Changes' : 'Create Server'}
          </button>
        </div>
      </div>
    </form>
  {/if}

  {#if servers.length === 0 && !showForm && !showImport}
    <div class="p-8 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] text-center space-y-2">
      <div class="text-sm text-[#E5E5E8] font-medium">No MCP servers configured</div>
      <p class="text-xs text-[#73737A]">Add a local stdio server or remote SSE/HTTP MCP endpoint.</p>
    </div>
  {:else if servers.length > 0}
    <div class="grid grid-cols-1 md:grid-cols-[220px_minmax(0,1fr)] gap-4">
      <div class="space-y-1.5">
        {#each servers as srv (srv.id)}
          <button
            type="button"
            class="w-full p-2.5 rounded-[6px] text-left border active:scale-[0.99] transition-all {activeServer?.id === srv.id ? 'bg-[#1C1C1F] border-white/[0.16]' : 'bg-[#141416] border-white/[0.06] hover:bg-[#19191C]'}"
            onclick={() => (selectedServerId = srv.id)}
          >
            <div class="flex items-center justify-between gap-2">
              <span class="text-xs font-medium text-[#EDEDEF] truncate">{srv.name}</span>
              <span class="font-mono text-[10px] shrink-0 {srv.status === 'connected' ? 'text-[#4ADE80]' : srv.status === 'error' ? 'text-[#F87171]' : 'text-[#66666D]'}">
                {srv.status === 'connected' ? (srv.latencyMs !== undefined ? `${srv.latencyMs}ms` : 'online') : srv.status === 'error' ? 'error' : 'stopped'}
              </span>
            </div>
            <div class="font-mono text-[10.5px] text-[#66666D] mt-1">{srv.transport} · {srv.tools.length} tools</div>
          </button>
        {/each}
      </div>

      {#if activeServer}
        <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] space-y-4 min-w-0">
          <div class="space-y-2">
            <div class="flex items-center justify-between gap-2">
              <h3 class="text-sm font-medium text-[#EDEDEF] truncate">{activeServer.name}</h3>
              <div class="flex items-center gap-1.5 shrink-0">
                <button
                  type="button"
                  class="p-1.5 rounded-[6px] text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
                  onclick={() => openEditForm(activeServer)}
                  title="Edit server"
                >
                  <Icon name="edit" class="w-3.5 h-3.5" strokeWidth={1.6} />
                </button>
                <button
                  type="button"
                  class="p-1.5 rounded-[6px] text-[#73737A] hover:text-[#F87171] hover:bg-white/[0.06] transition-colors"
                  onclick={() => handleDeleteServer(activeServer)}
                  title="Delete server"
                >
                  <Icon name="trash" class="w-3.5 h-3.5" strokeWidth={1.6} />
                </button>
                <button
                  type="button"
                  class="h-7 px-3 rounded-[6px] text-xs whitespace-nowrap active:scale-[0.97] transition-all {activeServer.status === 'connected' ? 'bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[#D4D4D8]' : 'bg-[#EDEDEF] hover:bg-white text-[#111113] font-medium'}"
                  onclick={() => toggleConnection(activeServer)}
                >
                  {activeServer.status === 'connected' ? 'Disconnect' : activeServer.status === 'connecting' ? 'Connecting...' : 'Connect'}
                </button>
              </div>
            </div>

            <div class="font-mono text-[11px] {activeServer.status === 'error' ? 'text-[#F87171]' : 'text-[#73737A]'}">
              {formatStatusLabel(activeServer)}
            </div>
          </div>

          {#if activeServer.command || activeServer.url}
            <div class="p-2.5 rounded-[6px] bg-[#121214] border border-white/[0.06] font-mono text-[11px] text-[#A3A3AA] flex flex-wrap items-center gap-1.5">
              {#if activeServer.command}
                <span class="px-1.5 py-0.5 rounded-[4px] bg-[#1C1C1F] text-[#EDEDEF]">{activeServer.command}</span>
                {#each activeServer.args as arg}
                  <span class="px-1.5 py-0.5 rounded-[4px] bg-[#18181B] text-[#A3A3AA] break-all">{arg}</span>
                {/each}
              {:else if activeServer.url}
                <span class="break-all">{activeServer.url}</span>
              {/if}
            </div>
          {/if}

          <div class="space-y-2">
            <div class="font-mono text-[10.5px] text-[#73737A] uppercase tracking-wider">
              Discovered Tools ({activeServer.tools.length})
            </div>

            <div class="space-y-1.5">
              {#each activeServer.tools.slice(0, visibleToolsLimit) as tool, idx (`${idx}-${tool.name}`)}
                <div class="p-2.5 rounded-[6px] bg-[#121214] border border-white/[0.06]">
                  <div class="font-mono text-xs text-[#EDEDEF]">{tool.name}</div>
                  {#if tool.description}
                    <div class="text-[11px] text-[#73737A] mt-0.5">{tool.description}</div>
                  {/if}
                </div>
              {/each}
              {#if activeServer.tools.length > visibleToolsLimit}
                <button
                  type="button"
                  onclick={() => (visibleToolsLimit += 80)}
                  class="w-full py-2 rounded-[6px] bg-[#141416] hover:bg-[#1C1C1F] border border-white/[0.08] text-xs text-[#A3A3AA] hover:text-[#EDEDEF] transition-colors"
                >
                  Show more ({activeServer.tools.length - visibleToolsLimit} remaining)
                </button>
              {/if}
            </div>
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>
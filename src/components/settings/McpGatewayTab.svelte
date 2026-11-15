<script lang="ts">
import { fly } from 'svelte/transition';
import { invoke } from '@tauri-apps/api/core';

type McpTransportType = 'stdio' | 'sse' | 'streamable_http';
type ConnectionState = 'connected' | 'connecting' | 'disconnected' | 'error';

interface McpToolSchema {
  name: string;
  description: string;
}

interface McpServerConfig {
  id: string;
  name: string;
  transport: McpTransportType;
  command?: string;
  args?: string[];
  url?: string;
  status: ConnectionState;
  latencyMs?: number;
  errorMessage?: string;
  tools: McpToolSchema[];
}

let servers = $state<McpServerConfig[]>([]);

let selectedServerId = $state<string>('');
let activeServer = $derived(servers.find((s) => s.id === selectedServerId) ?? servers[0]);

function formatStatusLabel(srv: McpServerConfig): string {
  if (srv.status === 'connected') {
    return srv.latencyMs !== undefined ? `online · ${srv.latencyMs}ms` : 'online';
  }
  if (srv.status === 'connecting') return 'connecting';
  if (srv.status === 'error') return 'error';
  return 'stopped';
}

async function toggleConnection(server: McpServerConfig) {
  if (server.status === 'connected') {
    try {
      await invoke('disconnect_mcp_server', { serverId: server.id });
    } catch {}
    server.status = 'disconnected';
    server.latencyMs = undefined;
  } else {
    server.status = 'connecting';
    const start = performance.now();
    try {
      const res = await invoke<{ latencyMs?: number; tools?: McpToolSchema[] }>('connect_mcp_server', {
        serverId: server.id,
        transport: server.transport,
        command: server.command,
        args: server.args,
        url: server.url
      });
      server.status = 'connected';
      server.latencyMs = res?.latencyMs ?? Math.round(performance.now() - start);
      if (res?.tools) server.tools = res.tools;
    } catch (err: unknown) {
      server.status = 'error';
      server.errorMessage = err instanceof Error ? err.message : String(err);
    }
  }
}
</script>

<div class="space-y-4 text-[13px] text-[#E5E5E8]">
  <div>
    <h2 class="text-sm font-medium text-[#EDEDEF]">MCP Servers</h2>
    <p class="text-xs text-[#73737A] mt-0.5">Manage Model Context Protocol bridges and inspect exposed tools.</p>
  </div>

  <div class="grid grid-cols-1 md:grid-cols-[220px_minmax(0,1fr)] gap-4">
    <div class="space-y-1.5">
      {#each servers as srv (srv.id)}
        <button
          type="button"
          class="w-full p-2.5 rounded-[6px] text-left border active:scale-[0.99] transition-all {selectedServerId === srv.id ? 'bg-[#1C1C1F] border-white/[0.16]' : 'bg-[#141416] border-white/[0.06] hover:bg-[#19191C]'}"
          onclick={() => (selectedServerId = srv.id)}
        >
          <div class="flex items-center justify-between gap-2">
            <span class="text-xs font-medium text-[#EDEDEF] truncate">{srv.name}</span>
            <span class="font-mono text-[10px] shrink-0 {srv.status === 'connected' ? 'text-[#A3A3AA]' : srv.status === 'error' ? 'text-[#A16262]' : 'text-[#66666D]'}">
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
          <div class="flex items-center justify-between gap-3">
            <h3 class="text-sm font-medium text-[#EDEDEF] truncate">{activeServer.name}</h3>
            <button
              type="button"
              class="h-7 px-3 rounded-[6px] text-xs whitespace-nowrap active:scale-[0.97] transition-all {activeServer.status === 'connected' ? 'bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[#D4D4D8]' : 'bg-[#EDEDEF] hover:bg-white text-[#111113] font-medium'}"
              onclick={() => toggleConnection(activeServer)}
            >
              {activeServer.status === 'connected' ? 'Disconnect' : activeServer.status === 'connecting' ? 'Connecting...' : 'Connect'}
            </button>
          </div>

          <div class="font-mono text-[11px] text-[#73737A]">
            {formatStatusLabel(activeServer)}
          </div>
        </div>

        {#if activeServer.command || activeServer.url}
          <div class="p-2.5 rounded-[6px] bg-[#121214] border border-white/[0.06] font-mono text-[11px] text-[#A3A3AA] flex flex-wrap items-center gap-1.5">
            {#if activeServer.command}
              <span class="px-1.5 py-0.5 rounded-[4px] bg-[#1C1C1F] text-[#EDEDEF]">{activeServer.command}</span>
              {#each activeServer.args || [] as arg}
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

          {#key activeServer.id}
            <div class="space-y-1.5">
              {#each activeServer.tools as tool, idx (tool.name)}
                <div
                  in:fly={{ y: 6, duration: 150, delay: idx * 25 }}
                  class="p-2.5 rounded-[6px] bg-[#121214] border border-white/[0.06]"
                >
                  <div class="font-mono text-xs text-[#EDEDEF]">{tool.name}</div>
                  <div class="text-[11px] text-[#73737A] mt-0.5">{tool.description}</div>
                </div>
              {/each}
            </div>
          {/key}
        </div>
      </div>
    {/if}
  </div>
</div>
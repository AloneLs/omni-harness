<script lang="ts">
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/core';

interface ComputerUsePolicy {
  stealthMode: boolean;
  autoStageOffscreen: boolean;
  drawVirtualCursor: boolean;
  defaultWidth: number;
  defaultHeight: number;
}

interface ManagedWindowItem {
  hwnd: number;
  pid: number;
  processName: string;
  title: string;
  className: string;
  isMinimized?: boolean;
  isOffscreen?: boolean;
  isBound?: boolean;
  isStaged?: boolean;
}

let policy = $state<ComputerUsePolicy>({
  stealthMode: true,
  autoStageOffscreen: true,
  drawVirtualCursor: true,
  defaultWidth: 1280,
  defaultHeight: 820
});

let windows = $state<ManagedWindowItem[]>([]);
let stagedCount = $state<number>(0);
let busyHwnd = $state<number | null>(null);
let previewData = $state<{ hwnd: number; title: string; dataUrl: string; method: string } | null>(null);
let previewLoading = $state<boolean>(false);
let statusError = $state<string | null>(null);

async function refreshStatus() {
  try {
    const res = await invoke<{
      policy: ComputerUsePolicy;
      stagedWindows: any[];
      windows: ManagedWindowItem[];
    }>('cu_get_status');
    if (res?.policy) {
      policy = res.policy;
    }
    stagedCount = Array.isArray(res?.stagedWindows) ? res.stagedWindows.length : 0;
    const rawWins = Array.isArray(res?.windows) ? res.windows : [];
    rawWins.sort((a, b) => {
      const sa = (a.isStaged ? 4 : 0) + (a.isBound ? 2 : 0) + (a.isOffscreen ? 1 : 0);
      const sb = (b.isStaged ? 4 : 0) + (b.isBound ? 2 : 0) + (b.isOffscreen ? 1 : 0);
      return sb - sa;
    });
    windows = rawWins;
    statusError = null;
  } catch (err: unknown) {
    statusError = err instanceof Error ? err.message : String(err);
  }
}

async function updatePolicy(patch: Partial<ComputerUsePolicy>) {
  const next = { ...policy, ...patch };
  policy = next;
  try {
    const saved = await invoke<ComputerUsePolicy>('cu_set_policy', { policy: next });
    policy = saved;
  } catch (err: unknown) {
    statusError = err instanceof Error ? err.message : String(err);
  }
}

async function toggleStageWindow(win: ManagedWindowItem) {
  busyHwnd = win.hwnd;
  try {
    const targetOffscreen = !(win.isOffscreen || win.isStaged);
    await invoke('cu_stage_window', { hwnd: win.hwnd, offscreen: targetOffscreen });
    await refreshStatus();
    if (previewData && previewData.hwnd === win.hwnd) {
      await loadPreview(win);
    }
  } catch (err: unknown) {
    statusError = err instanceof Error ? err.message : String(err);
  } finally {
    busyHwnd = null;
  }
}

async function restoreAll() {
  try {
    await invoke('cu_restore_all_windows');
    await refreshStatus();
  } catch (err: unknown) {
    statusError = err instanceof Error ? err.message : String(err);
  }
}

async function loadPreview(win: ManagedWindowItem) {
  previewLoading = true;
  try {
    const shot = await invoke<{ dataUrl?: string; captureMethod?: string; title?: string }>(
      'cu_preview_window',
      { hwnd: win.hwnd }
    );
    if (shot?.dataUrl) {
      previewData = {
        hwnd: win.hwnd,
        title: shot.title || win.title || win.processName,
        dataUrl: shot.dataUrl,
        method: shot.captureMethod || 'PrintWindow'
      };
    }
  } catch (err: unknown) {
    statusError = err instanceof Error ? err.message : String(err);
  } finally {
    previewLoading = false;
  }
}

async function closeBgWindow(win: ManagedWindowItem) {
  busyHwnd = win.hwnd;
  try {
    await invoke('cu_close_window', { hwnd: win.hwnd });
    if (previewData?.hwnd === win.hwnd) {
      previewData = null;
    }
    setTimeout(refreshStatus, 180);
  } catch (err: unknown) {
    statusError = err instanceof Error ? err.message : String(err);
  } finally {
    busyHwnd = null;
  }
}

onMount(() => {
  refreshStatus();
  const iv = setInterval(refreshStatus, 4000);
  return () => clearInterval(iv);
});
</script>

<div class="space-y-5 text-[13px] text-[#E5E5E8]">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h2 class="text-sm font-medium text-[#EDEDEF]">Computer Use (Phantom Mode)</h2>
      <p class="text-xs text-[#73737A] mt-0.5">
        Background UIAutomation &amp; window-message control without cursor or focus theft.
      </p>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      {#if stagedCount > 0}
        <button
          type="button"
          onclick={restoreAll}
          class="h-8 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium transition-colors"
        >
          Restore All ({stagedCount})
        </button>
      {/if}
      <button
        type="button"
        onclick={refreshStatus}
        class="h-8 px-3 rounded-[6px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#D4D4D8] transition-colors"
      >
        Refresh
      </button>
    </div>
  </div>

  {#if statusError}
    <div class="px-3.5 py-2 rounded-[6px] bg-[#18181B] border border-[#F87171]/30 text-xs text-[#F87171]">
      {statusError}
    </div>
  {/if}

  <div class="rounded-[6px] bg-[#18181B]/70 border border-white/[0.07] divide-y divide-white/[0.06]">
    <div class="px-3.5 py-3 flex items-center justify-between gap-4">
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">Stealth Background Input</div>
        <div class="text-[11px] text-[#73737A] mt-0.5">
          Route clicks, typing, and hotkeys via COM UIAutomation &amp; child-HWND messages without moving your mouse or stealing focus.
        </div>
      </div>
      <button
        type="button"
        aria-label="Toggle Stealth Background Input"
        onclick={() => updatePolicy({ stealthMode: !policy.stealthMode })}
        class="w-9 h-5 rounded-full p-0.5 shrink-0 transition-colors {policy.stealthMode ? 'bg-[#8B5CF6]' : 'bg-[#27272A]'}"
      >
        <div class="w-4 h-4 rounded-full transition-transform {policy.stealthMode ? 'translate-x-4 bg-white' : 'translate-x-0 bg-[#73737A]'}"></div>
      </button>
    </div>

    <div class="px-3.5 py-3 flex items-center justify-between gap-4">
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">Stage Launched Apps Off-Screen</div>
        <div class="text-[11px] text-[#73737A] mt-0.5">
          Open agent-launched applications outside the visible monitor area so windows never pop up over your workspace.
        </div>
      </div>
      <button
        type="button"
        aria-label="Toggle Off-Screen Staging"
        onclick={() => updatePolicy({ autoStageOffscreen: !policy.autoStageOffscreen })}
        class="w-9 h-5 rounded-full p-0.5 shrink-0 transition-colors {policy.autoStageOffscreen ? 'bg-[#8B5CF6]' : 'bg-[#27272A]'}"
      >
        <div class="w-4 h-4 rounded-full transition-transform {policy.autoStageOffscreen ? 'translate-x-4 bg-white' : 'translate-x-0 bg-[#73737A]'}"></div>
      </button>
    </div>

    <div class="px-3.5 py-3 flex items-center justify-between gap-4">
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">Virtual Pointer Reticle</div>
        <div class="text-[11px] text-[#73737A] mt-0.5">
          Draw the virtual target pointer onto off-screen window captures for visual coordinate feedback.
        </div>
      </div>
      <button
        type="button"
        aria-label="Toggle Virtual Pointer Reticle"
        onclick={() => updatePolicy({ drawVirtualCursor: !policy.drawVirtualCursor })}
        class="w-9 h-5 rounded-full p-0.5 shrink-0 transition-colors {policy.drawVirtualCursor ? 'bg-[#8B5CF6]' : 'bg-[#27272A]'}"
      >
        <div class="w-4 h-4 rounded-full transition-transform {policy.drawVirtualCursor ? 'translate-x-4 bg-white' : 'translate-x-0 bg-[#73737A]'}"></div>
      </button>
    </div>

    <div class="px-3.5 py-3 flex items-center justify-between gap-4">
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">Off-Screen Viewport Resolution</div>
        <div class="text-[11px] text-[#73737A] mt-0.5">
          Default window dimensions assigned to newly staged background applications.
        </div>
      </div>
      <div class="flex items-center gap-1.5 shrink-0 font-mono text-xs">
        <input
          type="number"
          min="640"
          max="3840"
          value={policy.defaultWidth}
          onchange={(e) => updatePolicy({ defaultWidth: Number((e.currentTarget as HTMLInputElement).value) || 1280 })}
          class="w-18 h-7 px-2 rounded-[5px] bg-[#121214] border border-white/[0.08] text-[#EDEDEF] text-center outline-none focus:border-[#8B5CF6]/60"
        />
        <span class="text-[#66666D]">×</span>
        <input
          type="number"
          min="420"
          max="2160"
          value={policy.defaultHeight}
          onchange={(e) => updatePolicy({ defaultHeight: Number((e.currentTarget as HTMLInputElement).value) || 820 })}
          class="w-18 h-7 px-2 rounded-[5px] bg-[#121214] border border-white/[0.08] text-[#EDEDEF] text-center outline-none focus:border-[#8B5CF6]/60"
        />
      </div>
    </div>
  </div>

  {#if previewData}
    <div class="rounded-[6px] bg-[#121214] border border-white/[0.08] overflow-hidden">
      <div class="px-3.5 py-2 border-b border-white/[0.06] flex items-center justify-between gap-2">
        <div class="flex items-center gap-2 min-w-0">
          <span class="text-xs font-medium text-[#EDEDEF] truncate">{previewData.title}</span>
          <span class="font-mono text-[10px] text-[#73737A] shrink-0">{previewData.method}</span>
        </div>
        <div class="flex items-center gap-1.5 shrink-0">
          <button
            type="button"
            disabled={previewLoading}
            onclick={() => {
              const target = windows.find((w) => w.hwnd === previewData?.hwnd);
              if (target) loadPreview(target);
            }}
            class="px-2 py-0.5 rounded-[4px] bg-white/[0.06] hover:bg-white/[0.12] text-[11px] text-[#D4D4D8] transition-colors"
          >
            {previewLoading ? '...' : 'Update Frame'}
          </button>
          <button
            type="button"
            onclick={() => (previewData = null)}
            class="px-2 py-0.5 rounded-[4px] text-[11px] text-[#73737A] hover:text-[#EDEDEF] transition-colors"
          >
            Close
          </button>
        </div>
      </div>
      <div class="p-2.5 flex justify-center bg-[#0B0B0D]">
        <img
          src={previewData.dataUrl}
          alt={previewData.title}
          class="max-h-72 w-auto rounded-[4px] border border-white/[0.06] object-contain"
        />
      </div>
    </div>
  {/if}

  <div class="space-y-2">
    <div class="flex items-center justify-between">
      <span class="text-xs font-medium text-[#A3A3AA]">Controllable Windows ({windows.length})</span>
      <span class="font-mono text-[10.5px] text-[#66666D]">Panic abort: Ctrl+Alt+Shift+Esc</span>
    </div>

    <div class="rounded-[6px] bg-[#18181B]/70 border border-white/[0.07] divide-y divide-white/[0.05] max-h-72 overflow-y-auto">
      {#if windows.length === 0}
        <div class="p-4 text-center text-xs text-[#66666D]">No external windows detected</div>
      {:else}
        {#each windows as win (win.hwnd)}
          <div class="px-3 py-2 flex items-center justify-between gap-3 text-xs">
            <div class="flex items-center gap-2 min-w-0">
              <span class="text-[#EDEDEF] truncate">{win.title || win.processName}</span>
              <span class="font-mono text-[10.5px] text-[#66666D] shrink-0">{win.processName}</span>
              {#if win.isBound}
                <span class="px-1.5 py-0.5 rounded-[4px] border border-[#8B5CF6]/45 bg-[#8B5CF6]/12 font-mono text-[9.5px] leading-none text-[#DDD6FE] shrink-0">
                  bound
                </span>
              {/if}
              {#if win.isOffscreen || win.isStaged}
                <span class="px-1.5 py-0.5 rounded-[4px] border border-white/[0.1] bg-white/[0.05] font-mono text-[9.5px] leading-none text-[#A3A3AA] shrink-0">
                  off-screen
                </span>
              {/if}
            </div>

            <div class="flex items-center gap-1 shrink-0">
              <button
                type="button"
                onclick={() => loadPreview(win)}
                class="px-2 py-0.5 rounded-[4px] bg-white/[0.05] hover:bg-white/[0.1] text-[10.5px] text-[#D4D4D8] transition-colors"
              >
                Preview
              </button>
              <button
                type="button"
                disabled={busyHwnd === win.hwnd}
                onclick={() => toggleStageWindow(win)}
                class="px-2 py-0.5 rounded-[4px] bg-white/[0.05] hover:bg-white/[0.1] text-[10.5px] text-[#A3A3AA] hover:text-[#EDEDEF] transition-colors disabled:opacity-50"
              >
                {win.isOffscreen || win.isStaged ? 'Show' : 'Hide'}
              </button>
              <button
                type="button"
                disabled={busyHwnd === win.hwnd}
                onclick={() => closeBgWindow(win)}
                class="px-2 py-0.5 rounded-[4px] text-[10.5px] text-[#73737A] hover:text-[#F87171] hover:bg-[#EF4444]/10 transition-colors disabled:opacity-50"
              >
                Close
              </button>
            </div>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</div>

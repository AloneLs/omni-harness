<script lang="ts">
import { onMount } from 'svelte';
import { scale, fade } from 'svelte/transition';
import { Spring } from 'svelte/motion';
import { invoke } from '@tauri-apps/api/core';
import DotGridCanvas from './DotGridCanvas.svelte';
import Icon from './Icon.svelte';
import { settingsStore } from '../lib/stores/settings.svelte';
import { SPRING_POPUP, attachPressScale } from '../lib/motion';

let {
  isOpen = true,
  onComplete = () => {}
}: {
  isOpen?: boolean;
  onComplete?: () => void;
} = $props();

let installDir = $state<string>('');
let createShortcut = $state<boolean>(true);
let autoStart = $state<boolean>(false);
let stage = $state<'config' | 'launching'>('config');
let errorMsg = $state<string | null>(null);

const overlaySpring = new Spring({ opacity: 0, scale: 0.96, y: 14 }, SPRING_POPUP);

onMount(async () => {
  try {
    const defDir = await invoke<string>('get_default_install_dir');
    if (defDir) installDir = defDir;
  } catch {}
  if (isOpen) {
    overlaySpring.target = { opacity: 1, scale: 1, y: 0 };
  }
});

$effect(() => {
  if (!isOpen) {
    overlaySpring.target = { opacity: 0, scale: 1.04, y: -12 };
  } else if (stage === 'launching') {
    overlaySpring.target = { opacity: 0, scale: 1.05, y: -14 };
  } else {
    overlaySpring.target = { opacity: 1, scale: 1, y: 0 };
  }
});

async function browseInstallDir() {
  try {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select Installation Directory'
    });
    if (typeof selected === 'string' && selected) {
      installDir = selected;
    }
  } catch {}
}

async function finishSetup() {
  if (!installDir.trim()) {
    errorMsg = 'Select an installation directory.';
    return;
  }
  errorMsg = null;
  stage = 'launching';
  try {
    let targetExe: string | null = null;
    try {
      targetExe = await invoke<string>('install_app_to_dir', { installDir: installDir.trim() });
    } catch {}

    const cleanInstall = installDir.trim().replace(/[\\/]+$/, '');
    await invoke('complete_first_run', {
      workspaceDir: `${cleanInstall}\\Workspace`,
      defaultModelId: null
    });

    if (createShortcut) {
      try {
        await invoke('create_desktop_shortcut', { targetPath: targetExe });
      } catch {}
    }

    try {
      await invoke('set_windows_autostart', { enabled: autoStart, targetPath: targetExe });
    } catch {}

    settingsStore.data.firstRunCompleted = true;
    await settingsStore.saveSettings();

    setTimeout(() => {
      onComplete();
    }, 340);
  } catch (err: unknown) {
    stage = 'config';
    errorMsg = err instanceof Error ? err.message : String(err);
  }
}
</script>

{#if isOpen || overlaySpring.current.opacity > 0.02}
  <div
    class="fixed inset-0 z-[250] flex items-center justify-center bg-[#101012] p-6 select-none overflow-hidden {isOpen ? 'pointer-events-auto' : 'pointer-events-none'}"
    style="opacity: {overlaySpring.current.opacity};"
  >
    <DotGridCanvas active={stage === 'launching'} />

    <div
      class="relative z-10 w-full max-w-[460px] bg-[#161618]/95 border border-white/[0.08] rounded-[10px] p-6 space-y-5 text-[13px] text-[#E5E5E8]"
      style="transform: translateY({overlaySpring.current.y}px) scale({overlaySpring.current.scale});"
    >
      {#if stage === 'launching'}
        <div in:scale={{ start: 0.9, duration: 220 }} class="py-10 flex flex-col items-center justify-center gap-3 text-center">
          <div class="w-10 h-10 rounded-full bg-[#EDEDEF] text-[#111113] flex items-center justify-center">
            <Icon name="check" class="w-5 h-5" strokeWidth={2.2} />
          </div>
          <div class="text-sm font-medium text-[#EDEDEF]">Ready</div>
          <div class="font-mono text-[11px] text-[#73737A]">Launching OmniHarness...</div>
        </div>
      {:else}
        <div in:fade={{ duration: 150 }} class="space-y-5">
          <div class="space-y-1">
            <div class="font-mono text-[10.5px] text-[#73737A] uppercase tracking-wider">OmniHarness // Setup</div>
            <h1 class="text-base font-medium text-[#EDEDEF]">Install & Configure</h1>
            <p class="text-xs text-[#8E8E95] leading-relaxed">
              Choose where OmniHarness will be installed on your system.
            </p>
          </div>

          <div class="space-y-1.5">
            <span class="text-xs text-[#A3A3AA] block">Installation Directory</span>
            <div class="flex items-center gap-2">
              <input
                type="text"
                bind:value={installDir}
                placeholder="C:\Users\...\AppData\Local\OmniHarness"
                class="flex-1 px-3 py-2 rounded-[8px] bg-[#121214] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none focus:border-white/[0.2]"
              />
              <button
                type="button"
                class="h-8 px-3 rounded-[8px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-xs text-[#E5E5E8] shrink-0 active:scale-[0.97] transition-all"
                onclick={browseInstallDir}
              >
                Browse
              </button>
            </div>
          </div>

          <div class="space-y-2">
            <button
              type="button"
              class="w-full px-3.5 py-2.5 rounded-[8px] bg-[#121214] border border-white/[0.06] flex items-center justify-between text-left hover:border-white/[0.12] transition-colors"
              onclick={() => (createShortcut = !createShortcut)}
            >
              <div>
                <div class="text-xs text-[#EDEDEF] font-medium">Create desktop shortcut</div>
                <div class="text-[11px] text-[#73737A]">Add OmniHarness.lnk to Desktop</div>
              </div>
              <div class="w-9 h-5 rounded-full p-0.5 transition-colors {createShortcut ? 'bg-[#EDEDEF]' : 'bg-[#27272A]'}">
                <div class="w-4 h-4 rounded-full transition-transform {createShortcut ? 'translate-x-4 bg-[#111113]' : 'translate-x-0 bg-[#73737A]'}"></div>
              </div>
            </button>

            <button
              type="button"
              class="w-full px-3.5 py-2.5 rounded-[8px] bg-[#121214] border border-white/[0.06] flex items-center justify-between text-left hover:border-white/[0.12] transition-colors"
              onclick={() => (autoStart = !autoStart)}
            >
              <div>
                <div class="text-xs text-[#EDEDEF] font-medium">Launch on Windows startup</div>
                <div class="text-[11px] text-[#73737A]">Start quietly in system tray on login</div>
              </div>
              <div class="w-9 h-5 rounded-full p-0.5 transition-colors {autoStart ? 'bg-[#EDEDEF]' : 'bg-[#27272A]'}">
                <div class="w-4 h-4 rounded-full transition-transform {autoStart ? 'translate-x-4 bg-[#111113]' : 'translate-x-0 bg-[#73737A]'}"></div>
              </div>
            </button>
          </div>

          {#if errorMsg}
            <div class="px-3 py-2 rounded-[8px] bg-[#1C1C1F] border border-[#EF4444]/40 text-xs text-[#EF4444]">
              {errorMsg}
            </div>
          {/if}

          <div class="flex justify-end pt-1">
            <button
              type="button"
              class="h-8 px-4 rounded-[8px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium transition-all"
              onclick={finishSetup}
              use:attachPressScale
            >
              Install & Launch
            </button>
          </div>
        </div>
      {/if}
    </div>
  </div>
{/if}
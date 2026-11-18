<script lang="ts">
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface PairedChat {
  chatId: string;
  name: string;
  pairedAt: string;
  role: 'admin' | 'observer';
}

let botToken = $state<string>('');
let showBotToken = $state<boolean>(false);
let scrambling = $state<boolean>(false);
let scrambleText = $state<string>('');
let botRunning = $state<boolean>(false);
let botStatus = $state<'stopped' | 'connecting' | 'online' | 'error'>('stopped');
let botErrorMessage = $state<string | null>(null);

let pairingCode = $state<string>('000000');
let pairingCodeExpiresIn = $state<number>(300);
let rollingCode = $state<boolean>(false);
let copiedPair = $state<boolean>(false);

let pairedChats = $state<PairedChat[]>([]);

let notifyOnComplete = $state<boolean>(true);
let notifyOnApproval = $state<boolean>(true);
let notifyOnError = $state<boolean>(true);
let autoStart = $state<boolean>(false);
let proxyUrl = $state<string>('');
let feedbackMessage = $state<{ type: 'success' | 'error'; text: string } | null>(null);

async function refreshTelegramConfig(syncForm = false) {
  try {
    const cfg = await invoke<{
      botToken?: string;
      pairingCode?: string;
      pairedChats?: PairedChat[];
      isRunning?: boolean;
      notifyOnComplete?: boolean;
      notifyOnApproval?: boolean;
      notifyOnError?: boolean;
      autoStart?: boolean;
      proxyUrl?: string;
    }>('get_telegram_config');
    if (cfg.pairingCode) pairingCode = cfg.pairingCode;
    if (Array.isArray(cfg.pairedChats)) pairedChats = cfg.pairedChats;
    if (syncForm) {
      if (typeof cfg.botToken === 'string' && (!botToken || cfg.botToken)) botToken = cfg.botToken;
      if (typeof cfg.notifyOnComplete === 'boolean') notifyOnComplete = cfg.notifyOnComplete;
      if (typeof cfg.notifyOnApproval === 'boolean') notifyOnApproval = cfg.notifyOnApproval;
      if (typeof cfg.notifyOnError === 'boolean') notifyOnError = cfg.notifyOnError;
      if (typeof cfg.autoStart === 'boolean') autoStart = cfg.autoStart;
      if (typeof cfg.proxyUrl === 'string' && (!proxyUrl || cfg.proxyUrl)) proxyUrl = cfg.proxyUrl;
    }
    botRunning = Boolean(cfg.isRunning);
    botStatus = botRunning ? 'online' : 'stopped';
  } catch {}
}

onMount(() => {
  refreshTelegramConfig(true);
  let unlistenCfg: (() => void) | null = null;
  listen('telegram-config-updated', () => {
    refreshTelegramConfig(false);
  }).then((fn) => (unlistenCfg = fn)).catch(() => {});
  const poll = setInterval(() => {
    if (botRunning) refreshTelegramConfig(false);
  }, 2500);
  return () => {
    clearInterval(poll);
    if (unlistenCfg) unlistenCfg();
  };
});

$effect(() => {
  const timer = setInterval(() => {
    if (pairingCodeExpiresIn > 0) pairingCodeExpiresIn--;
  }, 1000);
  return () => clearInterval(timer);
});

let formattedExpiration = $derived(
  `${Math.floor(pairingCodeExpiresIn / 60)}:${(pairingCodeExpiresIn % 60).toString().padStart(2, '0')}`
);

let ringDashOffset = $derived(50.26 * (1 - pairingCodeExpiresIn / 300));

function toggleShowToken() {
  if (!showBotToken && botToken.length > 0) {
    scrambling = true;
    const chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*';
    let ticks = 0;
    const iv = setInterval(() => {
      scrambleText = Array.from({ length: Math.min(28, botToken.length) }, () =>
        chars[Math.floor(Math.random() * chars.length)]
      ).join('');
      ticks++;
      if (ticks >= 5) {
        clearInterval(iv);
        scrambling = false;
        showBotToken = true;
      }
    }, 30);
  } else {
    showBotToken = !showBotToken;
  }
}

async function generateNewPairingCode() {
  rollingCode = true;
  try {
    const code = await invoke<string>('generate_pairing_code');
    pairingCode = code;
    pairingCodeExpiresIn = 300;
  } catch {}
  rollingCode = false;
}

async function copyPairingCommand() {
  await navigator.clipboard.writeText(`/pair ${pairingCode}`);
  copiedPair = true;
  setTimeout(() => (copiedPair = false), 1500);
}

async function removePairedChat(chatId: string) {
  try {
    await invoke('remove_telegram_paired_chat', { chatId });
    pairedChats = pairedChats.filter((c) => c.chatId !== chatId);
  } catch (err: unknown) {
    feedbackMessage = { type: 'error', text: err instanceof Error ? err.message : String(err) };
  }
}

async function toggleBot() {
  feedbackMessage = null;
  botErrorMessage = null;
  if (botRunning) {
    try {
      await invoke('stop_telegram_bot');
      botRunning = false;
      botStatus = 'stopped';
    } catch (err: unknown) {
      botStatus = 'error';
      botErrorMessage = err instanceof Error ? err.message : String(err);
    }
  } else {
    if (!botToken.trim()) {
      feedbackMessage = { type: 'error', text: 'Bot token cannot be empty' };
      return;
    }
    botStatus = 'connecting';
    try {
      await refreshTelegramConfig(false);
      await invoke('save_telegram_config', {
        token: botToken.trim(),
        pairedChats,
        notifyOnComplete,
        notifyOnApproval,
        notifyOnError,
        autoStart,
        proxyUrl: proxyUrl.trim()
      });
      await invoke('start_telegram_bot', {
        token: botToken.trim(),
        allowedChats: pairedChats.map((c) => c.chatId)
      });
      botRunning = true;
      botStatus = 'online';
    } catch (err: unknown) {
      botRunning = false;
      botStatus = 'error';
      botErrorMessage = err instanceof Error ? err.message : String(err);
    }
  }
}

async function saveBotSettings() {
  try {
    await refreshTelegramConfig(false);
    await invoke('save_telegram_config', {
      token: botToken.trim(),
      pairedChats,
      notifyOnComplete,
      notifyOnApproval,
      notifyOnError,
      autoStart,
      proxyUrl: proxyUrl.trim()
    });
    await refreshTelegramConfig(true);
    feedbackMessage = { type: 'success', text: 'Configuration saved' };
  } catch (err: unknown) {
    feedbackMessage = { type: 'error', text: err instanceof Error ? err.message : String(err) };
  }
}
</script>

<div class="space-y-5 text-[13px] text-[#E5E5E8]">
  <div class="flex items-center justify-between gap-4">
    <div class="flex items-center gap-3 min-w-0">
      <h2 class="text-sm font-medium text-[#EDEDEF] whitespace-nowrap">Telegram Remote</h2>
      <span class="px-2 py-0.5 rounded-[4px] bg-[#1C1C1F] border border-white/[0.07] font-mono text-[10px] flex items-center gap-1.5 whitespace-nowrap">
        <span class="w-1.5 h-1.5 rounded-full {botStatus === 'online' ? 'bg-[#22C55E]' : botStatus === 'connecting' ? 'bg-[#F59E0B] animate-pulse' : botStatus === 'error' ? 'bg-[#EF4444]' : 'bg-[#66666D]'}"></span>
        <span class="text-[#A3A3AA]">
          {botStatus === 'online' ? 'ONLINE' : botStatus === 'connecting' ? 'CONNECTING...' : botStatus === 'error' ? 'ERROR' : 'STOPPED'}
        </span>
      </span>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#1C1C1F] hover:bg-[#242428] border border-white/[0.08] border-t-white/[0.14] text-xs font-medium text-[#E5E5E8] whitespace-nowrap transition-colors"
        onclick={toggleBot}
      >
        {botRunning ? 'Stop Bot' : 'Start Telegram Bot'}
      </button>
      <button
        type="button"
        class="h-8 px-3.5 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium whitespace-nowrap transition-colors"
        onclick={saveBotSettings}
      >
        Save Settings
      </button>
    </div>
  </div>

  {#if feedbackMessage || botErrorMessage}
    <div class="px-3.5 py-2 rounded-[6px] bg-[#1C1C1F] border border-white/[0.07] text-xs {feedbackMessage?.type === 'error' || botErrorMessage ? 'text-[#F87171]' : 'text-[#A3A3AA]'}">
      {botErrorMessage || feedbackMessage?.text}
    </div>
  {/if}

  <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] space-y-3">
    <span class="text-xs font-medium text-[#EDEDEF] block">Telegram Bot API Token</span>
    <div class="flex items-center gap-2">
      {#if scrambling}
        <div class="flex-1 px-3 py-2 rounded-[6px] bg-[#1E1E22] border border-white/[0.08] font-mono text-xs text-[#A3A3AA] truncate">
          {scrambleText}
        </div>
      {:else}
        <input
          bind:value={botToken}
          type={showBotToken ? 'text' : 'password'}
          placeholder="123456789:ABCdefGHI..."
          class="flex-1 px-3 py-2 rounded-[6px] bg-[#1E1E22] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none focus:border-white/[0.2]"
        />
      {/if}
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#121214] hover:bg-[#242428] border border-white/[0.07] text-xs text-[#A3A3AA] hover:text-white whitespace-nowrap transition-colors"
        onclick={toggleShowToken}
      >
        {showBotToken ? 'Hide' : 'Show'}
      </button>
    </div>
  </div>

  <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] space-y-2">
    <span class="text-xs font-medium text-[#EDEDEF] block">Telegram Proxy URL (optional)</span>
    <input
      bind:value={proxyUrl}
      type="text"
      placeholder="socks5://127.0.0.1:10808 or http://127.0.0.1:7890"
      class="w-full px-3 py-2 rounded-[6px] bg-[#1E1E22] border border-white/[0.08] font-mono text-xs text-[#E5E5E8] placeholder-[#55555C] outline-none focus:border-white/[0.2]"
    />
  </div>

  <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] flex items-center justify-between gap-4">
    <div class="space-y-1">
      <div class="text-xs font-medium text-[#EDEDEF]">Device Pairing Code</div>
      <div class="flex items-center gap-3">
        <span class="font-mono text-lg tracking-[0.2em] text-[#EDEDEF] {rollingCode ? 'opacity-70' : ''}">
          {pairingCode.slice(0, 3)} {pairingCode.slice(3)}
        </span>
        <div class="flex items-center gap-1.5 text-[11px] font-mono text-[#73737A]">
          <svg class="w-4 h-4 -rotate-90" viewBox="0 0 20 20">
            <circle cx="10" cy="10" r="8" fill="none" stroke="rgba(255,255,255,0.08)" stroke-width="2" />
            <circle
              cx="10"
              cy="10"
              r="8"
              fill="none"
              stroke="#A3A3AA"
              stroke-width="2"
              stroke-dasharray="50.26"
              stroke-dashoffset={ringDashOffset}
            />
          </svg>
          <span>Expires in {formattedExpiration}</span>
        </div>
      </div>
    </div>

    <div class="flex items-center gap-2 shrink-0">
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#121214] hover:bg-[#242428] border border-white/[0.07] text-xs text-[#A3A3AA] hover:text-white whitespace-nowrap transition-colors"
        onclick={generateNewPairingCode}
      >
        Refresh
      </button>
      <button
        type="button"
        class="h-8 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium whitespace-nowrap transition-all"
        onclick={copyPairingCommand}
      >
        {copiedPair ? 'Copied ✓' : 'Copy /pair'}
      </button>
    </div>
  </div>

  {#if pairedChats.length > 0}
    <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] space-y-2.5">
      <div class="text-xs font-medium text-[#EDEDEF]">Paired Chats</div>
      <div class="space-y-1.5">
        {#each pairedChats as chat}
          <div class="flex items-center justify-between px-3 py-2 rounded-[6px] bg-[#121214] border border-white/[0.05]">
            <div class="flex items-baseline gap-2.5 min-w-0">
              <span class="text-xs leading-none text-[#EDEDEF] truncate">{chat.name}</span>
              <span class="font-mono text-[11px] leading-none text-[#73737A] shrink-0">{chat.chatId}</span>
            </div>
            <button
              type="button"
              class="text-[11px] text-[#73737A] hover:text-[#F87171] transition-colors"
              onclick={() => removePairedChat(chat.chatId)}
            >
              Remove
            </button>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] space-y-3">
    <div class="text-xs font-medium text-[#EDEDEF]">Push Notification Triggers</div>
    <div class="space-y-2.5">
      <label class="flex items-center justify-between cursor-pointer">
        <span class="text-xs text-[#A3A3AA]">Notify when autonomous task completes</span>
        <button
          type="button"
          class="w-8 h-4.5 rounded-full p-0.5 transition-colors {notifyOnComplete ? 'bg-[#EDEDEF]' : 'bg-[#121214] border border-white/[0.12]'}"
          onclick={() => (notifyOnComplete = !notifyOnComplete)}
          aria-label="Toggle notify on complete"
        >
          <div class="w-3.5 h-3.5 rounded-full transition-transform {notifyOnComplete ? 'translate-x-3.5 bg-[#111113]' : 'translate-x-0 bg-[#73737A]'}"></div>
        </button>
      </label>

      <label class="flex items-center justify-between cursor-pointer">
        <span class="text-xs text-[#A3A3AA]">Notify on approval or blocker prompt</span>
        <button
          type="button"
          class="w-8 h-4.5 rounded-full p-0.5 transition-colors {notifyOnApproval ? 'bg-[#EDEDEF]' : 'bg-[#121214] border border-white/[0.12]'}"
          onclick={() => (notifyOnApproval = !notifyOnApproval)}
          aria-label="Toggle notify on approval"
        >
          <div class="w-3.5 h-3.5 rounded-full transition-transform {notifyOnApproval ? 'translate-x-3.5 bg-[#111113]' : 'translate-x-0 bg-[#73737A]'}"></div>
        </button>
      </label>

      <label class="flex items-center justify-between cursor-pointer">
        <span class="text-xs text-[#A3A3AA]">Notify on runtime execution error</span>
        <button
          type="button"
          class="w-8 h-4.5 rounded-full p-0.5 transition-colors {notifyOnError ? 'bg-[#EDEDEF]' : 'bg-[#121214] border border-white/[0.12]'}"
          onclick={() => (notifyOnError = !notifyOnError)}
          aria-label="Toggle notify on error"
        >
          <div class="w-3.5 h-3.5 rounded-full transition-transform {notifyOnError ? 'translate-x-3.5 bg-[#111113]' : 'translate-x-0 bg-[#73737A]'}"></div>
        </button>
      </label>

      <label class="flex items-center justify-between cursor-pointer pt-1 border-t border-white/[0.05]">
        <span class="text-xs text-[#EDEDEF]">Auto-start bot on app launch</span>
        <button
          type="button"
          class="w-8 h-4.5 rounded-full p-0.5 transition-colors {autoStart ? 'bg-[#EDEDEF]' : 'bg-[#121214] border border-white/[0.12]'}"
          onclick={() => {
            autoStart = !autoStart;
            if (botToken.trim()) saveBotSettings();
          }}
          aria-label="Toggle auto-start bot on launch"
        >
          <div class="w-3.5 h-3.5 rounded-full transition-transform {autoStart ? 'translate-x-3.5 bg-[#111113]' : 'translate-x-0 bg-[#73737A]'}"></div>
        </button>
      </label>
    </div>
  </div>
</div>
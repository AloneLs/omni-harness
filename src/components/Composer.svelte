<script lang="ts">
import { onMount, tick } from 'svelte';
import { Spring } from 'svelte/motion';
import { fly, slide, scale } from 'svelte/transition';
import { cubicOut } from 'svelte/easing';
import Icon from './Icon.svelte';
import FileToolIcon from './FileToolIcon.svelte';
import { themeStore } from '../lib/stores/theme.svelte';
import { chatStore, type ChatAttachment } from '../lib/stores/chat.svelte';
import { renderMarkdown } from '../lib/markdown';
import { settingsStore, groupProviderModels, getSupportedReasoningLevels, getDisplayModelId, REASONING_ORDER, type ReasoningLevel } from '../lib/stores/settings.svelte';
import { workspaceStore } from '../lib/stores/workspace.svelte';
import { VoiceCapture } from '../lib/voiceCapture';
import { SPRING_POPUP, SPRING_KEYCAP, SPRING_SLIDER, attachPressScale } from '../lib/motion';
import { invoke, convertFileSrc } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { ProviderConfig } from '../lib/stores/settings.svelte';

interface ComposerOAuthAccount {
  id: string;
  provider: string;
  enabled: boolean;
  cooldownUntilMs?: number | null;
  quotaLabel?: string | null;
}

let inputText = $state<string>(chatStore.getDraftText(chatStore.activeChatId));
let lastSyncedDraftChatId = $state<string | null | undefined>(chatStore.activeChatId);
let textareaEl = $state<HTMLTextAreaElement | null>(null);
let mirrorEl = $state<HTMLDivElement | null>(null);
let slashLeadMatch = $derived(inputText.match(/^(\/[a-zA-Z0-9_-]+)([\s\S]*)$/));
let composerRootEl = $state<HTMLDivElement | null>(null);

let showPlusMenu = $state<boolean>(false);
let isDraggingOver = $state<boolean>(false);
let attachments = $state<ChatAttachment[]>(chatStore.getDraftAttachments(chatStore.activeChatId));
let showModelPopup = $state<boolean>(false);
let showProjectPopup = $state<boolean>(false);
let activePalette = $state<'slash' | 'at' | 'dollar' | null>(null);
let paletteFilter = $state<string>('');
let paletteSelectedIndex = $state<number>(0);
let planFeedbackOpen = $state<boolean>(false);
let planFeedbackText = $state<string>('');
let planExpanded = $state<boolean>(true);
let sideQuestionMaxHeight = $state<number>(224);
let isResizingSideQuestion = $state<boolean>(false);
let loadedSkills = $state<{ name: string; command: string; desc: string }[]>([]);
let oauthAccounts = $state<ComposerOAuthAccount[]>([]);

async function refreshOAuthTiers() {
  try {
    const res = await invoke<ComposerOAuthAccount[]>('oauth_list_accounts', { provider: null });
    if (Array.isArray(res)) {
      oauthAccounts = res;
    }
  } catch {}
}

function tierRank(label: string): number {
  const l = label.toLowerCase();
  if (l.includes('ultra') || l === 'gpt pro' || l.includes('max')) return 4;
  if (l.includes('enterprise') || l.includes('business') || l.includes('team') || l.includes('edu')) return 3;
  if (l.includes('pro') || l.includes('plus')) return 2;
  if (l.includes('go') || l.includes('standard')) return 1;
  return 0;
}

function providerTierLabel(prov?: ProviderConfig | null): string {
  if (!prov?.apiProtocol) return '';
  const matching = oauthAccounts.filter((a) => a.provider === prov.apiProtocol && a.quotaLabel);
  if (matching.length === 0) return '';
  const active = matching.filter((a) => a.enabled);
  const pool = active.length > 0 ? active : matching;
  let best = pool[0].quotaLabel || '';
  for (const acc of pool) {
    const lbl = acc.quotaLabel || '';
    if (tierRank(lbl) > tierRank(best)) best = lbl;
  }
  return best;
}

function tierBadgeClass(label: string): string {
  const l = label.toLowerCase();
  if (l.includes('ultra') || l === 'gpt pro' || l.includes('max')) {
    return 'border-[#8B5CF6]/45 bg-[#8B5CF6]/12 text-[#DDD6FE]';
  }
  if (l.includes('pro') || l.includes('plus') || l.includes('team') || l.includes('business') || l.includes('enterprise') || l.includes('edu') || l.includes('go')) {
    return 'border-white/[0.12] bg-white/[0.06] text-[#E5E5E8]';
  }
  return 'border-white/[0.06] bg-white/[0.03] text-[#8E8E95]';
}

async function refreshLoadedSkills() {
  try {
    const res = await invoke<any[]>('list_skills');
    if (Array.isArray(res)) {
      loadedSkills = res
        .map((r) => {
          const meta = r.metadata || r;
          const name = String(meta.name || '').trim();
          const rawCmd = meta.command ? String(meta.command).trim() : '';
          const cmd = rawCmd ? (rawCmd.startsWith('/') ? rawCmd : `/${rawCmd}`) : `/${name}`;
          return {
            name,
            command: cmd,
            desc: String(meta.description || '').trim()
          };
        })
        .filter((s) => s.name && !s.name.startsWith('system:'));
    }
  } catch {}
}

function startSideQuestionResize(e: PointerEvent) {
  e.preventDefault();
  isResizingSideQuestion = true;
  const startY = e.clientY;
  const startH = sideQuestionMaxHeight;
  const onMove = (mv: PointerEvent) => {
    const delta = startY - mv.clientY;
    const maxAllowed = typeof window !== 'undefined' ? Math.min(window.innerHeight * 0.68, 620) : 540;
    sideQuestionMaxHeight = Math.max(110, Math.min(maxAllowed, startH + delta));
  };
  const onUp = () => {
    isResizingSideQuestion = false;
    window.removeEventListener('pointermove', onMove);
    window.removeEventListener('pointerup', onUp);
  };
  window.addEventListener('pointermove', onMove);
  window.addEventListener('pointerup', onUp);
}

const REASONING_LABELS: Record<ReasoningLevel, string> = {
  OFF: 'OFF',
  LOW: 'LOW',
  MEDIUM: 'MED',
  HIGH: 'HIGH',
  MAX: 'MAX'
};

let currentChatSession = $derived(chatStore.activeSession);
let currentProviderId = $derived(currentChatSession?.providerId || settingsStore.data.activeProvider);
let currentProvider = $derived(settingsStore.data.providers[currentProviderId] || settingsStore.currentProvider);
let currentModelId = $derived(currentChatSession?.modelId || settingsStore.data.activeModel);

let supportedReasoningLevels = $derived(
  getSupportedReasoningLevels(currentProvider, currentModelId)
);

let displayActiveModel = $derived(
  getDisplayModelId(currentProvider, currentModelId)
);

let reasoningIndex = $derived(
  Math.max(0, supportedReasoningLevels.indexOf(settingsStore.data.reasoningLevel))
);

$effect(() => {
  if (supportedReasoningLevels.length > 0 && !supportedReasoningLevels.includes(settingsStore.data.reasoningLevel)) {
    const targetIdx = REASONING_ORDER.indexOf(settingsStore.data.reasoningLevel);
    let best = supportedReasoningLevels[0];
    let bestDist = Infinity;
    for (const lvl of supportedReasoningLevels) {
      const d = Math.abs(REASONING_ORDER.indexOf(lvl) - targetIdx);
      if (d < bestDist) {
        bestDist = d;
        best = lvl;
      }
    }
    if (best) settingsStore.setReasoningLevel(best);
  }
});

const plusMenuSpring = new Spring({ opacity: 0, y: 6, scale: 0.96 }, SPRING_POPUP);
const projectMenuSpring = new Spring({ opacity: 0, scale: 0.95, y: 6 }, SPRING_POPUP);
const modelMenuSpring = new Spring({ opacity: 0, y: 6, scale: 0.96 }, SPRING_POPUP);
const paletteSpring = new Spring({ opacity: 0, y: 6 }, SPRING_POPUP);
const keycapSpring = new Spring({ scale: 1, y: 0 }, SPRING_KEYCAP);
const sliderSpring = new Spring(0, SPRING_SLIDER);

$effect(() => {
  const cid = chatStore.activeChatId;
  if (lastSyncedDraftChatId !== cid) {
    lastSyncedDraftChatId = cid;
    inputText = chatStore.getDraftText(cid);
    attachments = chatStore.getDraftAttachments(cid);
    setTimeout(adjustHeight, 0);
  }
});

$effect(() => {
  const cid = chatStore.activeChatId;
  if (lastSyncedDraftChatId === cid) {
    chatStore.setDraftText(cid, inputText);
    chatStore.setDraftAttachments(cid, attachments);
  }
});

$effect(() => {
  sliderSpring.target = reasoningIndex;
});

$effect(() => {
  plusMenuSpring.target = showPlusMenu
    ? { opacity: 1, y: 0, scale: 1 }
    : { opacity: 0, y: 6, scale: 0.96 };
});

$effect(() => {
  projectMenuSpring.target = showProjectPopup
    ? { opacity: 1, scale: 1, y: 0 }
    : { opacity: 0, scale: 0.95, y: 6 };
  modelMenuSpring.target = showModelPopup
    ? { opacity: 1, y: 0, scale: 1 }
    : { opacity: 0, y: 6, scale: 0.96 };
});

$effect(() => {
  paletteSpring.target = activePalette ? { opacity: 1, y: 0 } : { opacity: 0, y: 6 };
});


function addDroppedPaths(paths: string[]) {
  for (const filePath of paths) {
    if (typeof filePath !== 'string' || !filePath.trim()) continue;
    const name = filePath.split(/[\\/]/).pop() || filePath;
    const kind = detectAttachmentKind(name);
    const previewUrl = kind === 'image' || kind === 'video' ? convertFileSrc(filePath) : undefined;
    if (!attachments.some((a) => a.path === filePath)) {
      attachments = [
        ...attachments,
        {
          id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          name,
          path: filePath,
          kind,
          previewUrl
        }
      ];
    }
  }
}

function handleHtmlDrop(e: DragEvent) {
  e.preventDefault();
  isDraggingOver = false;
  const files = e.dataTransfer?.files;
  if (!files || files.length === 0) return;
  for (const file of Array.from(files)) {
    const anyFile = file as any;
    if (typeof anyFile.path === 'string' && anyFile.path) {
      addDroppedPaths([anyFile.path]);
      continue;
    }
    const kind = detectAttachmentKind(file.name);
    if (kind === 'image') {
      const reader = new FileReader();
      reader.onload = () => {
        attachments = [
          ...attachments,
          {
            id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
            name: file.name,
            path: file.name,
            size: file.size,
            kind,
            previewUrl: typeof reader.result === 'string' ? reader.result : undefined
          }
        ];
      };
      reader.readAsDataURL(file);
    } else {
      attachments = [
        ...attachments,
        {
          id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          name: file.name,
          path: file.name,
          size: file.size,
          kind
        }
      ];
    }
  }
}

onMount(() => {
  refreshLoadedSkills();
  refreshOAuthTiers();
  refreshWhisperAvailability();
  const onWhisperUpdated = () => refreshWhisperAvailability();
  window.addEventListener('omni-whisper-updated', onWhisperUpdated);
  let unlisten: (() => void) | null = null;
  let unlistenOAuth: (() => void) | null = null;
  listen('oauth-accounts-updated', () => {
    refreshOAuthTiers();
  }).then((fn) => (unlistenOAuth = fn)).catch(() => {});
  import('@tauri-apps/api/webview')
    .then(async ({ getCurrentWebview }) => {
      unlisten = await getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === 'over' || event.payload.type === 'enter') {
          isDraggingOver = true;
        } else if (event.payload.type === 'leave') {
          isDraggingOver = false;
        } else if (event.payload.type === 'drop') {
          isDraggingOver = false;
          if (Array.isArray(event.payload.paths) && event.payload.paths.length > 0) {
            addDroppedPaths(event.payload.paths);
          }
        }
      });
    })
    .catch(() => {});
  return () => {
    window.removeEventListener('omni-whisper-updated', onWhisperUpdated);
    if (unlisten) unlisten();
    if (unlistenOAuth) unlistenOAuth();
  };
});

function detectAttachmentKind(name: string): 'image' | 'video' | 'file' {
  const ext = name.split('.').pop()?.toLowerCase() || '';
  if (['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'svg'].includes(ext)) return 'image';
  if (['mp4', 'webm', 'mov', 'mkv', 'avi'].includes(ext)) return 'video';
  return 'file';
}

async function pickAttachments() {
  closeAllPopups();
  try {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const selected = await open({
      multiple: true,
      title: 'Attach files or media'
    });
    const paths = Array.isArray(selected) ? selected : selected ? [selected] : [];
    for (const filePath of paths) {
      if (typeof filePath !== 'string') continue;
      const name = filePath.split(/[\\/]/).pop() || filePath;
      const kind = detectAttachmentKind(name);
      const previewUrl = kind === 'image' || kind === 'video' ? convertFileSrc(filePath) : undefined;
      attachments = [
        ...attachments,
        {
          id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          name,
          path: filePath,
          kind,
          previewUrl
        }
      ];
    }
  } catch {}
}

function removeAttachment(id: string) {
  attachments = attachments.filter((a) => a.id !== id);
}

function handlePaste(e: ClipboardEvent) {
  const items = e.clipboardData?.items;
  if (!items) return;
  for (const item of items) {
    if (item.kind === 'file') {
      const file = item.getAsFile();
      if (!file) continue;
      const kind = detectAttachmentKind(file.name || 'pasted.png');
      if (kind === 'image') {
        const reader = new FileReader();
        reader.onload = () => {
          attachments = [
            ...attachments,
            {
              id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
              name: file.name || 'image.png',
              path: file.name || 'image.png',
              size: file.size,
              kind: 'image',
              previewUrl: typeof reader.result === 'string' ? reader.result : undefined
            }
          ];
        };
        reader.readAsDataURL(file);
      }
    }
  }
}


let contextPercent = $derived.by(() => {
  const { usedTokens, contextWindow } = chatStore.contextTelemetry;
  if (!usedTokens || !contextWindow) return 0;
  return Math.min(100, Math.round((usedTokens / contextWindow) * 100));
});

let contextTooltip = $derived.by(() => {
  const { usedTokens, contextWindow, cacheHitRate } = chatStore.contextTelemetry;
  const usedK = (usedTokens / 1000).toFixed(1);
  const winK = Math.round(contextWindow / 1000);
  const cacheStr = cacheHitRate > 0 ? ` · Cache ${Math.round(cacheHitRate)}%` : '';
  return `${usedK}k / ${winK}k (${contextPercent}%)${cacheStr}`;
});


function editQueuedItem(id: string) {
  const item = chatStore.pendingQueue.find((i) => i.id === id);
  if (!item) return;
  chatStore.dequeueMessage(id);
  inputText = item.text;
  setTimeout(() => {
    adjustHeight();
    textareaEl?.focus();
  }, 10);
}

function closeAllPopups() {
  showPlusMenu = false;
  showModelPopup = false;
  showProjectPopup = false;
  localPopupProviderId = null;
}

function handleWindowPointerDown(e: PointerEvent) {
  if (!composerRootEl) return;
  if (!composerRootEl.contains(e.target as Node)) {
    closeAllPopups();
    activePalette = null;
  }
}

let isVoiceActive = $state<boolean>(false);
let hasWhisperModel = $state<boolean>(false);

async function refreshWhisperAvailability() {
  try {
    const res = await invoke<Array<{ isDownloaded?: boolean; is_downloaded?: boolean }>>('get_speech_status');
    hasWhisperModel = Array.isArray(res) && res.some((m) => Boolean(m.isDownloaded ?? m.is_downloaded));
  } catch {
    hasWhisperModel = false;
  }
}
let voiceBands = $state<number[]>(new Array(28).fill(0.1));
let voiceTimerStr = $state<string>('00:00');
let voiceCaptureInstance = $state<VoiceCapture | null>(null);
let voiceRafId: number | null = null;
let voiceTimerInterval: number | null = null;

const SLASH_COMMANDS = [
  { cmd: '/goal', desc: 'Autonomous completion loop with blind evaluator (/goal <cond>, status, clear)' },
  { cmd: '/btw', desc: 'Ask a side question without interrupting or saving to history' },
  { cmd: '/plan', desc: 'Generate engineering execution plan' },
  { cmd: '/compact', desc: 'Compact message context history' },
  { cmd: '/clear', desc: 'Clear messages in active session' },
  { cmd: '/rollback', desc: 'Rollback modified files to last snapshot' },
  { cmd: '/model', desc: 'Switch primary language model' },
  { cmd: '/reasoning', desc: 'Adjust deep reasoning level' },
  { cmd: '/export', desc: 'Export session as Markdown' }
];

let skillItems = $derived(
  loadedSkills.length > 0
    ? loadedSkills
    : [{ name: 'skill-creator', command: '/skill-create', desc: 'Create and package new skills' }]
);

let enabledProviders = $derived(
  Object.values(settingsStore.data.providers).filter((p) => p.enabled)
);

let localPopupProviderId = $state<string | null>(null);

let popupProviderId = $derived.by(() => {
  if (localPopupProviderId && settingsStore.data.providers[localPopupProviderId]?.enabled) {
    return localPopupProviderId;
  }
  if (currentProviderId && settingsStore.data.providers[currentProviderId]?.enabled) {
    return currentProviderId;
  }
  const active = settingsStore.data.activeProvider;
  if (active && settingsStore.data.providers[active]?.enabled) return active;
  return enabledProviders[0]?.id || '';
});

let popupProviderIndex = $derived(
  Math.max(0, enabledProviders.findIndex((p) => p.id === popupProviderId))
);

const providerTabSpring = new Spring(0, SPRING_SLIDER);

$effect(() => {
  providerTabSpring.target = popupProviderIndex;
});

let availableModels = $derived.by(() => {
  const prov = settingsStore.data.providers[popupProviderId];
  if (!prov || !prov.enabled) return [];
  return groupProviderModels(prov.availableModels).map((g) => ({
    providerId: prov.id,
    model: g.displayId,
    rawIds: g.rawIds
  }));
});

let allSlashCommands = $derived.by(() => {
  const seen = new Set(SLASH_COMMANDS.map((c) => c.cmd.toLowerCase()));
  const skillCmds = skillItems
    .filter((s) => !seen.has(s.command.toLowerCase()))
    .map((s) => ({ cmd: s.command, desc: `Skill: ${s.desc}` }));
  return [...SLASH_COMMANDS, ...skillCmds];
});

let filteredSlash = $derived.by(() => {
  const q = paletteFilter.toLowerCase();
  return allSlashCommands.filter((c) => c.cmd.toLowerCase().includes(q) || c.desc.toLowerCase().includes(q));
});

let filteredFiles = $derived.by(() => {
  const q = paletteFilter.toLowerCase();
  return workspaceStore.searchResults.slice(0, 8).filter(
    (f) => f.name.toLowerCase().includes(q) || f.path.toLowerCase().includes(q)
  );
});

let filteredSkills = $derived.by(() => {
  const q = paletteFilter.toLowerCase();
  return skillItems.filter(
    (s) => s.name.toLowerCase().includes(q) || s.command.toLowerCase().includes(q) || s.desc.toLowerCase().includes(q)
  );
});

let dynamicButtonState = $derived.by<'send' | 'stop' | 'queue' | 'continue'>(() => {
  if (!chatStore.isStreaming) {
    if (chatStore.hasPausedTurn && inputText.trim().length === 0 && attachments.length === 0) {
      return 'continue';
    }
    return 'send';
  }
  if (inputText.trim().length === 0) return 'stop';
  return 'queue';
});

function adjustHeight() {
  if (!textareaEl) return;
  if (!inputText) {
    textareaEl.style.height = '24px';
    return;
  }
  textareaEl.style.height = '24px';
  const newHeight = Math.min(180, Math.max(24, textareaEl.scrollHeight));
  textareaEl.style.height = `${newHeight}px`;
}

$effect(() => {
  const _val = inputText;
  tick().then(() => adjustHeight());
});

function handleInput() {
  adjustHeight();
  checkPaletteTrigger();
}

function checkPaletteTrigger() {
  if (!textareaEl) return;
  const cursor = textareaEl.selectionStart;
  const textBefore = inputText.slice(0, cursor);
  const lastSlash = textBefore.lastIndexOf('/');
  const lastAt = textBefore.lastIndexOf('@');
  const lastDollar = textBefore.lastIndexOf('$');

  const maxTrigger = Math.max(lastSlash, lastAt, lastDollar);
  if (maxTrigger === -1 || (maxTrigger > 0 && textBefore[maxTrigger - 1] !== ' ' && textBefore[maxTrigger - 1] !== '\n')) {
    activePalette = null;
    paletteFilter = '';
    return;
  }

  const query = textBefore.slice(maxTrigger + 1);
  if (query.includes(' ') || query.includes('\n')) {
    activePalette = null;
    paletteFilter = '';
    return;
  }

  paletteFilter = query;
  paletteSelectedIndex = 0;

  if (maxTrigger === lastSlash) {
    activePalette = 'slash';
    refreshLoadedSkills();
  } else if (maxTrigger === lastAt) {
    activePalette = 'at';
    workspaceStore.searchFiles(query);
  } else if (maxTrigger === lastDollar) {
    activePalette = 'dollar';
    refreshLoadedSkills();
  }
}

function insertCompletion(text: string, triggerChar: string) {
  if (!textareaEl) return;
  const cursor = textareaEl.selectionStart;
  const textBefore = inputText.slice(0, cursor);
  const triggerIdx = textBefore.lastIndexOf(triggerChar);

  if (triggerIdx !== -1) {
    const beforeTrigger = inputText.slice(0, triggerIdx);
    const afterCursor = inputText.slice(cursor);
    inputText = `${beforeTrigger}${text} ${afterCursor}`;
    setTimeout(() => {
      if (textareaEl) {
        const nextPos = triggerIdx + text.length + 1;
        textareaEl.selectionStart = nextPos;
        textareaEl.selectionEnd = nextPos;
        textareaEl.focus();
        adjustHeight();
      }
    }, 10);
  }
  activePalette = null;
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.key === 'Escape') {
    closeAllPopups();
    activePalette = null;
    return;
  }

  if (activePalette) {
    const count =
      activePalette === 'slash'
        ? filteredSlash.length
        : activePalette === 'at'
        ? filteredFiles.length
        : filteredSkills.length;

    if (event.key === 'ArrowDown' && count > 0) {
      event.preventDefault();
      paletteSelectedIndex = (paletteSelectedIndex + 1) % count;
      return;
    }
    if (event.key === 'ArrowUp' && count > 0) {
      event.preventDefault();
      paletteSelectedIndex = (paletteSelectedIndex - 1 + count) % count;
      return;
    }
    if (event.key === 'Enter' || event.key === 'Tab') {
      event.preventDefault();
      if (activePalette === 'slash' && filteredSlash[paletteSelectedIndex]) {
        insertCompletion(filteredSlash[paletteSelectedIndex].cmd, '/');
      } else if (activePalette === 'at' && filteredFiles[paletteSelectedIndex]) {
        insertCompletion(`@${filteredFiles[paletteSelectedIndex].path}`, '@');
      } else if (activePalette === 'dollar' && filteredSkills[paletteSelectedIndex]) {
        insertCompletion(`$${filteredSkills[paletteSelectedIndex].name}`, '$');
      }
      return;
    }
  }

  if (event.key === 'Enter' && !event.shiftKey) {
    event.preventDefault();
    handleSubmit();
  }
}

function handleSubmit() {
  activePalette = null;
  paletteFilter = '';
  keycapSpring.target = { scale: 1, y: 0 };
  const rawTrimmed = inputText.trim();
  if (/^\/btw(\s|$)/i.test(rawTrimmed)) {
    inputText = '';
    chatStore.askSideQuestion(rawTrimmed);
    return;
  }
  if (/^\/goal\s+(clear|stop)$/i.test(rawTrimmed)) {
    inputText = '';
    chatStore.clearGoal();
    return;
  }
  if (chatStore.isStreaming && /^\/goal(\s+status)?$/i.test(rawTrimmed)) {
    inputText = '';
    chatStore.refreshGoalState();
    return;
  }
  if (dynamicButtonState === 'stop') {
    chatStore.stopStreaming();
    return;
  }
  if (dynamicButtonState === 'continue') {
    chatStore.continueAssistantMessage();
    return;
  }
  if (dynamicButtonState === 'queue') {
    const qText = inputText.trim();
    inputText = '';
    chatStore.setDraftText(chatStore.activeChatId, '');
    chatStore.setDraftText(null, '');
    chatStore.enqueueMessage(qText);
    return;
  }
  if (inputText.trim() || attachments.length > 0) {
    const text = inputText.trim();
    const currentAttachments = [...attachments];
    inputText = '';
    attachments = [];
    chatStore.setDraftText(chatStore.activeChatId, '');
    chatStore.setDraftAttachments(chatStore.activeChatId, []);
    chatStore.setDraftText(null, '');
    chatStore.setDraftAttachments(null, []);
    chatStore.sendMessage(text, { attachments: currentAttachments });
  }
}

async function startVoice() {
  showPlusMenu = false;
  try {
    const vc = new VoiceCapture();
    await vc.start();
    voiceCaptureInstance = vc;
    isVoiceActive = true;

    const updateWave = () => {
      if (!isVoiceActive || !voiceCaptureInstance) return;
      voiceBands = voiceCaptureInstance.getBands();
      voiceRafId = requestAnimationFrame(updateWave);
    };
    voiceRafId = requestAnimationFrame(updateWave);

    const startTime = Date.now();
    voiceTimerInterval = window.setInterval(() => {
      const elapsed = Math.floor((Date.now() - startTime) / 1000);
      const mins = String(Math.floor(elapsed / 60)).padStart(2, '0');
      const secs = String(elapsed % 60).padStart(2, '0');
      voiceTimerStr = `${mins}:${secs}`;
    }, 500);
  } catch (err: any) {
    alert(`Microphone access error: ${err?.message || 'Access denied'}`);
    cancelVoice();
  }
}

function cancelVoice() {
  if (voiceRafId) cancelAnimationFrame(voiceRafId);
  if (voiceTimerInterval) clearInterval(voiceTimerInterval);
  if (voiceCaptureInstance) {
    voiceCaptureInstance.cancel();
    voiceCaptureInstance = null;
  }
  isVoiceActive = false;
  voiceTimerStr = '00:00';
  voiceBands = new Array(28).fill(0.1);
}

async function activeModelNativeAudio(): Promise<boolean> {
  try {
    return await invoke<boolean>('model_native_audio', { modelId: displayActiveModel });
  } catch {
    return false;
  }
}

async function stopAndTranscribe() {
  if (!voiceCaptureInstance) return;
  try {
    const result = await voiceCaptureInstance.stop();
    cancelVoice();
    if (await activeModelNativeAudio()) {
      attachments = [
        ...attachments,
        {
          id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
          name: `voice_${Date.now()}.wav`,
          path: '',
          kind: 'audio',
          previewUrl: `data:audio/wav;base64,${result.base64}`
        }
      ];
      return;
    }
    try {
      const transcribed = await invoke<string>('transcribe_audio', {
        audioBase64: result.base64,
        model: settingsStore.data.whisperModel
      });
      if (transcribed && transcribed.trim()) {
        inputText = (inputText ? `${inputText} ` : '') + transcribed.trim();
        adjustHeight();
      }
    } catch {
      inputText = (inputText ? `${inputText} ` : '') + '[Transcribed audio]';
      adjustHeight();
    }
  } catch {
    cancelVoice();
  }
}

async function stopAndSendVoice() {
  if (!voiceCaptureInstance) return;
  try {
    const result = await voiceCaptureInstance.stop();
    cancelVoice();
    if (await activeModelNativeAudio()) {
      await chatStore.sendMessage('', {
        attachments: [
          {
            id: `att-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
            name: `voice_${Date.now()}.wav`,
            path: '',
            kind: 'audio',
            previewUrl: `data:audio/wav;base64,${result.base64}`
          }
        ]
      });
      return;
    }
    let textToSend = '[Voice Command]';
    try {
      const transcribed = await invoke<string>('transcribe_audio', {
        audioBase64: result.base64,
        model: settingsStore.data.whisperModel
      });
      if (transcribed && transcribed.trim()) {
        textToSend = transcribed.trim();
      }
    } catch {}
    await chatStore.sendMessage(textToSend);
  } catch {
    cancelVoice();
  }
}
</script>

<svelte:window onpointerdown={handleWindowPointerDown} />

<div bind:this={composerRootEl} data-omni-composer="" class="relative w-full max-w-[680px] mx-auto select-none">
  {#if chatStore.activeGoal && chatStore.activeGoal.condition}
    {@const g = chatStore.activeGoal}
    <div class="mb-2 px-3 py-2 rounded-[8px] bg-[#121214]/95 border border-white/[0.08] flex items-center justify-between gap-3">
      <div class="flex items-center gap-2 min-w-0">
        <span class="font-mono text-xs {g.status === 'active' ? 'text-[#A855F7] animate-pulse' : g.status === 'achieved' ? 'text-[#22C55E]' : g.status === 'failed' ? 'text-[#EF4444]' : 'text-[#F59E0B]'}">◎</span>
        <span class="font-mono text-[11px] font-medium text-[#EDEDEF] shrink-0">/goal {g.status}</span>
        <span class="text-xs text-[#A3A3AA] truncate" title={g.condition}>{g.condition}</span>
      </div>
      <div class="flex items-center gap-2.5 shrink-0">
        <span class="font-mono text-[10.5px] text-[#73737A]">
          turn {g.turn}/{g.maxTurns} · {g.elapsedSecs}s
        </span>
        <button
          type="button"
          class="px-2 py-0.5 rounded-[4px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] font-mono text-[10.5px] text-[#A3A3AA] hover:text-[#EDEDEF] transition-colors"
          onclick={() => chatStore.clearGoal()}
        >
          Clear
        </button>
      </div>
    </div>
  {/if}


  {#if chatStore.pendingPlan}
    {@const plan = chatStore.pendingPlan}
    <div
      transition:fly={{ y: 6, duration: 150 }}
      class="mb-2 rounded-[10px] bg-[#121214]/95 border border-white/[0.08] overflow-hidden shadow-xl"
    >
      <div class="px-3.5 py-2 border-b border-white/[0.06] flex items-center justify-between gap-2 bg-[#161619]">
        <button
          type="button"
          class="flex items-center gap-2 min-w-0 text-left"
          onclick={() => (planExpanded = !planExpanded)}
        >
          <span class="w-1.5 h-1.5 rounded-full bg-[#38BDF8] shrink-0"></span>
          <span class="text-xs font-medium text-[#EDEDEF]">Plan ready for review</span>
          <Icon name="chevron-down" class="w-3.5 h-3.5 text-[#73737A] transition-transform {planExpanded ? 'rotate-180' : ''}" strokeWidth={1.8} />
        </button>
        <div class="flex items-center gap-1.5 shrink-0">
          <button
            type="button"
            class="h-6 px-2.5 rounded-[5px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-[11px] font-medium transition-colors"
            onclick={() => {
              planFeedbackOpen = false;
              chatStore.approvePlan();
            }}
          >
            Approve &amp; Run
          </button>
          <button
            type="button"
            class="h-6 px-2.5 rounded-[5px] bg-[#1E1E22] hover:bg-[#27272A] border border-white/[0.08] text-[11px] text-[#D4D4D8] transition-colors"
            onclick={() => (planFeedbackOpen = !planFeedbackOpen)}
          >
            Request Changes
          </button>
          <button
            type="button"
            class="h-6 px-2 rounded-[5px] text-[11px] text-[#73737A] hover:text-[#F87171] hover:bg-white/[0.05] transition-colors"
            onclick={() => {
              planFeedbackOpen = false;
              chatStore.rejectPlan();
            }}
          >
            Reject
          </button>
        </div>
      </div>

      {#if planExpanded && plan.summary}
        <div class="markdown-prose px-4 py-3 max-h-[42vh] overflow-y-auto text-[13px] text-[#E5E5E8] leading-relaxed select-text">
          {@html renderMarkdown(plan.summary)}
        </div>
      {/if}

      {#if planFeedbackOpen}
        <div class="px-3 py-2 border-t border-white/[0.06] flex items-center gap-2 bg-[#141417]">
          <input
            bind:value={planFeedbackText}
            type="text"
            placeholder="Describe what to change in the plan..."
            class="flex-1 px-2.5 py-1.5 rounded-[6px] bg-[#1C1C1F] border border-white/[0.08] text-xs text-[#EDEDEF] placeholder-[#66666D] outline-none focus:border-white/[0.2]"
            onkeydown={(e) => {
              if (e.key === 'Enter' && planFeedbackText.trim()) {
                const fb = planFeedbackText.trim();
                planFeedbackText = '';
                planFeedbackOpen = false;
                chatStore.requestPlanChanges(fb);
              }
            }}
          />
          <button
            type="button"
            class="h-7 px-3 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium transition-colors"
            onclick={() => {
              if (!planFeedbackText.trim()) return;
              const fb = planFeedbackText.trim();
              planFeedbackText = '';
              planFeedbackOpen = false;
              chatStore.requestPlanChanges(fb);
            }}
          >
            Send
          </button>
        </div>
      {/if}
    </div>
  {/if}

  {#if chatStore.sideQuestion && chatStore.sideQuestion.visible && chatStore.sideQuestion.chatId === chatStore.activeChatId}
    {@const sq = chatStore.sideQuestion}
    <div
      transition:slide={{ duration: 180, axis: 'y' }}
      class="mb-2 rounded-[10px] bg-[#121214]/95 border border-white/[0.08] overflow-hidden shadow-xl"
    >
      <div
        role="separator"
        aria-orientation="horizontal"
        title="Drag to resize"
        onpointerdown={startSideQuestionResize}
        class="h-1.5 w-full cursor-ns-resize transition-colors {isResizingSideQuestion ? 'bg-[#8B5CF6]/50' : 'hover:bg-[#8B5CF6]/35'}"
      ></div>
      <div class="px-3.5 py-2 border-b border-white/[0.06] flex items-center justify-between gap-2 bg-[#161619]">
        <div class="flex items-center gap-2 min-w-0">
          <svg class="w-3.5 h-3.5 text-[#A3A3AA] shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">
            <rect x="8" y="8" width="12" height="12" rx="2" />
            <path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2" />
            <path d="M12.5 12.5a1.5 1.5 0 0 1 3 0c0 1-1.5 1.3-1.5 2.2" />
            <circle cx="14" cy="17.2" r="0.4" fill="currentColor" />
          </svg>
          <span class="text-xs font-medium text-[#EDEDEF] truncate">Side Question: {sq.question}</span>
        </div>
        <div class="flex items-center gap-0.5 shrink-0">
          <button
            type="button"
            class="p-1 rounded-[4px] text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
            onclick={() => { if (sq.answer) navigator.clipboard.writeText(sq.answer); }}
            title="Copy answer"
          >
            <Icon name="copy" class="w-3.5 h-3.5" strokeWidth={1.6} />
          </button>
          <button
            type="button"
            class="p-1 rounded-[4px] text-[#73737A] hover:text-[#F87171] hover:bg-white/[0.06] transition-colors"
            onclick={() => chatStore.clearSideQuestion()}
            title="Clear side question"
          >
            <Icon name="trash" class="w-3.5 h-3.5" strokeWidth={1.6} />
          </button>
          <button
            type="button"
            class="p-1 rounded-[4px] text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
            onclick={() => chatStore.hideSideQuestion()}
            title="Dismiss"
          >
            <Icon name="close" class="w-3.5 h-3.5" strokeWidth={1.8} />
          </button>
        </div>
      </div>
      <div class="px-3.5 py-2.5 overflow-y-auto text-[12.5px] leading-relaxed select-text" style="max-height: {sideQuestionMaxHeight}px;">
        {#if sq.error}
          <div class="text-[#F87171] font-mono text-xs">{sq.error}</div>
        {:else if sq.streaming && !sq.answer}
          <div class="text-[#73737A]">Thinking...</div>
        {:else}
          <div class="markdown-prose text-[#D4D4D8]">{@html renderMarkdown(sq.answer)}</div>
        {/if}
      </div>
    </div>
  {/if}

  {#if chatStore.pendingQueue.length > 0}
    <div class="mx-2 mb-1.5 space-y-1 max-h-28 overflow-y-auto">
      {#each chatStore.pendingQueue as item (item.id)}
        <div class="h-8 px-3 rounded-[8px] bg-[#18181B]/95 border border-white/[0.07] flex items-center justify-between gap-2 text-xs">
          <div class="relative flex-1 min-w-0 overflow-hidden">
            <div class="text-[12px] text-[#D4D4D8] whitespace-nowrap overflow-hidden">{item.text}</div>
            <div class="pointer-events-none absolute inset-y-0 right-0 w-8 bg-gradient-to-l from-[#18181B] to-transparent"></div>
          </div>
          <div class="flex items-center gap-0.5 shrink-0">
            <button
              type="button"
              class="p-1 rounded-[4px] text-[#73737A] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
              onclick={() => editQueuedItem(item.id)}
              title="Edit"
              use:attachPressScale
            >
              <Icon name="edit" class="w-3 h-3" strokeWidth={1.6} />
            </button>
            <button
              type="button"
              class="p-1 rounded-[4px] text-[#73737A] hover:text-[#EF4444] hover:bg-white/[0.06] transition-colors"
              onclick={() => chatStore.dequeueMessage(item.id)}
              title="Delete"
              use:attachPressScale
            >
              <Icon name="trash" class="w-3 h-3" strokeWidth={1.6} />
            </button>
            <button
              type="button"
              class="p-1 rounded-[4px] text-[#A3A3AA] hover:text-[#111113] hover:bg-[#EDEDEF] transition-colors ml-0.5"
              onclick={() => chatStore.sendNow(item.id)}
              title="Interrupt & send now"
              use:attachPressScale
            >
              <Icon name="enter" class="w-3 h-3" strokeWidth={1.8} />
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  {#if activePalette}
    <div
      class="absolute bottom-full mb-2 left-0 right-0 bg-[#18181B] border border-white/[0.08] rounded-[6px] overflow-hidden z-40 max-h-56 overflow-y-auto"
      style="opacity: {paletteSpring.current.opacity}; transform: translateY({paletteSpring.current.y}px);"
    >
      {#if activePalette === 'slash'}
        <div class="p-1.5 divide-y divide-white/[0.04]">
          {#each filteredSlash as item, idx}
            <button
              type="button"
              class="w-full px-3 py-2 text-left rounded-[4px] flex items-center justify-between text-xs transition-colors {idx === paletteSelectedIndex ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#A3A3AA] hover:bg-white/[0.04]'}"
              onclick={() => insertCompletion(item.cmd, '/')}
            >
              <span class="font-mono font-semibold text-[#EDEDEF]">{item.cmd}</span>
              <span class="text-[11px] text-[#66666D]">{item.desc}</span>
            </button>
          {/each}
          {#if filteredSlash.length === 0}
            <div class="p-3 text-center text-[#66666D] text-xs">No matching commands</div>
          {/if}
        </div>
      {:else if activePalette === 'at'}
        <div class="p-1.5 divide-y divide-white/[0.04]">
          {#each filteredFiles as item, idx}
            <button
              type="button"
              class="w-full px-3 py-2 text-left rounded-[4px] flex items-center justify-between text-xs transition-colors {idx === paletteSelectedIndex ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#A3A3AA] hover:bg-white/[0.04]'}"
              onclick={() => insertCompletion(`@${item.path}`, '@')}
            >
              <span class="font-mono text-[#E5E5E8] truncate">{item.name}</span>
              <span class="text-[10.5px] font-mono text-[#66666D] truncate max-w-[220px]">{item.path}</span>
            </button>
          {/each}
          {#if filteredFiles.length === 0}
            <div class="p-3 text-center text-[#66666D] text-xs">No matching files</div>
          {/if}
        </div>
      {:else if activePalette === 'dollar'}
        <div class="p-1.5 divide-y divide-white/[0.04]">
          {#each filteredSkills as item, idx}
            <button
              type="button"
              class="w-full px-3 py-2 text-left rounded-[4px] flex items-center justify-between text-xs transition-colors {idx === paletteSelectedIndex ? 'bg-white/[0.08] text-[#F5F5F7]' : 'text-[#A3A3AA] hover:bg-white/[0.04]'}"
              onclick={() => insertCompletion(`$${item.name}`, '$')}
            >
              <span class="font-mono text-[#E5E5E8]">${item.name}</span>
              <span class="text-[11px] text-[#66666D]">{item.desc}</span>
            </button>
          {/each}
        </div>
      {/if}
    </div>
  {/if}

  {#if isVoiceActive}
    <div class="rounded-[6px] border border-white/[0.07] bg-[#1C1C1F] p-4 flex flex-col gap-3">
      <div class="flex items-center justify-between text-xs">
        <div class="flex items-center gap-2">
          <div class="w-2 h-2 rounded-full bg-[#EF4444]"></div>
          <span class="text-[#E5E5E8]">Recording Voice</span>
        </div>
        <span class="font-mono text-[#A3A3AA]">{voiceTimerStr}</span>
      </div>

      <div class="h-9 w-full flex items-center justify-between gap-1 px-2.5 bg-[#121214] rounded-[4px] border border-white/[0.06]">
        {#each voiceBands as band}
          <div
            class="flex-1 bg-[#EDEDEF] rounded-[1px]"
            style="height: {Math.max(3, Math.round(band * 28))}px; opacity: {0.35 + band * 0.65};"
          ></div>
        {/each}
      </div>

      <div class="flex items-center justify-between">
        <button
          type="button"
          class="px-3 py-1.5 rounded-[4px] bg-white/[0.05] hover:bg-white/[0.09] text-xs text-[#A3A3AA] hover:text-[#E5E5E8]"
          onclick={cancelVoice}
          use:attachPressScale
        >
          Cancel
        </button>
        <div class="flex items-center gap-2">
          <button
            type="button"
            class="px-3 py-1.5 rounded-[4px] bg-white/[0.05] hover:bg-white/[0.09] text-xs text-[#E5E5E8]"
            onclick={stopAndTranscribe}
            use:attachPressScale
          >
            Transcribe
          </button>
          <button
            type="button"
            class="px-3 py-1.5 rounded-[4px] bg-[#EDEDEF] hover:bg-white text-xs text-[#111113] font-medium"
            onclick={stopAndSendVoice}
            use:attachPressScale
          >
            Send
          </button>
        </div>
      </div>
    </div>
  {:else}
    {#if attachments.length > 0}
      <div transition:slide={{ duration: 150 }} class="mb-2 flex flex-wrap gap-1.5">
        {#each attachments as att (att.id)}
          <div
            transition:scale={{ duration: 160, start: 0.9, opacity: 0, easing: cubicOut }}
            class="group/att flex items-center gap-2 pl-1.5 pr-2 py-1 rounded-[8px] bg-[#18181B] hover:bg-[#202024] border border-white/[0.08] hover:border-white/[0.16] text-xs max-w-[220px] transition-all duration-150"
          >
            {#if att.kind === 'image' && att.previewUrl}
              <img src={att.previewUrl} alt={att.name} class="w-7 h-7 rounded-[5px] object-cover shrink-0 bg-[#121214] border border-white/[0.06]" />
            {:else if att.kind === 'video' && att.previewUrl}
              <video src={att.previewUrl} class="w-7 h-7 rounded-[5px] object-cover shrink-0 bg-[#121214] border border-white/[0.06]" muted></video>
            {:else}
              <div class="w-7 h-7 rounded-[5px] bg-[#121214] border border-white/[0.06] flex items-center justify-center shrink-0">
                <FileToolIcon name={att.name} class="w-4 h-4" />
              </div>
            {/if}
            <span class="font-mono text-[11px] text-[#D4D4D8] truncate">{att.name}</span>
            <button
              type="button"
              class="w-4 h-4 rounded-full flex items-center justify-center text-[#73737A] hover:text-[#EF4444] hover:bg-white/[0.08] text-xs shrink-0 ml-0.5 transition-colors"
              onclick={() => removeAttachment(att.id)}
              title="Remove"
            >
              ×
            </button>
          </div>
        {/each}
      </div>
    {/if}

    <div
      data-omni-composer-box=""
      class="rounded-[10px] border p-3 transition-colors duration-150 {isDraggingOver ? 'border-white/[0.35] bg-[#222227]' : 'border-white/[0.07] bg-[#1C1C1F]/95'}"
      ondragover={(e) => { e.preventDefault(); isDraggingOver = true; }}
      ondragleave={() => (isDraggingOver = false)}
      ondrop={handleHtmlDrop}
      role="region"
    >
      <div class="relative w-full">
        {#if slashLeadMatch}
          <div
            bind:this={mirrorEl}
            aria-hidden="true"
            class="pointer-events-none absolute inset-0 px-1 font-sans text-[13.5px] leading-relaxed whitespace-pre-wrap break-words overflow-hidden"
          ><span class="text-[#D8B4FE] bg-[#8B5CF6]/15 rounded-[3px]">{slashLeadMatch[1]}</span><span class="text-[#E5E5E8]">{slashLeadMatch[2]}</span></div>
        {/if}
        <textarea
          bind:this={textareaEl}
          bind:value={inputText}
          oninput={handleInput}
          onkeydown={handleKeyDown}
          onpaste={handlePaste}
          onscroll={() => { if (mirrorEl && textareaEl) mirrorEl.scrollTop = textareaEl.scrollTop; }}
          data-omni-composer-input="" placeholder={themeStore.t('ask_placeholder', 'Ask OmniHarness anything, @ to add context, / for commands or capabilities')}
          rows="1"
          class="relative z-10 w-full px-1 bg-transparent font-sans text-[13.5px] {slashLeadMatch ? 'text-transparent caret-[#EDEDEF] selection:bg-white/20' : 'text-[#E5E5E8]'} placeholder-[#686870] outline-none focus:outline-none focus:ring-0 resize-none min-h-[24px] max-h-[180px] leading-relaxed transition-[height] duration-150 ease-out"
        ></textarea>
      </div>

      <div class="flex items-center justify-between pt-1.5 select-none gap-2">
        <div class="relative">
          <button
            type="button"
            class="w-7 h-7 -ml-0.5 rounded-[4px] flex items-center justify-center text-[#8E8E95] hover:text-[#E5E5E8] hover:bg-white/[0.06] transition-colors"
            onclick={() => {
              const next = !showPlusMenu;
              closeAllPopups();
              showPlusMenu = next;
            }}
            title="Context, Projects & Tools"
            use:attachPressScale
          >
            <Icon name="plus" class="w-4 h-4" strokeWidth={1.6} />
          </button>

          {#if showPlusMenu || plusMenuSpring.current.opacity > 0.02}
            <div
              class="absolute bottom-full mb-2 left-0 w-60 bg-[#18181B] border border-white/[0.08] rounded-[6px] p-1.5 z-50 text-xs space-y-1 {showPlusMenu ? 'pointer-events-auto' : 'pointer-events-none'}"
              style="opacity: {plusMenuSpring.current.opacity}; transform: translateY({plusMenuSpring.current.y}px) scale({plusMenuSpring.current.scale});"
            >
              <button
                type="button"
                class="w-full px-2.5 py-1.5 rounded-[4px] text-left text-[#C4C4CA] hover:bg-white/[0.06] hover:text-white flex items-center justify-between"
                onclick={() => {
                  inputText = (inputText ? `${inputText} ` : '') + '@';
                  closeAllPopups();
                  textareaEl?.focus();
                  checkPaletteTrigger();
                }}
              >
                <span>Add file context</span>
                <span class="font-mono text-[10.5px] text-[#66666D]">@</span>
              </button>
              <button
                type="button"
                class="w-full px-2.5 py-1.5 rounded-[4px] text-left text-[#C4C4CA] hover:bg-white/[0.06] hover:text-white flex items-center justify-between"
                onclick={() => {
                  inputText = (inputText ? `${inputText} ` : '') + '/';
                  closeAllPopups();
                  textareaEl?.focus();
                  checkPaletteTrigger();
                }}
              >
                <span>Slash commands</span>
                <span class="font-mono text-[10.5px] text-[#66666D]">/</span>
              </button>
              <button
                type="button"
                class="w-full px-2.5 py-1.5 rounded-[4px] text-left text-[#C4C4CA] hover:bg-white/[0.06] hover:text-white flex items-center justify-between"
                onclick={startVoice}
              >
                <span>Voice input</span>
                <span class="font-mono text-[10.5px] text-[#66666D]">MIC</span>
              </button>
              <button
                type="button"
                class="w-full px-2.5 py-1.5 rounded-[4px] text-left text-[#C4C4CA] hover:bg-white/[0.06] hover:text-white flex items-center justify-between"
                onclick={pickAttachments}
              >
                <span>Attach files</span>
                <span class="font-mono text-[10.5px] text-[#66666D]">FILE</span>
              </button>


            </div>
          {/if}
        </div>

        <div class="flex items-center gap-2">
          {#if !chatStore.activeSession?.isThemeStudio}
            <div class="relative">
              <button
                type="button"
                data-omni-project-btn=""
                class="h-6 px-2 rounded-[4px] inline-flex items-center gap-1.5 text-[11px] leading-none font-mono text-[#A3A3AA] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors max-w-[150px]"
                onclick={() => {
                  const next = !showProjectPopup;
                  closeAllPopups();
                  showProjectPopup = next;
                }}
                title={workspaceStore.currentPath || 'No project (Full PC)'}
                use:attachPressScale
              >
                <Icon name="folder" class="w-3.5 h-3.5 text-[#73737A] shrink-0 block -translate-y-[0.5px]" strokeWidth={1.6} />
                <span class="truncate leading-none translate-y-[1px]">{workspaceStore.workspaceName || 'Full PC'}</span>
                <Icon name="chevron-down" class="w-3 h-3 text-[#66666D] shrink-0 block translate-y-[0.5px] transition-transform duration-150 {showProjectPopup ? 'rotate-180' : ''}" strokeWidth={1.8} />
              </button>

              {#if showProjectPopup || projectMenuSpring.current.opacity > 0.02}
                <div
                  class="absolute bottom-full mb-2 right-0 w-64 bg-[#18181B] border border-white/[0.08] rounded-[6px] p-1.5 z-50 text-xs space-y-1 shadow-2xl {showProjectPopup ? 'pointer-events-auto' : 'pointer-events-none'}"
                  style="opacity: {projectMenuSpring.current.opacity}; transform: translateY({projectMenuSpring.current.y}px) scale({projectMenuSpring.current.scale});"
                >
                  <div class="px-2 py-0.5 font-mono text-[10px] text-[#66666D] uppercase tracking-wider">Workspace / Project</div>
                  <button
                    type="button"
                    class="w-full px-2.5 py-1.5 rounded-[4px] text-left font-mono text-[11px] flex items-center justify-between transition-colors {!workspaceStore.currentPath ? 'bg-white/[0.08] text-[#EDEDEF]' : 'text-[#A3A3AA] hover:bg-white/[0.05] hover:text-white'}"
                    onclick={() => {
                      workspaceStore.clearWorkspace();
                      chatStore.setActiveSessionProject('');
                      closeAllPopups();
                    }}
                  >
                    <span>No project (Full PC)</span>
                    {#if !workspaceStore.currentPath}
                      <Icon name="check" class="w-3 h-3 text-[#EDEDEF]" strokeWidth={2} />
                    {/if}
                  </button>

                  {#if workspaceStore.recentWorkspaces.length > 0}
                    <div class="max-h-40 overflow-y-auto space-y-0.5 border-t border-white/[0.06] pt-1">
                      {#each workspaceStore.recentWorkspaces as path}
                        <button
                          type="button"
                          class="w-full px-2.5 py-1.5 rounded-[4px] text-left font-mono text-[11px] flex items-center justify-between gap-2 transition-colors {workspaceStore.currentPath === path ? 'bg-white/[0.08] text-[#EDEDEF]' : 'text-[#A3A3AA] hover:bg-white/[0.05] hover:text-white'}"
                          onclick={() => {
                            workspaceStore.setWorkspaceDir(path);
                            chatStore.setActiveSessionProject(path);
                            closeAllPopups();
                          }}
                          title={path}
                        >
                          <span class="truncate">{path.split(/[\\/]/).pop() || path}</span>
                          {#if workspaceStore.currentPath === path}
                            <Icon name="check" class="w-3 h-3 text-[#EDEDEF] shrink-0" strokeWidth={2} />
                          {/if}
                        </button>
                      {/each}
                    </div>
                  {/if}

                  <button
                    type="button"
                    class="w-full px-2.5 py-1.5 border-t border-white/[0.06] pt-1.5 rounded-[4px] text-left text-xs text-[#D4D4D8] hover:bg-white/[0.05] flex items-center justify-between transition-colors"
                    onclick={async () => {
                      closeAllPopups();
                      const picked = await workspaceStore.openDirectoryPicker();
                      if (picked) chatStore.setActiveSessionProject(picked);
                    }}
                  >
                    <span>Open folder...</span>
                    <Icon name="chevron-right" class="w-3.5 h-3.5 text-[#73737A]" strokeWidth={1.8} />
                  </button>
                </div>
              {/if}
            </div>
          {/if}
          {#if chatStore.contextTelemetry.usedTokens > 0}
            {@const r = 6}
            {@const circ = 2 * Math.PI * r}
            {@const dashOffset = circ - (Math.max(4, contextPercent) / 100) * circ}
            <div class="relative group/ctx flex items-center justify-center">
              <div class="w-6 h-6 flex items-center justify-center rounded-[4px] hover:bg-white/[0.04] transition-colors">
                <svg class="w-4 h-4 -rotate-90" viewBox="0 0 16 16" fill="none">
                  <circle cx="8" cy="8" r={r} stroke="rgba(255,255,255,0.14)" stroke-width="1.8" />
                  <circle
                    cx="8"
                    cy="8"
                    r={r}
                    stroke="#EDEDEF"
                    stroke-width="1.8"
                    stroke-linecap="round"
                    stroke-dasharray={circ}
                    stroke-dashoffset={dashOffset}
                  />
                </svg>
              </div>
              <div class="pointer-events-none opacity-0 group-hover/ctx:opacity-100 transition-opacity duration-150 absolute bottom-full mb-2 right-0 px-2.5 py-1 rounded-[6px] bg-[#18181B] border border-white/[0.08] font-mono text-[10.5px] text-[#D4D4D8] whitespace-nowrap z-50">
                {contextTooltip}
              </div>
            </div>
          {/if}
          <div class="relative">
            <button
              type="button"
              class="px-2.5 py-1 rounded-[4px] flex items-center gap-1.5 text-[11.5px] font-mono text-[#A3A3AA] hover:text-[#E5E5E8] hover:bg-white/[0.05] transition-colors"
              onclick={() => {
                const next = !showModelPopup;
                closeAllPopups();
                showModelPopup = next;
                if (next) refreshOAuthTiers();
              }}
              data-omni-model-btn=""
              title="Model & Reasoning"
              use:attachPressScale
            >
              <span>{displayActiveModel || 'Select model'}</span>
              {#if supportedReasoningLevels.length > 0}
                <span class="text-[#73737A]">{REASONING_LABELS[settingsStore.data.reasoningLevel]}</span>
              {/if}
              <Icon name="chevron-down" class="w-3 h-3 text-[#66666D]" strokeWidth={1.8} />
            </button>

            {#if showModelPopup || modelMenuSpring.current.opacity > 0.02}
              {@const popupProv = settingsStore.data.providers[popupProviderId] || enabledProviders[0]}
              {@const popupTier = providerTierLabel(popupProv)}
              <div
                class="absolute bottom-full mb-2 right-0 w-72 bg-[#18181B] border border-white/[0.08] rounded-[6px] p-3 z-50 text-xs space-y-3 {showModelPopup ? 'pointer-events-auto' : 'pointer-events-none'}"
                style="opacity: {modelMenuSpring.current.opacity}; transform: translateY({modelMenuSpring.current.y}px) scale({modelMenuSpring.current.scale});"
              >
                <div class="space-y-2">
                  <div class="flex items-center justify-between gap-2 px-1">
                    <span class="font-mono text-[10px] text-[#66666D] uppercase tracking-wider">Model</span>
                    <div class="flex items-center gap-1.5 min-w-0">
                      {#if enabledProviders.length === 1}
                        <span class="font-mono text-[10px] text-[#73737A] truncate max-w-[110px]">{enabledProviders[0].name}</span>
                      {/if}
                      {#if popupTier}
                        <span class="px-1.5 py-0.5 rounded-[4px] border font-mono text-[9.5px] leading-none shrink-0 {tierBadgeClass(popupTier)}">{popupTier}</span>
                      {/if}
                    </div>
                  </div>

                  {#if enabledProviders.length > 1}
                    <div
                      class="h-7 bg-[#121214] rounded-[4px] border border-white/[0.06] p-0.5 flex items-center gap-0.5 overflow-x-auto [scrollbar-width:none] [-ms-overflow-style:none] [&::-webkit-scrollbar]:hidden"
                      onwheel={(e) => {
                        if (e.currentTarget.scrollWidth > e.currentTarget.clientWidth) {
                          e.preventDefault();
                          e.currentTarget.scrollLeft += e.deltaY !== 0 ? e.deltaY : e.deltaX;
                        }
                      }}
                    >
                      {#each enabledProviders as prov (prov.id)}
                        {@const provTier = providerTierLabel(prov)}
                        <button
                          type="button"
                          class="shrink-0 h-full px-2.5 rounded-[3px] font-mono text-[10px] whitespace-nowrap transition-colors {popupProviderId === prov.id ? 'bg-white/[0.12] border-t border-white/[0.18] text-[#EDEDEF] font-semibold' : 'text-[#73737A] hover:text-[#D4D4D8]'}"
                          onclick={() => {
                            localPopupProviderId = prov.id;
                            const defModel = prov.defaultModel || prov.availableModels[0] || '';
                            chatStore.setSessionModel(prov.id, defModel);
                          }}
                          title={provTier ? `${prov.name} · ${provTier}` : prov.name}
                        >
                          {prov.name}
                        </button>
                      {/each}
                    </div>
                  {/if}

                  {#if availableModels.length === 0}
                    <button
                      type="button"
                      class="w-full px-2.5 py-2 rounded-[4px] bg-white/[0.04] hover:bg-white/[0.08] text-left text-xs text-[#EDEDEF] flex items-center justify-between transition-colors"
                      onclick={() => {
                        closeAllPopups();
                        settingsStore.openSettings();
                      }}
                    >
                      <span>Configure provider...</span>
                      <Icon name="chevron-right" class="w-3.5 h-3.5 text-[#73737A]" strokeWidth={1.8} />
                    </button>
                  {:else}
                    {#key popupProviderId}
                      <div in:fly={{ y: 4, duration: 140 }} class="max-h-40 overflow-y-auto space-y-0.5">
                        {#each availableModels as item (item.model)}
                          {@const isSelected = currentProviderId === item.providerId && (displayActiveModel === item.model || item.rawIds.includes(currentModelId))}
                          <button
                            type="button"
                            class="w-full px-2.5 py-1.5 rounded-[4px] text-left font-mono text-[11px] flex items-center justify-between transition-colors {isSelected ? 'bg-white/[0.09] text-[#EDEDEF]' : 'text-[#A3A3AA] hover:bg-white/[0.04] hover:text-white'}"
                            onclick={() => {
                              const prev = displayActiveModel;
                              chatStore.setSessionModel(item.providerId, item.model);
                              if (prev && prev !== item.model) {
                                chatStore.recordModelChanged(item.model);
                              }
                              closeAllPopups();
                            }}
                          >
                            <span class="truncate">{item.model}</span>
                          </button>
                        {/each}
                      </div>
                    {/key}
                  {/if}
                </div>

                {#if supportedReasoningLevels.length > 0}
                  <div class="border-t border-white/[0.06] pt-2.5 space-y-2">
                    <div class="flex items-center justify-between px-0.5">
                      <span class="font-mono text-[10px] text-[#66666D] uppercase tracking-wider">Reasoning</span>
                      <span class="font-mono text-[10.5px] text-[#D4D4D8]">
                        {REASONING_LABELS[settingsStore.data.reasoningLevel]}
                      </span>
                    </div>

                    <div
                      data-omni-reasoning-slider=""
                      class="relative h-7 bg-[#121214] rounded-[4px] border border-white/[0.06] p-0.5 grid items-center"
                      style="grid-template-columns: repeat({supportedReasoningLevels.length}, minmax(0, 1fr));"
                    >
                      <div
                        class="absolute top-0.5 bottom-0.5 left-0.5 rounded-[3px] bg-white/[0.12] border-t border-white/[0.18] pointer-events-none"
                        style="width: calc((100% - 4px) / {supportedReasoningLevels.length}); transform: translateX(calc({sliderSpring.current} * 100%));"
                      ></div>
                      {#each supportedReasoningLevels as lvl}
                        <button
                          type="button"
                          class="relative z-10 h-full font-mono text-[10px] text-center transition-colors {settingsStore.data.reasoningLevel === lvl ? 'text-[#EDEDEF] font-semibold' : 'text-[#73737A] hover:text-[#D4D4D8]'}"
                          onclick={() => settingsStore.setReasoningLevel(lvl)}
                        >
                          {REASONING_LABELS[lvl]}
                        </button>
                      {/each}
                    </div>
                  </div>
                {/if}
              </div>
            {/if}
          </div>

          {#if hasWhisperModel}
            <button
              type="button"
              class="w-7 h-7 rounded-[4px] flex items-center justify-center text-[#8E8E95] hover:text-[#EDEDEF] hover:bg-white/[0.06] transition-colors"
              onclick={startVoice}
              title="Voice input (Whisper)"
              use:attachPressScale
            >
              <Icon name="mic" class="w-3.5 h-3.5" strokeWidth={1.7} />
            </button>
          {/if}

          {#if chatStore.isStreaming}
            <div class="flex items-center gap-1.5">
              {#if dynamicButtonState === 'queue'}
                <button
                  type="button"
                  data-omni-button="send"
                  class="w-7 h-7 rounded-[4px] bg-[#EDEDEF] hover:bg-white text-[#111113] border-b-2 border-[#A8A8B0] flex items-center justify-center transition-colors"
                  onclick={handleSubmit}
                  title="Queue message (Enter)"
                  use:attachPressScale
                >
                  <Icon name="enter" class="w-3.5 h-3.5" strokeWidth={2.1} />
                </button>
              {/if}
              <button
                type="button"
                class="w-7 h-7 rounded-[4px] bg-[#262629] hover:bg-[#323236] border border-white/[0.1] text-[#EF4444] flex items-center justify-center transition-colors"
                onclick={() => chatStore.stopStreaming()}
                title="Stop generation"
                use:attachPressScale
              >
                <div class="w-2.5 h-2.5 bg-[#EF4444] rounded-[1px]"></div>
              </button>
            </div>
          {:else}
            <button
              type="button"
              data-omni-button="send"
              class="w-7 h-7 rounded-[4px] flex items-center justify-center transition-colors {inputText.trim() || attachments.length > 0 || dynamicButtonState === 'continue' ? 'bg-[#EDEDEF] hover:bg-white text-[#111113] border-b-2 border-[#A8A8B0]' : 'bg-[#28282C] text-[#73737A] border-b-2 border-[#1B1B1E] cursor-not-allowed'}"
              style="transform: translateY({keycapSpring.current.y}px) scale({keycapSpring.current.scale});"
              onpointerdown={() => { if (inputText.trim() || attachments.length > 0 || dynamicButtonState === 'continue') keycapSpring.target = { scale: 0.94, y: 1.5 }; }}
              onpointerup={() => (keycapSpring.target = { scale: 1, y: 0 })}
              onpointerleave={() => (keycapSpring.target = { scale: 1, y: 0 })}
              disabled={!inputText.trim() && attachments.length === 0 && dynamicButtonState !== 'continue'}
              onclick={handleSubmit}
              title={dynamicButtonState === 'continue' ? 'Продолжить с паузы (Enter)' : 'Send (Enter)'}
            >
              <Icon name="enter" class="w-3.5 h-3.5" strokeWidth={2.1} />
            </button>
          {/if}
        </div>
      </div>
    </div>
  {/if}
</div>

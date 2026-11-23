<script lang="ts">
import { onMount } from 'svelte';
import { invoke } from '@tauri-apps/api/core';
import { settingsStore } from '../../lib/stores/settings.svelte';
import { whisperStore, type WhisperModelInfo } from '../../lib/stores/whisper.svelte';
import { VoiceCapture } from '../../lib/voiceCapture';
import Icon from '../Icon.svelte';

let selectedModelId = $state<string>(settingsStore.data.whisperModel?.replace(/^ggml-/, '') || 'large-v3-turbo');
let selectedLanguage = $state<string>(settingsStore.data.whisperLanguage || 'ru');

let isRecording = $state<boolean>(false);
let recordingSeconds = $state<number>(0);
let isTranscribing = $state<boolean>(false);
let transcriptionResult = $state<string>('');
let transcriptionError = $state<string | null>(null);

let voiceCapture = $state<VoiceCapture | null>(null);
let audioBands = $state<number[]>(new Array(24).fill(0.08));
let animFrameId: number | null = null;
let recordTimerId: ReturnType<typeof setInterval> | null = null;

onMount(() => {
  whisperStore.refresh();
  return () => {
    cleanupRecording();
  };
});

function cleanupRecording() {
  if (animFrameId) {
    cancelAnimationFrame(animFrameId);
    animFrameId = null;
  }
  if (recordTimerId) {
    clearInterval(recordTimerId);
    recordTimerId = null;
  }
  if (voiceCapture) {
    voiceCapture.cancel();
    voiceCapture = null;
  }
  isRecording = false;
  audioBands = new Array(24).fill(0.08);
}

function selectModel(id: string) {
  selectedModelId = id;
  settingsStore.data.whisperModel = `ggml-${id}`;
  settingsStore.saveSettings();
}

function updateLanguage(lang: string) {
  selectedLanguage = lang;
  settingsStore.data.whisperLanguage = lang;
  settingsStore.saveSettings();
}

async function startRecording() {
  cleanupRecording();
  transcriptionError = null;
  transcriptionResult = '';

  try {
    const vc = new VoiceCapture();
    await vc.start();
    voiceCapture = vc;
    isRecording = true;
    recordingSeconds = 0;

    recordTimerId = setInterval(() => {
      recordingSeconds++;
    }, 1000);

    const loop = () => {
      if (!isRecording || !voiceCapture) return;
      const raw = voiceCapture.getBands();
      audioBands = raw.slice(0, 24);
      animFrameId = requestAnimationFrame(loop);
    };
    animFrameId = requestAnimationFrame(loop);
  } catch (err: any) {
    cleanupRecording();
    transcriptionError = `Не удалось получить доступ к микрофону: ${err?.message || err}`;
  }
}

async function stopRecording() {
  if (!voiceCapture) {
    cleanupRecording();
    return;
  }

  isTranscribing = true;
  try {
    const res = await voiceCapture.stop();
    cleanupRecording();

    const selectedModel = whisperStore.models.find((m) => m.id === selectedModelId);
    if (!selectedModel || selectedModel.status !== 'installed') {
      transcriptionError = `Модель Whisper "${selectedModel?.name || selectedModelId}" не скачана. Скачайте её выше для распознавания речи.`;
      isTranscribing = false;
      return;
    }

    const t0 = performance.now();
    const text = await invoke<string>('transcribe_audio', {
      audioBase64: res.base64,
      model: `ggml-${selectedModelId}`,
      language: selectedLanguage
    });
    const dt = Math.round(performance.now() - t0);

    if (text && text.trim()) {
      transcriptionResult = text.trim();
    } else {
      transcriptionResult = '[Голос не распознан / тишина]';
    }
  } catch (err: any) {
    transcriptionError = `Ошибка транскрибации: ${err?.message || err}`;
  } finally {
    isTranscribing = false;
    cleanupRecording();
  }
}

function formatBytes(bytes?: number): string {
  if (!bytes || bytes <= 0) return '0 MB';
  const mb = bytes / (1024 * 1024);
  if (mb < 1000) return `${mb.toFixed(0)} MB`;
  return `${(mb / 1024).toFixed(2)} GB`;
}
</script>

<div class="space-y-5 text-[13px] text-[#E5E5E8]">
  <div>
    <h2 class="text-sm font-medium text-[#EDEDEF]">Локальные веса Whisper</h2>
    <p class="text-xs text-[#73737A] mt-0.5">Выберите модель для офлайн-распознавания голосовых сообщений и аудиоввода.</p>
  </div>

  <div class="flex items-center justify-between p-3.5 rounded-[8px] bg-[#1C1C1F] border border-white/[0.06]">
    <div>
      <div class="text-xs font-medium text-[#EDEDEF]">Язык распознавания речи</div>
      <div class="text-[11px] text-[#73737A] mt-0.5">Принудительный русский отключает ложные переводы и галлюцинации на английский</div>
    </div>
    <select
      value={selectedLanguage}
      onchange={(e) => updateLanguage((e.target as HTMLSelectElement).value)}
      class="bg-[#121214] border border-white/[0.1] rounded-[6px] px-3 py-1.5 text-xs text-[#EDEDEF] focus:outline-none focus:border-[#A855F7] cursor-pointer"
    >
      <option value="ru">Русский (ru) — фиксированный</option>
      <option value="auto">Автоопределение (auto)</option>
      <option value="en">English (en)</option>
    </select>
  </div>

  {#if whisperStore.statusError}
    <div class="px-3 py-2 rounded-[6px] bg-[#EF4444]/10 border border-[#EF4444]/30 text-xs text-[#FCA5A5]">
      {whisperStore.statusError}
    </div>
  {/if}

  <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
    {#each whisperStore.models as model (model.id)}
      <div
        class="p-4 rounded-[8px] bg-[#1C1C1F] border flex flex-col justify-between h-full transition-colors duration-200 cursor-pointer {whisperStore.justInstalledId === model.id ? 'border-[#22C55E]/60' : selectedModelId === model.id ? 'border-white/[0.32]' : 'border-white/[0.06] hover:border-white/[0.14]'}"
        onclick={() => selectModel(model.id)}
        role="button"
        tabindex="0"
        onkeydown={(e) => { if (e.key === 'Enter') selectModel(model.id); }}
      >
        <div class="space-y-2">
          <div class="flex items-start justify-between gap-2">
            <span class="font-medium text-[#EDEDEF] text-xs">{model.name}</span>
            <span class="px-1.5 py-0.5 rounded-[4px] font-mono text-[10px] shrink-0 {model.status === 'installed' ? 'bg-[#22C55E]/10 text-[#4ADE80]' : model.status === 'downloading' ? 'bg-[#8B5CF6]/15 text-[#DDD6FE]' : 'bg-[#121214] text-[#73737A]'}">
              {model.status === 'installed' ? 'УСТАНОВЛЕНА' : model.status === 'downloading' ? 'СКАЧИВАНИЕ' : 'НЕ СКАЧАНА'}
            </span>
          </div>
          <div class="font-mono text-[11px] text-[#73737A] flex items-center gap-3">
            <span>{model.sizeFormatted}</span>
            <span>VRAM {model.vramRequired}</span>
          </div>
        </div>

        <div class="mt-4 pt-3 border-t border-white/[0.05]" onclick={(e) => e.stopPropagation()} role="presentation">
          {#if model.status === 'downloading'}
            <div class="space-y-1.5">
              <div class="w-full h-8 rounded-[6px] bg-[#121214] border border-white/[0.08] relative overflow-hidden flex items-center justify-between px-3">
                <div
                  class="absolute inset-y-0 left-0 bg-[#8B5CF6]/30 border-r border-[#8B5CF6]/60 transition-all duration-200"
                  style="width: {Math.min(100, Math.max(2, model.downloadProgress ?? 0))}%;"
                ></div>
                <span class="relative z-10 font-mono text-[11px] font-medium text-[#EDEDEF]">
                  {Math.min(100, model.downloadProgress ?? 0)}%
                </span>
                <span class="relative z-10 font-mono text-[10.5px] text-[#A3A3AA]">
                  {model.downloadSpeed || 'Загрузка...'}
                </span>
              </div>
              <div class="flex items-center justify-between px-0.5 font-mono text-[10px] text-[#71717A]">
                <span>{formatBytes(model.downloadedBytes)} / {model.sizeFormatted}</span>
                <span>Фоновая загрузка активна</span>
              </div>
            </div>
          {:else if model.status === 'installed'}
            <button
              type="button"
              class="w-full h-8 rounded-[6px] bg-[#121214] hover:bg-[#242428] border border-white/[0.07] text-xs text-[#A3A3AA] hover:text-[#EF4444] transition-colors"
              onclick={() => whisperStore.delete(model.id)}
            >
              Удалить веса
            </button>
          {:else}
            <button
              type="button"
              class="w-full h-8 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium transition-colors"
              onclick={() => whisperStore.download(model.id)}
            >
              Скачать модель
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>

  <div class="p-4 rounded-[8px] bg-[#1C1C1F] border border-white/[0.06] space-y-3">
    <div class="flex items-center justify-between">
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">Тестовый стенд микрофона</div>
        <div class="text-[11px] text-[#73737A]">Проверка микрофонного тракта и качества распознавания выбранной модели</div>
      </div>
      <button
        type="button"
        class="h-8 px-3.5 rounded-[6px] text-xs font-medium whitespace-nowrap transition-colors {isRecording ? 'bg-[#EF4444]/20 text-[#F87171] border border-[#EF4444]/40 hover:bg-[#EF4444]/30' : isTranscribing ? 'bg-white/[0.08] text-[#A3A3AA] cursor-wait' : 'bg-[#EDEDEF] hover:bg-white text-[#111113]'}"
        disabled={isTranscribing}
        onclick={isRecording ? stopRecording : startRecording}
      >
        {#if isTranscribing}
          Распознавание...
        {:else if isRecording}
          Стоп ({recordingSeconds}с)
        {:else}
          Тест микрофона
        {/if}
      </button>
    </div>

    <!-- Live Audio Equalizer -->
    <div class="h-10 px-3 rounded-[6px] bg-[#121214] border border-white/[0.06] flex items-center justify-between gap-1">
      {#each audioBands as level, i}
        <div
          class="flex-1 rounded-[1px] transition-all duration-75 {isRecording ? 'bg-[#8B5CF6]/80' : 'bg-white/[0.12]'}"
          style="height: {Math.max(3, Math.round(level * 32))}px;"
        ></div>
      {/each}
    </div>

    {#if transcriptionResult}
      <div class="p-3 rounded-[6px] bg-[#141416] border border-white/[0.06] space-y-1">
        <div class="text-[10.5px] font-mono text-[#71717A] uppercase tracking-wider">Результат распознавания:</div>
        <div class="text-[12.5px] text-[#EDEDEF] select-text font-sans leading-relaxed">
          «{transcriptionResult}»
        </div>
      </div>
    {/if}

    {#if transcriptionError}
      <div class="p-2.5 rounded-[6px] bg-[#EF4444]/10 border border-[#EF4444]/30 text-xs text-[#FCA5A5]">
        {transcriptionError}
      </div>
    {/if}
  </div>
</div>
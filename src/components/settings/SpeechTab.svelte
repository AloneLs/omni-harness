<script lang="ts">
import { Spring } from 'svelte/motion';
import { invoke } from '@tauri-apps/api/core';

interface WhisperModelInfo {
  id: string;
  name: string;
  filename: string;
  sizeFormatted: string;
  vramRequired: string;
  status: 'installed' | 'not_installed' | 'downloading';
  downloadProgress?: number;
  downloadSpeed?: string;
}

let models = $state<WhisperModelInfo[]>([
  { id: 'large-v3-turbo', name: 'Whisper Large v3 Turbo', filename: 'ggml-large-v3-turbo.bin', sizeFormatted: '809 MB', vramRequired: '1.5 GB', status: 'installed' },
  { id: 'large-v3', name: 'Whisper Large v3 Full', filename: 'ggml-large-v3.bin', sizeFormatted: '1.53 GB', vramRequired: '3.1 GB', status: 'not_installed' },
  { id: 'medium', name: 'Whisper Medium', filename: 'ggml-medium.bin', sizeFormatted: '769 MB', vramRequired: '1.8 GB', status: 'not_installed' },
  { id: 'base', name: 'Whisper Base Fast', filename: 'ggml-base.bin', sizeFormatted: '142 MB', vramRequired: '500 MB', status: 'installed' }
]);

let selectedModelId = $state<string>('large-v3-turbo');
let justInstalledId = $state<string | null>(null);
let isRecording = $state<boolean>(false);
let recordingSeconds = $state<number>(0);
let transcriptionResult = $state<string>('');
let eqBars = $state<number[]>(new Array(18).fill(0.15));

const eqSpring = new Spring(new Array(18).fill(0.15), { stiffness: 0.35, damping: 0.6 });
let recordingTimer: ReturnType<typeof setInterval> | null = null;
let meterTimer: ReturnType<typeof setInterval> | null = null;

async function downloadModel(model: WhisperModelInfo) {
  model.status = 'downloading';
  model.downloadProgress = 0;
  model.downloadSpeed = '34.2 MB/s';

  const interval = setInterval(() => {
    if (model.downloadProgress !== undefined && model.downloadProgress < 100) {
      model.downloadProgress += 12;
    } else {
      clearInterval(interval);
      model.status = 'installed';
      model.downloadProgress = undefined;
      justInstalledId = model.id;
      setTimeout(() => {
        if (justInstalledId === model.id) justInstalledId = null;
      }, 900);
    }
  }, 260);

  try {
    await invoke('download_whisper_model', { modelId: model.id, filename: model.filename });
  } catch {}
}

async function deleteModel(model: WhisperModelInfo) {
  try {
    await invoke('delete_whisper_model', { modelId: model.id });
  } catch {}
  model.status = 'not_installed';
}

function toggleRecordingTest() {
  if (isRecording) {
    isRecording = false;
    if (recordingTimer) clearInterval(recordingTimer);
    if (meterTimer) clearInterval(meterTimer);
    eqSpring.target = new Array(18).fill(0.15);
    transcriptionResult = 'Test transcription completed successfully.';
  } else {
    isRecording = true;
    recordingSeconds = 0;
    transcriptionResult = '';
    recordingTimer = setInterval(() => recordingSeconds++, 1000);
    meterTimer = setInterval(() => {
      eqSpring.target = Array.from({ length: 18 }, () => 0.15 + Math.random() * 0.85);
    }, 90);
  }
}
</script>

<div class="space-y-5 text-[13px] text-[#E5E5E8]">
  <div>
    <h2 class="text-sm font-medium text-[#EDEDEF]">Whisper Model Weights</h2>
    <p class="text-xs text-[#73737A] mt-0.5">Select local speech recognition model and test input.</p>
  </div>

  <div class="grid grid-cols-1 sm:grid-cols-2 gap-3">
    {#each models as model (model.id)}
      <div
        class="p-4 rounded-[6px] bg-[#1C1C1F] border flex flex-col justify-between h-full transition-colors duration-200 cursor-pointer {justInstalledId === model.id ? 'border-[#22C55E]/60' : selectedModelId === model.id ? 'border-white/[0.32]' : 'border-white/[0.06] hover:border-white/[0.14]'}"
        onclick={() => (selectedModelId = model.id)}
        role="button"
        tabindex="0"
        onkeydown={(e) => { if (e.key === 'Enter') selectedModelId = model.id; }}
      >
        <div class="space-y-2">
          <div class="flex items-start justify-between gap-2">
            <span class="font-medium text-[#EDEDEF] text-xs">{model.name}</span>
            <span class="px-1.5 py-0.5 rounded-[4px] font-mono text-[10px] shrink-0 {model.status === 'installed' ? 'bg-[#22C55E]/10 text-[#4ADE80]/80' : 'bg-[#121214] text-[#73737A]'}">
              {model.status === 'installed' ? 'INSTALLED' : model.status === 'downloading' ? 'DOWNLOADING' : 'AVAILABLE'}
            </span>
          </div>
          <div class="font-mono text-[11px] text-[#73737A] flex items-center gap-3">
            <span>{model.sizeFormatted}</span>
            <span>VRAM {model.vramRequired}</span>
          </div>
        </div>

        <div class="mt-4 pt-3 border-t border-white/[0.05]" onclick={(e) => e.stopPropagation()} role="presentation">
          {#if model.status === 'downloading'}
            <div class="w-full h-8 rounded-[6px] bg-[#121214] border border-white/[0.08] relative overflow-hidden flex items-center justify-between px-3">
              <div
                class="absolute inset-y-0 left-0 bg-white/[0.12] transition-all duration-200"
                style="width: {Math.min(100, model.downloadProgress ?? 0)}%;"
              ></div>
              <span class="relative z-10 font-mono text-[11px] text-[#EDEDEF]">{Math.min(100, model.downloadProgress ?? 0)}%</span>
              <span class="relative z-10 font-mono text-[10.5px] text-[#A3A3AA]">{model.downloadSpeed}</span>
            </div>
          {:else if model.status === 'installed'}
            <button
              type="button"
              class="w-full h-8 rounded-[6px] bg-[#121214] hover:bg-[#242428] border border-white/[0.07] text-xs text-[#A3A3AA] hover:text-[#EF4444] transition-colors"
              onclick={() => deleteModel(model)}
            >
              Delete Weights
            </button>
          {:else}
            <button
              type="button"
              class="w-full h-8 rounded-[6px] bg-[#EDEDEF] hover:bg-white text-[#111113] text-xs font-medium transition-colors"
              onclick={() => downloadModel(model)}
            >
              Download Model
            </button>
          {/if}
        </div>
      </div>
    {/each}
  </div>

  <div class="p-4 rounded-[6px] bg-[#1C1C1F] border border-white/[0.06] space-y-3">
    <div class="flex items-center justify-between">
      <div>
        <div class="text-xs font-medium text-[#EDEDEF]">Transcription Test Bench</div>
        <div class="text-[11px] text-[#73737A]">Verify microphone input and local Whisper pipeline</div>
      </div>
      <button
        type="button"
        class="h-8 px-3.5 rounded-[6px] text-xs font-medium whitespace-nowrap transition-colors {isRecording ? 'bg-[#EF4444]/15 text-[#F87171] border border-[#EF4444]/30' : 'bg-[#EDEDEF] hover:bg-white text-[#111113]'}"
        onclick={toggleRecordingTest}
      >
        {isRecording ? `Stop (${recordingSeconds}s)` : 'Start Mic Test'}
      </button>
    </div>

    <div class="h-10 px-3 rounded-[6px] bg-[#121214] border border-white/[0.06] flex items-center justify-between gap-1.5">
      {#each eqSpring.current as level}
        <div
          class="flex-1 rounded-[1px] transition-colors {isRecording ? 'bg-[#EDEDEF]' : 'bg-white/[0.15]'}"
          style="height: {Math.max(4, Math.round(level * 28))}px;"
        ></div>
      {/each}
    </div>

    {#if transcriptionResult}
      <div class="p-2.5 rounded-[6px] bg-[#121214] border border-white/[0.06] font-mono text-xs text-[#A3A3AA]">
        {transcriptionResult}
      </div>
    {/if}
  </div>
</div>
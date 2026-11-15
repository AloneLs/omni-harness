export interface VoiceCaptureResult {
  blob: Blob;
  base64: string;
  durationMs: number;
}

export class VoiceCapture {
  private audioContext: AudioContext | null = null;
  private mediaStream: MediaStream | null = null;
  private analyser: AnalyserNode | null = null;
  private sourceNode: MediaStreamAudioSourceNode | null = null;
  private scriptProcessor: ScriptProcessorNode | null = null;
  private recordedChunks: Float32Array[] = [];
  private totalSampleCount = 0;
  private startTime = 0;
  private recording = false;
  private smoothedBands: number[] = new Array(28).fill(0);
  private sampleRate = 16000;

  public async start(): Promise<void> {
    if (this.recording) {
      return;
    }

    this.recordedChunks = [];
    this.totalSampleCount = 0;
    this.smoothedBands = new Array(28).fill(0);

    const stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        channelCount: 1,
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true
      }
    });

    this.mediaStream = stream;
    const AudioContextClass = window.AudioContext || (window as any).webkitAudioContext;
    this.audioContext = new AudioContextClass({ sampleRate: 16000 });
    this.sampleRate = this.audioContext.sampleRate;

    this.analyser = this.audioContext.createAnalyser();
    this.analyser.fftSize = 512;
    this.analyser.smoothingTimeConstant = 0.3;

    this.sourceNode = this.audioContext.createMediaStreamSource(stream);
    this.scriptProcessor = this.audioContext.createScriptProcessor(4096, 1, 1);

    this.scriptProcessor.onaudioprocess = (event: AudioProcessingEvent) => {
      if (!this.recording) {
        return;
      }
      const inputBuffer = event.inputBuffer.getChannelData(0);
      const chunk = new Float32Array(inputBuffer.length);
      chunk.set(inputBuffer);
      this.recordedChunks.push(chunk);
      this.totalSampleCount += chunk.length;
    };

    this.sourceNode.connect(this.analyser);
    this.analyser.connect(this.scriptProcessor);
    this.scriptProcessor.connect(this.audioContext.destination);

    this.recording = true;
    this.startTime = Date.now();
  }

  public getBands(): number[] {
    if (!this.recording || !this.analyser) {
      return this.smoothedBands.map((v) => Math.max(0, v * 0.76));
    }

    const freqData = new Uint8Array(this.analyser.frequencyBinCount);
    this.analyser.getByteFrequencyData(freqData);

    const binCount = freqData.length;
    const bandCount = 28;

    for (let i = 0; i < bandCount; i++) {
      const startFreqRatio = Math.pow(i / bandCount, 1.8);
      const endFreqRatio = Math.pow((i + 1) / bandCount, 1.8);

      const startBin = Math.min(binCount - 1, Math.floor(startFreqRatio * binCount));
      const endBin = Math.min(binCount, Math.max(startBin + 1, Math.floor(endFreqRatio * binCount)));

      let sum = 0;
      let count = 0;
      for (let b = startBin; b < endBin; b++) {
        sum += freqData[b];
        count++;
      }

      const avg = count > 0 ? sum / count : 0;
      const target = Math.min(1.0, avg / 220);
      const last = this.smoothedBands[i];
      this.smoothedBands[i] = last * 0.76 + target * 0.24;
    }

    return [...this.smoothedBands];
  }

  public async stop(): Promise<VoiceCaptureResult> {
    if (!this.recording) {
      throw new Error('Not recording');
    }

    const durationMs = Date.now() - this.startTime;
    this.recording = false;

    if (this.mediaStream) {
      for (const track of this.mediaStream.getTracks()) {
        track.stop();
      }
      this.mediaStream = null;
    }

    if (this.scriptProcessor) {
      this.scriptProcessor.disconnect();
      this.scriptProcessor = null;
    }

    if (this.sourceNode) {
      this.sourceNode.disconnect();
      this.sourceNode = null;
    }

    if (this.analyser) {
      this.analyser.disconnect();
      this.analyser = null;
    }

    if (this.audioContext && this.audioContext.state !== 'closed') {
      await this.audioContext.close();
      this.audioContext = null;
    }

    const wavBlob = this.encodeWav(this.recordedChunks, this.sampleRate);
    const base64 = await this.blobToBase64(wavBlob);

    this.recordedChunks = [];
    this.totalSampleCount = 0;

    return {
      blob: wavBlob,
      base64,
      durationMs
    };
  }

  public cancel(): void {
    this.recording = false;
    if (this.mediaStream) {
      for (const track of this.mediaStream.getTracks()) {
        track.stop();
      }
      this.mediaStream = null;
    }
    if (this.scriptProcessor) {
      this.scriptProcessor.disconnect();
      this.scriptProcessor = null;
    }
    if (this.sourceNode) {
      this.sourceNode.disconnect();
      this.sourceNode = null;
    }
    if (this.analyser) {
      this.analyser.disconnect();
      this.analyser = null;
    }
    if (this.audioContext && this.audioContext.state !== 'closed') {
      this.audioContext.close().catch(() => {});
      this.audioContext = null;
    }
    this.recordedChunks = [];
    this.totalSampleCount = 0;
  }

  public isRecordingActive(): boolean {
    return this.recording;
  }

  public getElapsedMs(): number {
    return this.recording ? Date.now() - this.startTime : 0;
  }

  private encodeWav(chunks: Float32Array[], sampleRate: number): Blob {
    let totalLength = 0;
    for (const chunk of chunks) {
      totalLength += chunk.length;
    }

    const merged = new Float32Array(totalLength);
    let offset = 0;
    for (const chunk of chunks) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }

    const buffer = new ArrayBuffer(44 + merged.length * 2);
    const view = new DataView(buffer);

    const writeString = (offset: number, string: string) => {
      for (let i = 0; i < string.length; i++) {
        view.setUint8(offset + i, string.charCodeAt(i));
      }
    };

    writeString(0, 'RIFF');
    view.setUint32(4, 36 + merged.length * 2, true);
    writeString(8, 'WAVE');
    writeString(12, 'fmt ');
    view.setUint32(16, 16, true);
    view.setUint16(20, 1, true);
    view.setUint16(22, 1, true);
    view.setUint32(24, sampleRate, true);
    view.setUint32(28, sampleRate * 2, true);
    view.setUint16(32, 2, true);
    view.setUint16(34, 16, true);
    writeString(36, 'data');
    view.setUint32(40, merged.length * 2, true);

    let dataOffset = 44;
    for (let i = 0; i < merged.length; i++) {
      const s = Math.max(-1, Math.min(1, merged[i]));
      const val = s < 0 ? s * 0x8000 : s * 0x7fff;
      view.setInt16(dataOffset, val, true);
      dataOffset += 2;
    }

    return new Blob([view], { type: 'audio/wav' });
  }

  private blobToBase64(blob: Blob): Promise<string> {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onloadend = () => {
        const result = reader.result as string;
        const base64Data = result.split(',')[1] || result;
        resolve(base64Data);
      };
      reader.onerror = reject;
      reader.readAsDataURL(blob);
    });
  }
}

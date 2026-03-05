type RecorderAudioContext = AudioContext;

type RecorderWindow = Window &
  typeof globalThis & {
  webkitAudioContext?: typeof AudioContext;
  MediaRecorder?: typeof MediaRecorder;
};

type LegacyGetUserMedia = (
  constraints: MediaStreamConstraints,
  success: (stream: MediaStream) => void,
  failure: (error: Error) => void
) => void;

type RecorderNavigator = Navigator & {
  getUserMedia?: LegacyGetUserMedia;
  webkitGetUserMedia?: LegacyGetUserMedia;
  mozGetUserMedia?: LegacyGetUserMedia;
  msGetUserMedia?: LegacyGetUserMedia;
};

export type AudioRecordingResult = {
  blob: Blob;
  durationMs: number;
  sampleRate: number;
};

export type AudioRecordingSession = {
  stop: () => Promise<AudioRecordingResult>;
  cancel: () => Promise<void>;
};

const DEFAULT_SAMPLE_RATE = 44100;
const SCRIPT_PROCESSOR_BUFFER_SIZE = 4096;

const resolveAudioContextCtor = (): typeof AudioContext | null => {
  if (typeof window === 'undefined') return null;
  const host = window as RecorderWindow;
  if (typeof host.AudioContext === 'function') return host.AudioContext;
  if (typeof host.webkitAudioContext === 'function') return host.webkitAudioContext;
  return null;
};

const resolveLegacyGetUserMedia = (): LegacyGetUserMedia | null => {
  if (typeof navigator === 'undefined') return null;
  const source = navigator as RecorderNavigator;
  return (
    source.getUserMedia ||
    source.webkitGetUserMedia ||
    source.mozGetUserMedia ||
    source.msGetUserMedia ||
    null
  );
};

const resolveMediaRecorderCtor = (): typeof MediaRecorder | null => {
  if (typeof window === 'undefined') return null;
  const host = window as RecorderWindow;
  if (typeof host.MediaRecorder === 'function') return host.MediaRecorder;
  return null;
};

const resolveMediaRecorderMimeType = (ctor: typeof MediaRecorder): string => {
  if (typeof ctor.isTypeSupported !== 'function') {
    return '';
  }
  const candidates = [
    'audio/webm;codecs=opus',
    'audio/webm',
    'audio/ogg;codecs=opus',
    'audio/ogg'
  ];
  for (const candidate of candidates) {
    if (ctor.isTypeSupported(candidate)) {
      return candidate;
    }
  }
  return '';
};

const clampPcmSample = (value: number): number => {
  if (Number.isNaN(value)) return 0;
  return Math.max(-1, Math.min(1, value));
};

const mergeFloatChunks = (chunks: Float32Array[], totalLength: number): Float32Array => {
  const output = new Float32Array(totalLength);
  let offset = 0;
  chunks.forEach((chunk) => {
    output.set(chunk, offset);
    offset += chunk.length;
  });
  return output;
};

const encodePcmToWav = (pcmSamples: Float32Array, sampleRate: number): Blob => {
  const sampleCount = pcmSamples.length;
  const bytesPerSample = 2;
  const dataLength = sampleCount * bytesPerSample;
  const buffer = new ArrayBuffer(44 + dataLength);
  const view = new DataView(buffer);
  let offset = 0;
  const writeText = (value: string) => {
    for (let index = 0; index < value.length; index += 1) {
      view.setUint8(offset + index, value.charCodeAt(index));
    }
    offset += value.length;
  };
  const writeUint32 = (value: number) => {
    view.setUint32(offset, value, true);
    offset += 4;
  };
  const writeUint16 = (value: number) => {
    view.setUint16(offset, value, true);
    offset += 2;
  };

  writeText('RIFF');
  writeUint32(36 + dataLength);
  writeText('WAVE');
  writeText('fmt ');
  writeUint32(16);
  writeUint16(1);
  writeUint16(1);
  writeUint32(sampleRate);
  writeUint32(sampleRate * bytesPerSample);
  writeUint16(bytesPerSample);
  writeUint16(16);
  writeText('data');
  writeUint32(dataLength);

  for (let index = 0; index < sampleCount; index += 1) {
    const normalized = clampPcmSample(pcmSamples[index]);
    const intSample = normalized < 0 ? normalized * 0x8000 : normalized * 0x7fff;
    view.setInt16(offset, Math.round(intSample), true);
    offset += 2;
  }

  return new Blob([buffer], { type: 'audio/wav' });
};

const stopMediaTracks = (stream: MediaStream) => {
  stream.getTracks().forEach((track) => {
    try {
      track.stop();
    } catch {
      // ignore stop errors
    }
  });
};

export const isAudioRecordingSupported = (): boolean => {
  if (typeof navigator === 'undefined') return false;
  const hasMediaDevices = Boolean(
    navigator.mediaDevices && typeof navigator.mediaDevices.getUserMedia === 'function'
  );
  const hasLegacyGetUserMedia = Boolean(resolveLegacyGetUserMedia());
  if (!hasMediaDevices && !hasLegacyGetUserMedia) {
    return false;
  }
  return Boolean(resolveAudioContextCtor() || resolveMediaRecorderCtor());
};

export const startAudioRecording = async (): Promise<AudioRecordingSession> => {
  const AudioContextCtor = resolveAudioContextCtor();
  const MediaRecorderCtor = resolveMediaRecorderCtor();
  const mediaDevices = navigator.mediaDevices;
  const modernGetUserMedia = mediaDevices?.getUserMedia?.bind(mediaDevices);
  const legacyGetUserMedia = resolveLegacyGetUserMedia();
  if ((!AudioContextCtor && !MediaRecorderCtor) || (!modernGetUserMedia && !legacyGetUserMedia)) {
    throw new Error('audio recording is not supported');
  }
  const requestStream = async () => {
    if (modernGetUserMedia) {
      return modernGetUserMedia({ audio: true });
    }
    return new Promise<MediaStream>((resolve, reject) => {
      if (!legacyGetUserMedia) {
        reject(new Error('audio recording is not supported'));
        return;
      }
      legacyGetUserMedia.call(navigator, { audio: true }, resolve, reject);
    });
  };
  const stream = await requestStream();
  let context: RecorderAudioContext | null = null;
  let sourceNode: MediaStreamAudioSourceNode | null = null;
  let processorNode: ScriptProcessorNode | null = null;
  let silentNode: GainNode | null = null;
  const chunks: Float32Array[] = [];
  let totalSamples = 0;
  let sampleRate = DEFAULT_SAMPLE_RATE;
  let finalized = false;
  let stopPromise: Promise<AudioRecordingResult> | null = null;

  const cleanupGraph = async () => {
    if (processorNode) {
      processorNode.onaudioprocess = null;
      try {
        processorNode.disconnect();
      } catch {
        // ignore disconnect errors
      }
      processorNode = null;
    }
    if (sourceNode) {
      try {
        sourceNode.disconnect();
      } catch {
        // ignore disconnect errors
      }
      sourceNode = null;
    }
    if (silentNode) {
      try {
        silentNode.disconnect();
      } catch {
        // ignore disconnect errors
      }
      silentNode = null;
    }
    stopMediaTracks(stream);
    if (context) {
      try {
        await context.close();
      } catch {
        // ignore close errors
      }
      context = null;
    }
  };

  const canUseWebAudio = Boolean(AudioContextCtor);
  if (canUseWebAudio) {
    try {
      context = new AudioContextCtor();
      if (typeof context.createScriptProcessor !== 'function') {
        throw new Error('audio recording is not supported');
      }
      sampleRate = Number(context.sampleRate) || DEFAULT_SAMPLE_RATE;
      sourceNode = context.createMediaStreamSource(stream);
      processorNode = context.createScriptProcessor(SCRIPT_PROCESSOR_BUFFER_SIZE, 1, 1);
      silentNode = context.createGain();
      silentNode.gain.value = 0;
      processorNode.onaudioprocess = (event: AudioProcessingEvent) => {
        if (finalized) return;
        const source = event.inputBuffer.getChannelData(0);
        if (!source || !source.length) return;
        const snapshot = new Float32Array(source.length);
        snapshot.set(source);
        chunks.push(snapshot);
        totalSamples += snapshot.length;
      };
      sourceNode.connect(processorNode);
      processorNode.connect(silentNode);
      silentNode.connect(context.destination);
      if (context.state === 'suspended') {
        await context.resume();
      }
    } catch (error) {
      await cleanupGraph();
      if (!MediaRecorderCtor) {
        throw error;
      }
      const fallbackStream = await requestStream();
      return createMediaRecorderSession(fallbackStream, MediaRecorderCtor);
    }
  } else if (MediaRecorderCtor) {
    return createMediaRecorderSession(stream, MediaRecorderCtor);
  }

  const finalizeRecording = async (discard: boolean): Promise<AudioRecordingResult> => {
    if (finalized) {
      if (stopPromise) return stopPromise;
      throw new Error('recording has already finished');
    }
    finalized = true;
    await cleanupGraph();
    if (discard) {
      return {
        blob: new Blob([], { type: 'audio/wav' }),
        durationMs: 0,
        sampleRate
      };
    }
    if (!totalSamples) {
      throw new Error('recorded audio is empty');
    }
    const mergedSamples = mergeFloatChunks(chunks, totalSamples);
    const blob = encodePcmToWav(mergedSamples, sampleRate);
    const durationMs = Math.max(1, Math.round((totalSamples / sampleRate) * 1000));
    return { blob, durationMs, sampleRate };
  };

  return {
    stop: () => {
      if (!stopPromise) {
        stopPromise = finalizeRecording(false);
      }
      return stopPromise;
    },
    cancel: async () => {
      if (stopPromise) {
        await stopPromise.catch(() => undefined);
        return;
      }
      await finalizeRecording(true).catch(() => undefined);
    }
  };
};

const createMediaRecorderSession = (
  stream: MediaStream,
  ctor: typeof MediaRecorder
): AudioRecordingSession => {
  let recorder: MediaRecorder | null = null;
  let finalized = false;
  let stopPromise: Promise<AudioRecordingResult> | null = null;
  const chunks: BlobPart[] = [];
  const startedAt = Date.now();
  const mimeType = resolveMediaRecorderMimeType(ctor);
  const options = mimeType ? { mimeType } : undefined;

  const cleanup = () => {
    if (recorder) {
      recorder.ondataavailable = null;
      recorder.onstop = null;
      recorder.onerror = null;
      recorder = null;
    }
    stopMediaTracks(stream);
  };

  recorder = new ctor(stream, options);
  recorder.ondataavailable = (event: BlobEvent) => {
    if (event.data && event.data.size > 0) {
      chunks.push(event.data);
    }
  };
  try {
    recorder.start();
  } catch (error) {
    cleanup();
    throw error;
  }

  const finalize = (discard: boolean): Promise<AudioRecordingResult> => {
    if (finalized) {
      if (stopPromise) return stopPromise;
      return Promise.reject(new Error('recording has already finished'));
    }
    finalized = true;
    stopPromise = new Promise<AudioRecordingResult>((resolve, reject) => {
      let settled = false;
      let fallbackTimer: number | null = null;
      const settleResolve = (result: AudioRecordingResult) => {
        if (settled) return;
        settled = true;
        if (fallbackTimer !== null) {
          window.clearTimeout(fallbackTimer);
          fallbackTimer = null;
        }
        resolve(result);
      };
      const settleReject = (error: Error) => {
        if (settled) return;
        settled = true;
        if (fallbackTimer !== null) {
          window.clearTimeout(fallbackTimer);
          fallbackTimer = null;
        }
        reject(error);
      };
      const handleStop = () => {
        cleanup();
        if (discard) {
          settleResolve({ blob: new Blob([], { type: mimeType || 'audio/webm' }), durationMs: 0, sampleRate: 0 });
          return;
        }
        const blob = new Blob(chunks, { type: mimeType || chunks[0]?.type || 'audio/webm' });
        if (!blob.size) {
          settleReject(new Error('recorded audio is empty'));
          return;
        }
        const durationMs = Math.max(1, Date.now() - startedAt);
        settleResolve({ blob, durationMs, sampleRate: 0 });
      };
      const handleError = (event: Event) => {
        cleanup();
        const error = (event as { error?: Error }).error;
        settleReject(error || new Error('recording failed'));
      };
      recorder?.addEventListener('stop', handleStop, { once: true });
      recorder?.addEventListener('error', handleError, { once: true });
      fallbackTimer = window.setTimeout(() => {
        cleanup();
        settleReject(new Error('recording failed'));
      }, 2000);
      if (recorder && recorder.state !== 'inactive') {
        try {
          recorder.stop();
        } catch (error) {
          cleanup();
          settleReject((error as Error) || new Error('recording failed'));
        }
      } else {
        handleStop();
      }
    });
    return stopPromise;
  };

  return {
    stop: () => finalize(false),
    cancel: async () => {
      await finalize(true).catch(() => undefined);
    }
  };
};

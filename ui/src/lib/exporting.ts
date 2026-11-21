export type Format = 'mp4' | 'webm' | 'gif' | 'mov';
export type VideoCodec = 'h264' | 'hevc' | 'av1' | 'vp9' | 'vp8' | 'prores' | 'gif';
export type AudioCodec = 'aac' | 'opus' | 'flac' | 'alac' | 'vorbis' | 'pcm' | 'none';
export type Quality =
  | { tier: 'small' | 'balanced' | 'high' }
  | { tier: 'custom'; crf: number | null; kbps: number | null };
export type SizeChoice =
  | { kind: 'original' }
  | { kind: 'class' | 'width' | 'height' | 'long'; value: number };
export type Loop = { kind: 'forever' | 'once' } | { kind: 'times'; times: number };

export interface ExportSettings {
  format: Format;
  video: VideoCodec;
  audio: AudioCodec;
  encoder: 'best' | 'fast';
  quality: Quality;
  fps: number;
  size: SizeChoice;
  upscale: boolean;
  gif: { mode: 'gifski' | 'palette'; quality: number; motion_quality: number; lossy: number; colors: number; dither: boolean; repeat: Loop };
  out_dir: string | null;
  name: string;
}

export interface ExportOptions {
  store: {
    last_format: Format | null;
    per_format: Record<string, ExportSettings>;
    presets: { name: string; settings: ExportSettings }[];
  };
  available: { video: Record<string, { best: boolean; fast: boolean }>; gifski: boolean };
  recording_fps: number;
  reference: { w: number; h: number };
  ratio: [number, number];
  default_dir: string | null;
}

export interface ExportEstimate {
  canvas: { w: number; h: number };
  fps: number;
  frames: number;
  duration_ns: number;
  file: string;
  bytes_low: number;
  bytes_high: number;
  seconds: number;
  audio: boolean;
  less_compatible: string | null;
  too_large_for_fast: boolean;
}

export const VIDEO_FPS = [15, 24, 25, 30, 50, 60];
export const GIF_FPS = [5, 8, 10, 12, 15, 20, 25, 30, 50];

export const FORMAT_ALLOWED: Record<Format, { video: VideoCodec[]; audio: AudioCodec[] }> = {
  mp4: { video: ['h264', 'hevc', 'av1'], audio: ['aac', 'opus', 'flac', 'alac', 'none'] },
  webm: { video: ['vp9', 'av1', 'vp8'], audio: ['opus', 'vorbis', 'none'] },
  gif: { video: ['gif'], audio: ['none'] },
  mov: { video: ['prores', 'h264', 'hevc'], audio: ['pcm', 'aac', 'none'] },
};

export const VIDEO_LABELS: Record<VideoCodec, string> = {
  h264: 'H.264',
  hevc: 'HEVC (H.265)',
  av1: 'AV1',
  vp9: 'VP9',
  vp8: 'VP8 (legacy)',
  prores: 'ProRes 422',
  gif: 'GIF',
};

export const AUDIO_LABELS: Record<AudioCodec, string> = {
  aac: 'AAC',
  opus: 'Opus',
  flac: 'FLAC (lossless)',
  alac: 'Apple Lossless',
  vorbis: 'Vorbis',
  pcm: 'PCM (uncompressed)',
  none: 'No audio',
};

export function forFormat(format: Format): ExportSettings {
  const base = {
    encoder: 'best' as const,
    quality: { tier: 'balanced' as const },
    upscale: false,
    gif: { mode: 'gifski' as const, quality: 80, motion_quality: 80, lossy: 100, colors: 256, dither: true, repeat: { kind: 'forever' as const } },
    out_dir: null,
    name: '{title}',
  };
  switch (format) {
    case 'mp4':
      return { ...base, format, video: 'h264', audio: 'aac', fps: 30, size: { kind: 'original' } };
    case 'webm':
      return { ...base, format, video: 'vp9', audio: 'opus', fps: 30, size: { kind: 'original' } };
    case 'gif':
      return { ...base, format, video: 'gif', audio: 'none', fps: 15, size: { kind: 'long', value: 960 } };
    case 'mov':
      return { ...base, format, video: 'prores', audio: 'pcm', fps: 30, size: { kind: 'original' } };
  }
}

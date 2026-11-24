export type Format = 'mp4' | 'webm' | 'gif' | 'mov' | 'webp' | 'apng';
export type VideoCodec = 'h264' | 'hevc' | 'av1' | 'vp9' | 'vp8' | 'prores' | 'gif' | 'webp' | 'apng';
export type AudioCodec = 'aac' | 'opus' | 'flac' | 'alac' | 'vorbis' | 'pcm' | 'none';
export type Quality =
  | { tier: 'small' | 'balanced' | 'high' }
  | { tier: 'custom'; crf: number | null; kbps: number | null }
  | { tier: 'target_size'; mb: number };
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
  audio_kbps?: number | null;
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
  webp: { video: ['webp'], audio: ['none'] },
  apng: { video: ['apng'], audio: ['none'] },
};

/** GIF, animated WebP and APNG: no sound, image frame rates, a loop setting. */
export function isAnimation(f: Format): boolean {
  return f === 'gif' || f === 'webp' || f === 'apng';
}

export const VIDEO_LABELS: Record<VideoCodec, string> = {
  h264: 'H.264',
  hevc: 'HEVC (H.265)',
  av1: 'AV1',
  vp9: 'VP9',
  vp8: 'VP8 (legacy)',
  prores: 'ProRes 422',
  gif: 'GIF',
  webp: 'WebP',
  apng: 'APNG',
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
    audio_kbps: null,
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
    case 'webp':
    case 'apng':
      return { ...base, format, video: format, audio: 'none', fps: 15, size: { kind: 'long', value: 960 } };
  }
}

export type FrameShape = 'original' | '16:9' | '4:3' | '1:1' | '4:5' | '9:16';

export interface Destination {
  id: string;
  label: string;
  hint: string;
  /** The frame the destination wants; the sheet offers to switch to it. */
  frame?: FrameShape;
  settings: () => ExportSettings;
}

export const DESTINATIONS: Destination[] = [
  {
    id: 'email',
    label: 'Email',
    hint: 'MP4 under 18 MB, so it attaches in Gmail and Outlook (their 25 MB limit counts encoding overhead)',
    settings: () => ({ ...forFormat('mp4'), quality: { tier: 'target_size', mb: 18 }, size: { kind: 'class', value: 1080 }, fps: 30 }),
  },
  {
    id: 'slack',
    label: 'Slack & Teams',
    hint: 'MP4 at 1080p, compact, plays inline in chat',
    settings: () => ({ ...forFormat('mp4'), quality: { tier: 'small' }, size: { kind: 'class', value: 1080 }, fps: 30 }),
  },
  {
    id: 'youtube',
    label: 'YouTube',
    hint: 'MP4 at full size (up to 4K), high quality, your recording\'s frame rate',
    settings: () => ({ ...forFormat('mp4'), quality: { tier: 'high' }, size: { kind: 'original' }, fps: 60, audio_kbps: 256 }),
  },
  {
    id: 'vertical',
    label: 'TikTok, Reels & Shorts',
    hint: 'Vertical 1080 × 1920 MP4, high quality',
    frame: '9:16',
    settings: () => ({ ...forFormat('mp4'), quality: { tier: 'high' }, size: { kind: 'long', value: 1920 }, fps: 30, audio_kbps: 192 }),
  },
  {
    id: 'gif',
    label: 'GIF',
    hint: 'For docs and chat: 15 fps, 960 wide, loops forever',
    settings: () => forFormat('gif'),
  },
];

/** The destination these settings match, if any (format, quality and size alike). */
export function destinationOf(st: ExportSettings): string | null {
  const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
  const d = DESTINATIONS.find((x) => {
    const t = x.settings();
    return t.format === st.format && same(t.quality, st.quality) && same(t.size, st.size) && t.video === st.video;
  });
  return d?.id ?? null;
}

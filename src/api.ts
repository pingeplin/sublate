import { Channel, invoke } from "@tauri-apps/api/core";

export type TrackKind = "manual" | "auto";

export interface SubtitleTrack {
  code: string;
  name: string;
  kind: TrackKind;
}

export interface VideoMetadata {
  id: string;
  title: string;
  thumbnail: string | null;
  duration: number | null;
  url: string;
  subtitles: SubtitleTrack[];
}

export interface Language {
  code: string;
  name: string;
  native: string;
}

export interface SubtitleOutput {
  sourcePath: string;
  translatedPath: string;
  cueCount: number;
}

export type JobEvent =
  | { event: "videoProgress"; data: { percent: number } }
  | { event: "subtitleDownloaded"; data: { path: string } }
  | { event: "translationProgress"; data: { done: number; total: number } };

function videoRef({ url, id, title }: VideoMetadata) {
  return { url, id, title };
}

function channel(onEvent: (e: JobEvent) => void): Channel<JobEvent> {
  const ch = new Channel<JobEvent>();
  ch.onmessage = onEvent;
  return ch;
}

export const api = {
  targetLanguages: () => invoke<Language[]>("target_languages"),
  credentialSource: () => invoke<string>("credential_source"),
  fetchMetadata: (url: string) => invoke<VideoMetadata>("fetch_metadata", { url }),
  downloadVideo: (video: VideoMetadata, outDir: string, onEvent: (e: JobEvent) => void) =>
    invoke<string>("download_video", { video: videoRef(video), outDir, onEvent: channel(onEvent) }),
  translateSubtitles: (
    video: VideoMetadata,
    track: SubtitleTrack,
    target: string,
    outDir: string,
    onEvent: (e: JobEvent) => void,
  ) =>
    invoke<SubtitleOutput>("translate_subtitles", {
      video: videoRef(video),
      outDir,
      track,
      target,
      onEvent: channel(onEvent),
    }),
};

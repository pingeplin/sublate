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
  | { event: "translationProgress"; data: { done: number; total: number } };

function videoRef({ url, id, title }: VideoMetadata) {
  return { url, id, title };
}

export const api = {
  targetLanguages: () => invoke<Language[]>("target_languages"),
  credentialSource: () => invoke<string>("credential_source"),
  fetchMetadata: (url: string) => invoke<VideoMetadata>("fetch_metadata", { url }),
  downloadVideo: (video: VideoMetadata, outDir: string, onEvent: (e: JobEvent) => void) =>
    invoke<string>("download_video", {
      video: videoRef(video),
      outDir,
      onEvent: new Channel<JobEvent>(onEvent),
    }),
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
      onEvent: new Channel<JobEvent>(onEvent),
    }),
};

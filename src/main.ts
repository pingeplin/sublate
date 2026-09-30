import { downloadDir, join } from "@tauri-apps/api/path";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { api, type JobEvent, type Language, type SubtitleTrack, type VideoMetadata } from "./api";

const DEFAULT_TARGET = "zh-TW";
const OUTPUT_FOLDER = "contents-title";

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;

const ui = {
  form: $<HTMLFormElement>("url-form"),
  url: $<HTMLInputElement>("url"),
  fetch: $<HTMLButtonElement>("fetch"),
  status: $<HTMLParagraphElement>("status"),
  auth: $<HTMLSpanElement>("auth-badge"),
  video: $<HTMLElement>("video"),
  thumb: $<HTMLImageElement>("thumb"),
  title: $<HTMLHeadingElement>("title"),
  duration: $<HTMLParagraphElement>("duration"),
  source: $<HTMLSelectElement>("source"),
  target: $<HTMLSelectElement>("target"),
  outDir: $<HTMLInputElement>("out-dir"),
  chooseDir: $<HTMLButtonElement>("choose-dir"),
  withVideo: $<HTMLInputElement>("with-video"),
  run: $<HTMLButtonElement>("run"),
  progress: $<HTMLDivElement>("progress"),
  videoStep: $<HTMLDivElement>("video-step"),
  videoBar: $<HTMLProgressElement>("video-bar"),
  subLabel: $<HTMLSpanElement>("sub-label"),
  subBar: $<HTMLProgressElement>("sub-bar"),
  results: $<HTMLUListElement>("results"),
};

let current: VideoMetadata | null = null;

function setStatus(message: string, isError = false) {
  ui.status.textContent = message;
  ui.status.classList.toggle("error", isError);
}

function formatDuration(seconds: number | null): string {
  if (seconds == null) return "";
  const s = Math.round(seconds);
  const parts = [Math.floor(s / 3600), Math.floor(s / 60) % 60, s % 60];
  const [h, m, sec] = parts.map((n) => String(n).padStart(2, "0"));
  return parts[0] > 0 ? `${h}:${m}:${sec}` : `${m}:${sec}`;
}

function option(value: string, label: string): HTMLOptionElement {
  const el = document.createElement("option");
  el.value = value;
  el.textContent = label;
  return el;
}

function renderTracks(tracks: SubtitleTrack[]) {
  ui.source.replaceChildren();
  if (tracks.length === 0) {
    ui.source.append(option("", "No subtitles available"));
    ui.source.disabled = true;
    return;
  }
  ui.source.disabled = false;
  const groups: Record<SubtitleTrack["kind"], string> = { manual: "Uploaded", auto: "Auto-generated" };
  for (const kind of ["manual", "auto"] as const) {
    const items = tracks.filter((t) => t.kind === kind);
    if (items.length === 0) continue;
    const group = document.createElement("optgroup");
    group.label = groups[kind];
    items.forEach((t) => group.append(option(String(tracks.indexOf(t)), `${t.name} (${t.code})`)));
    ui.source.append(group);
  }
}

function renderTargets(languages: Language[]) {
  ui.target.replaceChildren(...languages.map((l) => option(l.code, `${l.native} (${l.code})`)));
  ui.target.value = DEFAULT_TARGET;
}

function renderVideo(meta: VideoMetadata) {
  ui.title.textContent = meta.title;
  ui.duration.textContent = formatDuration(meta.duration);
  ui.thumb.hidden = !meta.thumbnail;
  ui.thumb.src = meta.thumbnail ?? "";
  renderTracks(meta.subtitles);
  ui.results.replaceChildren();
  ui.progress.hidden = true;
  ui.video.hidden = false;
}

function addResult(label: string, path: string) {
  const li = document.createElement("li");
  const text = document.createElement("span");
  text.innerHTML = `<strong></strong><code></code>`;
  text.querySelector("strong")!.textContent = label;
  text.querySelector("code")!.textContent = path;
  const reveal = document.createElement("button");
  reveal.type = "button";
  reveal.className = "secondary";
  reveal.textContent = "Show in Finder";
  reveal.onclick = () => revealItemInDir(path);
  li.append(text, reveal);
  ui.results.append(li);
}

function onJobEvent(e: JobEvent) {
  switch (e.event) {
    case "videoProgress":
      ui.videoBar.value = e.data.percent;
      break;
    case "subtitleDownloaded":
      ui.subLabel.textContent = "Translating";
      break;
    case "translationProgress":
      ui.subBar.max = e.data.total;
      ui.subBar.value = e.data.done;
      ui.subLabel.textContent = `Translating ${e.data.done}/${e.data.total}`;
      break;
  }
}

function setBusy(busy: boolean) {
  [ui.fetch, ui.run, ui.chooseDir, ui.url].forEach((el) => (el.disabled = busy));
}

async function fetchMetadata(event: SubmitEvent) {
  event.preventDefault();
  setBusy(true);
  setStatus("Fetching metadata…");
  try {
    current = await api.fetchMetadata(ui.url.value);
    renderVideo(current);
    setStatus("");
  } catch (err) {
    ui.video.hidden = true;
    setStatus(String(err), true);
  } finally {
    setBusy(false);
  }
}

async function getContent() {
  if (!current) return;
  const video = current;
  const track = video.subtitles[Number(ui.source.value)];
  const outDir = ui.outDir.value;
  const target = ui.target.value;
  const withVideo = ui.withVideo.checked;

  ui.results.replaceChildren();
  ui.progress.hidden = false;
  ui.videoStep.hidden = !withVideo;
  ui.videoBar.value = 0;
  ui.subBar.value = 0;
  ui.subLabel.textContent = track ? "Downloading subtitles" : "No subtitles to translate";
  setBusy(true);
  setStatus("Working…");
  try {
    const jobs: Promise<void>[] = [];
    if (withVideo) {
      jobs.push(api.downloadVideo(video.url, outDir, onJobEvent).then((p) => addResult("Video", p)));
    }
    if (track) {
      jobs.push(
        api.translateSubtitles(video, track, target, outDir, onJobEvent).then((r) => {
          addResult(`Source subtitle (${track.code})`, r.sourcePath);
          addResult(`Translated subtitle (${target}, ${r.cueCount} cues)`, r.translatedPath);
        }),
      );
    }
    const failures = (await Promise.allSettled(jobs)).filter((r) => r.status === "rejected");
    if (failures.length > 0) {
      setStatus(failures.map((f) => String((f as PromiseRejectedResult).reason)).join("\n"), true);
    } else {
      setStatus("Done.");
    }
  } finally {
    setBusy(false);
  }
}

async function chooseDir() {
  const dir = await open({ directory: true, defaultPath: ui.outDir.value });
  if (typeof dir === "string") ui.outDir.value = dir;
}

async function init() {
  ui.form.addEventListener("submit", fetchMetadata);
  ui.run.addEventListener("click", getContent);
  ui.chooseDir.addEventListener("click", chooseDir);

  const [languages, auth, downloads] = await Promise.all([
    api.targetLanguages(),
    api.credentialSource(),
    downloadDir(),
  ]);
  renderTargets(languages);
  ui.auth.textContent = `Claude via ${auth}`;
  ui.outDir.value = await join(downloads, OUTPUT_FOLDER);
}

window.addEventListener("DOMContentLoaded", () => {
  init().catch((err) => setStatus(String(err), true));
});

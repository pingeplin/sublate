# Contents Title

Tauri 2 + Rust desktop app: paste a video URL, inspect its metadata with `yt-dlp`, download the video, and translate a subtitle track into another language with Claude. Subtitles are written as standalone `.srt` files — never merged into the video.

## Requirements

- `yt-dlp`, `ffmpeg`, and a JS runtime (`deno`, or `node` — passed via `--js-runtimes`) on the login-shell `PATH`
- `ant` CLI logged in to the **contents-title** workspace:

  ```sh
  ant auth login --profile contents-title
  ```

  Credential resolution: `ANTHROPIC_API_KEY` (if set) → `ant` profile from `ANTHROPIC_PROFILE` → `contents-title`.

## Run

```sh
pnpm install
pnpm tauri dev
```

## Output

Files land in `~/Downloads/contents-title/` by default:

| File | Content |
| --- | --- |
| `<id>.mp4` | Video (no embedded subtitles) |
| `<id>.<src>.srt` | Source subtitle as downloaded (e.g. `ko-orig`) |
| `<id>.<target>.srt` | Translated subtitle (e.g. `zh-TW`) |

YouTube auto-captions scroll (each cue repeats the previous line); the translated file collapses them into clean, non-overlapping cues.

## Tests

```sh
cd src-tauri
cargo test                                            # offline
cargo test --test live_test -- --ignored --nocapture  # hits YouTube + Claude API
```

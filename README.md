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

Files land in `~/Downloads/contents-title/` by default, named `<title> [<id>]` so same-titled videos never collide (reserved characters such as `? / : $` become full-width look-alikes; the id alone is used if the title is empty):

| File | Content |
| --- | --- |
| `<title> [<id>].<ext>` | Video in yt-dlp's best format (usually `.webm`/`.mkv`; play with IINA or VLC), no embedded subtitles |
| `<title> [<id>].<src>.srt` | Source subtitle as downloaded (e.g. `ko-orig`) |
| `<title> [<id>].<target>.srt` | Translated subtitle (e.g. `zh-TW`; `.<target>.translated.srt` when source and target codes match) |

YouTube auto-captions scroll (each cue repeats the previous line); the translated file collapses them into clean, non-overlapping cues. Translation uses `claude-sonnet-5-5` (effort `medium`); Chinese and Japanese output is post-processed so half-width punctuation next to CJK text becomes full-width.

## Tests

```sh
cd src-tauri
cargo test                                            # offline
cargo test --test live_test -- --ignored --nocapture  # hits YouTube + Claude API
```

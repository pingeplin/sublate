# Contents Title

Native macOS app (SwiftUI front end, Rust core): paste a video URL, inspect its metadata with `yt-dlp`, download the video, and translate a subtitle track into another language with Claude. Subtitles are written as standalone `.srt` files — never merged into the video.

## Requirements

- macOS 26, Xcode 27, Rust, and [XcodeGen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`)
- `yt-dlp`, `ffmpeg`, and a JS runtime (`deno`, or `node` — passed via `--js-runtimes`) on the login-shell `PATH`
- `ant` CLI logged in to the **contents-title** workspace:

  ```sh
  ant auth login --profile contents-title
  ```

  Credential resolution: `ANTHROPIC_API_KEY` (if set) → `ant` profile from `ANTHROPIC_PROFILE` → `contents-title`.

## Run

```sh
make run   # builds the Rust core, generates the Swift bindings and Xcode project, then opens the app
```

`make project` alone produces `app/ContentsTitle.xcodeproj` for working in Xcode; rerun `make core` after changing Rust code.

## Layout

| Path | Content |
| --- | --- |
| `core/` | Rust crate: yt-dlp, subtitle parsing, Claude translation. `src/ffi.rs` is the only surface the UI sees, exported through [UniFFI](https://mozilla.github.io/uniffi-rs/). |
| `app/Kit/` | Swift package: `ContentsTitleCore` (generated bindings over the static library) and `ContentsTitleUI` (view model and SwiftUI views). |
| `app/ContentsTitle/` | The app shell; `app/project.yml` is the XcodeGen spec. |

## Output

Files land in `~/Downloads/contents-title/` by default, named `<title> [<id>]` so same-titled videos never collide (reserved characters such as `? / : $` become full-width look-alikes; the id alone is used if the title is empty). Long titles are shortened so every name stays within 255 bytes in decomposed (NFD) UTF-8, the limit Synology Drive enforces:

| File | Content |
| --- | --- |
| `<title> [<id>].mp4` | Best-quality streams copied into mp4 (no re-encode) with the YouTube thumbnail as cover art, so Finder/Quick Look previews show it; no embedded subtitles. AV1 videos need VLC or IINA. |
| `<title> [<id>].<src>.srt` | Source subtitle as downloaded (e.g. `ko-orig`) |
| `<title> [<id>].<target>.srt` | Translated subtitle (e.g. `zh-TW`; `.<target>.translated.srt` when source and target codes match) |

YouTube auto-captions scroll (each cue repeats the previous line); the translated file collapses them into clean, non-overlapping cues. Translation uses `claude-sonnet-5-5` (effort `medium`); Chinese and Japanese output is post-processed so half-width punctuation next to CJK text becomes full-width.

## Tests

```sh
make test                                                                         # offline: Rust core + Swift view model
cargo test --manifest-path core/Cargo.toml --test live_test -- --ignored --nocapture  # hits YouTube + Claude API
LIVE=1 swift test --package-path app/Kit --filter LiveBridgeTests                  # same, through the Swift bridge
```

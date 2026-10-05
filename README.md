# Sublate

Native macOS app (SwiftUI front end, Rust core): paste a video URL, inspect its metadata with `yt-dlp`, download the video, and translate a subtitle track into another language with Claude. Subtitles are written as standalone `.srt` files — never merged into the video.

## Install

Download the disk image, open it and drag **Sublate** to Applications. Nothing else needs installing: `yt-dlp`, `ffmpeg` and the `deno` JavaScript runtime it relies on are inside the app. It needs macOS 26 on Apple Silicon.

Translation uses your own Anthropic API key: paste it in **Settings (⌘,)**, where it is kept in your Keychain. Fetching and downloading work without one.

Sites change faster than the app ships, so once a day the app looks for a newer official `yt-dlp` release, verifies its checksum, and installs it under `~/Library/Application Support/tech.radiw.sublate/`; **Settings → Check for Updates** does the same on demand. The newer of that copy and the bundled one is used.

## Build

Requirements: macOS 26, Xcode 27, Rust, [XcodeGen](https://github.com/yonaskolb/XcodeGen) (`brew install xcodegen`), and a Developer ID Application certificate (`SIGN_IDENTITY` in the `Makefile`).

```sh
make run       # downloads and signs the bundled tools, builds the Rust core and the app, then opens it
```

`make project` alone produces `app/Sublate.xcodeproj` for working in Xcode; rerun `make core` after changing Rust code.

The bundled tools are pinned by version and checksum in `vendor/tools.lock`; `make tools` rebuilds `vendor/tools` after a change there. Their licences are listed in `vendor/licenses/README.md` and ship inside the app.

Developers can skip the saved key: without one, the app falls back to `ANTHROPIC_API_KEY`, then to the `ant` profile named by `ANTHROPIC_PROFILE` (default `contents-title`, created with `ant auth login --profile contents-title`), both read from the login shell.

## Layout

| Path | Content |
| --- | --- |
| `core/` | Rust crate: yt-dlp and its updater, subtitle parsing, Claude translation. `src/ffi.rs` is the only surface the UI sees, exported through [UniFFI](https://mozilla.github.io/uniffi-rs/). |
| `app/Kit/` | Swift package: `SublateCore` (generated bindings over the static library) and `SublateUI` (view model and SwiftUI views). |
| `app/Sublate/` | The app shell; `app/project.yml` is the XcodeGen spec. |
| `vendor/` | Pinned third-party tools: `fetch.sh` downloads, verifies and signs them into `vendor/tools`, which the app bundles as `Contents/Resources/tools`. |

## Output

Files land in `~/Downloads/Sublate/` by default, named `<title> [<id>]` so same-titled videos never collide (reserved characters such as `? / : $` become full-width look-alikes; the id alone is used if the title is empty). Long titles are shortened so every name stays within 255 bytes in decomposed (NFD) UTF-8, the limit Synology Drive enforces:

| File | Content |
| --- | --- |
| `<title> [<id>].mp4` | Best-quality streams copied into mp4 (no re-encode) with the YouTube thumbnail as cover art, so Finder/Quick Look previews show it; no embedded subtitles. AV1 videos need VLC or IINA. |
| `<title> [<id>].<src>.srt` | Source subtitle as downloaded (e.g. `ko-orig`) |
| `<title> [<id>].<target>.srt` | Translated subtitle (e.g. `zh-TW`; `.<target>.translated.srt` when source and target codes match) |

YouTube auto-captions scroll (each cue repeats the previous line); the translated file collapses them into clean, non-overlapping cues. Translation uses `claude-sonnet-5-5` (effort `medium`); Chinese and Japanese output is post-processed so half-width punctuation next to CJK text becomes full-width.

## Tests

```sh
make test                                                                         # offline: Rust core + Swift view models
cargo test --manifest-path core/Cargo.toml --test live_test -- --ignored --nocapture  # hits YouTube, GitHub + Claude API
LIVE=1 swift test --package-path app/Kit                                           # same through the Swift bridge, plus the Keychain
```

The live tests run the vendored tools (`make tools`), the same binaries the app ships.

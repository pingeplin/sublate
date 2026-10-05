# Third-party software

Sublate itself is licensed under GPL-3.0-or-later (`GPL-3.0.txt`).

Sublate ships these programs unmodified inside the app (`Contents/Resources/tools`) and runs them as separate processes. Each stays under its own licence.

| Program | Version | Licence | Binary from | Source |
| --- | --- | --- | --- | --- |
| FFmpeg (`ffmpeg`, `ffprobe`) | 9.0.2 | GPL-3.0-or-later — `GPL-3.0.txt` | <https://ffmpeg.martin-riedl.de> | <https://ffmpeg.org/releases/ffmpeg-9.0.2.tar.xz>, build scripts at <https://git.martin-riedl.de/ffmpeg/build-script> |
| yt-dlp | 2026.08.19 | Unlicense; the bundled build includes GPL-3.0-or-later components — `yt-dlp-THIRD_PARTY_LICENSES.txt`, `GPL-3.0.txt` | <https://github.com/yt-dlp/yt-dlp/releases> | <https://github.com/yt-dlp/yt-dlp/archive/refs/tags/2026.08.19.tar.gz> |
| Deno | 2.9.7 | MIT — `deno-LICENSE.md` | <https://github.com/denoland/deno/releases> | <https://github.com/denoland/deno/tree/v2.9.7> |

The app may later download a newer official yt-dlp release into `~/Library/Application Support/tech.radiw.sublate/yt-dlp`; its licences are in that release's `_internal/THIRD_PARTY_LICENSES.txt`.

The complete corresponding source of the GPL-licensed programs is available from the links above. If one of them stops working, open an issue on this project and the source will be provided.

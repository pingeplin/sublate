#!/bin/bash
# Builds vendor/tools, the directory copied into the app bundle: downloads the tools pinned in
# tools.lock, verifies their checksums and signs every binary for the hardened runtime.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
identity="${SIGN_IDENTITY:?set SIGN_IDENTITY to a code-signing identity}"
cache="$here/cache"
stage="$here/.stage"
out="$here/tools"

# shellcheck source=tools.lock
source "$here/tools.lock"

matches() {
    [[ -f "$1" && "$(shasum -a 256 "$1" | cut -d' ' -f1)" == "$2" ]]
}

fetch() {
    local file="$cache/$1" url="$2" sha256="$3"
    if ! matches "$file" "$sha256"; then
        echo "Downloading $1"
        curl --fail --location --silent --show-error --retry 3 --output "$file" "$url"
    fi
    if ! matches "$file" "$sha256"; then
        echo "error: $1 does not match its pinned checksum" >&2
        exit 1
    fi
}

sign() {
    codesign --force --options runtime --timestamp --sign "$identity" "$@"
}

# The release zip stores Python.framework's symlinks as copies, which codesign rejects as an
# ambiguous bundle; this puts back the standard framework layout.
restore_framework_links() {
    local internal="$1" version
    version="$(basename "$(find "$internal/Python.framework/Versions" -mindepth 1 -maxdepth 1 ! -name Current)")"
    rm -rf "$internal/Python" "$internal/Python.framework/"{Python,Resources,Versions/Current}
    ln -s "$version" "$internal/Python.framework/Versions/Current"
    ln -s Versions/Current/Python "$internal/Python.framework/Python"
    ln -s Versions/Current/Resources "$internal/Python.framework/Resources"
    ln -s "Python.framework/Versions/$version/Python" "$internal/Python"
}

sign_mach_o_files() {
    local file
    while IFS= read -r -d '' file; do
        if [[ "$(file --brief "$file")" == Mach-O* ]]; then
            sign "$file"
        fi
    done < <(find "$1" -type f -print0)
}

mkdir -p "$cache"
fetch "yt-dlp-$YTDLP_VERSION.zip" "$YTDLP_URL" "$YTDLP_SHA256"
fetch "deno-$DENO_VERSION.zip" "$DENO_URL" "$DENO_SHA256"
fetch "ffmpeg-$FFMPEG_BUILD.zip" "$FFMPEG_URL" "$FFMPEG_SHA256"
fetch "ffprobe-$FFMPEG_BUILD.zip" "$FFPROBE_URL" "$FFPROBE_SHA256"

rm -rf "$stage"
mkdir -p "$stage"
ditto -x -k "$cache/yt-dlp-$YTDLP_VERSION.zip" "$stage/yt-dlp"
ditto -x -k "$cache/deno-$DENO_VERSION.zip" "$stage"
ditto -x -k "$cache/ffmpeg-$FFMPEG_BUILD.zip" "$stage"
ditto -x -k "$cache/ffprobe-$FFMPEG_BUILD.zip" "$stage"
restore_framework_links "$stage/yt-dlp/_internal"

echo "Signing as $identity"
sign_mach_o_files "$stage/yt-dlp"
sign "$stage/ffmpeg" "$stage/ffprobe"
sign --entitlements "$here/deno.entitlements" "$stage/deno"

cp -R "$here/licenses" "$stage/licenses"
cp "$stage/yt-dlp/_internal/THIRD_PARTY_LICENSES.txt" "$stage/licenses/yt-dlp-THIRD_PARTY_LICENSES.txt"
printf '%s\n' "$YTDLP_VERSION" > "$stage/yt-dlp.version"

rm -rf "$out"
mv "$stage" "$out"
echo "Tools ready in $out"

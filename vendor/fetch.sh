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

mkdir -p "$cache"
fetch "deno-$DENO_VERSION.zip" "$DENO_URL" "$DENO_SHA256"
fetch "ffmpeg-$FFMPEG_BUILD.zip" "$FFMPEG_URL" "$FFMPEG_SHA256"
fetch "ffprobe-$FFMPEG_BUILD.zip" "$FFPROBE_URL" "$FFPROBE_SHA256"

rm -rf "$stage"
mkdir -p "$stage"
ditto -x -k "$cache/deno-$DENO_VERSION.zip" "$stage"
ditto -x -k "$cache/ffmpeg-$FFMPEG_BUILD.zip" "$stage"
ditto -x -k "$cache/ffprobe-$FFMPEG_BUILD.zip" "$stage"

echo "Signing as $identity"
sign "$stage/ffmpeg" "$stage/ffprobe"
sign --entitlements "$here/deno.entitlements" "$stage/deno"

cp -R "$here/licenses" "$stage/licenses"

rm -rf "$out"
mv "$stage" "$out"
echo "Tools ready in $out"

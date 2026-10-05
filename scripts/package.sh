#!/bin/bash
# Packs the built app into a signed disk image that opens on the app beside an Applications
# shortcut, ready to be dragged across. dmgbuild writes the window layout itself, so no
# Finder scripting is involved.
# With a notarytool keychain profile, the app and the image are notarized and stapled, so
# Gatekeeper accepts them even on a Mac that is offline at first launch.
#
# usage: package.sh <app> <dmg> <signing identity> [notary profile]
set -euo pipefail

DMGBUILD_VERSION=1.6.7

app="$1" dmg="$2" identity="$3" profile="${4:-}"
here="$(cd "$(dirname "$0")" && pwd)"
name="$(basename "$app" .app)"
work="$(mktemp -d)"
staged="$work/$name.app"
trap 'rm -rf "$work"' EXIT

notarize() {
    xcrun notarytool submit "$1" --keychain-profile "$profile" --wait
}

mkdir -p "$(dirname "$dmg")"
ditto "$app" "$staged"

if [[ -n "$profile" ]]; then
    ditto -c -k --keepParent "$staged" "$work/app.zip"
    notarize "$work/app.zip"
    xcrun stapler staple "$staged"
fi

rm -f "$dmg"
uvx --from "dmgbuild==$DMGBUILD_VERSION" dmgbuild \
    -s "$here/dmg.settings.py" -D app="$staged" "$name" "$dmg"
codesign --timestamp --sign "$identity" "$dmg"

if [[ -n "$profile" ]]; then
    notarize "$dmg"
    xcrun stapler staple "$dmg"
    spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
fi
echo "Packaged $dmg"

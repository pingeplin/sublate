#!/bin/bash
# Packs the built app into a signed disk image with an Applications shortcut.
# With a notarytool keychain profile, the app and the image are notarized and stapled, so
# Gatekeeper accepts them even on a Mac that is offline at first launch.
#
# usage: package.sh <app> <dmg> <signing identity> [notary profile]
set -euo pipefail

app="$1" dmg="$2" identity="$3" profile="${4:-}"
name="$(basename "$app" .app)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

notarize() {
    xcrun notarytool submit "$1" --keychain-profile "$profile" --wait
}

mkdir -p "$work/image" "$(dirname "$dmg")"
ditto "$app" "$work/image/$name.app"

if [[ -n "$profile" ]]; then
    ditto -c -k --keepParent "$work/image/$name.app" "$work/app.zip"
    notarize "$work/app.zip"
    xcrun stapler staple "$work/image/$name.app"
fi

ln -s /Applications "$work/image/Applications"
rm -f "$dmg"
hdiutil create -volname "$name" -srcfolder "$work/image" -format ULMO "$dmg"
codesign --timestamp --sign "$identity" "$dmg"

if [[ -n "$profile" ]]; then
    notarize "$dmg"
    xcrun stapler staple "$dmg"
    spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
fi
echo "Packaged $dmg"

#!/bin/bash
# Fails when an object in a library needs a newer macOS than the app supports, such as a
# standard library that Homebrew's Rust built for the macOS it was bottled on.
set -euo pipefail

library="${1:?usage: check-minos.sh LIBRARY [MINIMUM]}"
minimum="${2:-${MACOSX_DEPLOYMENT_TARGET:?set MACOSX_DEPLOYMENT_TARGET or pass MINIMUM}}"

newest="$(otool -l "$library" | awk '/LC_BUILD_VERSION/ { found = 1 } found && /minos/ { print $2; found = 0 }' | sort -V | tail -1)"

if [[ -z "$newest" ]]; then
    echo "error: $library has no LC_BUILD_VERSION" >&2
    exit 1
fi

if [[ "$(printf '%s\n%s\n' "$newest" "$minimum" | sort -V | tail -1)" != "$minimum" ]]; then
    echo "error: $library has objects for macOS $newest; the app supports macOS $minimum." >&2
    echo "Build with the Rust toolchain from rustup, whose standard library supports older macOS." >&2
    exit 1
fi

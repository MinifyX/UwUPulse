#!/usr/bin/env bash
# Generates the Xcode project and builds both apps unsigned for the simulators.
# Usage: apple/scripts/build.sh [--test]   (--test also runs the PulseKit tests)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
derived="$root/build/DerivedData"
mkdir -p "$root/build"

command -v xcodegen >/dev/null || { echo "xcodegen missing (brew install xcodegen)" >&2; exit 1; }
(cd "$root" && xcodegen generate --quiet)

build() { # $1 = scheme, $2 = destination
  local log="$root/build/$1.log"
  if xcodebuild -project "$root/UwUPulse.xcodeproj" -scheme "$1" -destination "$2" \
      -derivedDataPath "$derived" CODE_SIGNING_ALLOWED=NO build >"$log" 2>&1; then
    grep -E '^/.*warning:' "$log" | sort -u || true
    echo "$1: BUILD SUCCEEDED"
  else
    grep -E '^/.*error:|error:' "$log" | sort -u | head -30
    echo "$1: BUILD FAILED, see $log" >&2
    exit 1
  fi
}

# The iOS build also builds and embeds the watch app.
build UwUPulse 'generic/platform=iOS Simulator'
build UwUPulseWatch 'generic/platform=watchOS Simulator'

if [ "${1:-}" = "--test" ]; then
  (cd "$root/PulseKit" && swift test 2>&1 | grep -E 'error:|✘|Test run|passed|failed' | tail -20)
fi

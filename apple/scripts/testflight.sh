#!/usr/bin/env bash
# Archives UwUPulse (iOS app with embedded watch app) and uploads it to TestFlight.
#
# Same flow as SoChat-iOS/scripts/testflight.sh:
# - archive with automatic signing via the App Store Connect API key,
# - export via the Xcode account (cloud-managed distribution certificate),
# - fallback (no Xcode account, or SIGNING=manual): own "Apple Distribution" certificate
#   from the keychain plus the API-created profiles "UwUPulse AppStore <bundle> (API)",
# - upload with the API key via altool.
#
# Usage:
#   ASC_KEY_PATH=/path/to/AuthKey_<ID>.p8 ASC_ISSUER_ID=… ASC_APP_ID=… apple/scripts/testflight.sh [--no-upload]
#
# Environment (nothing secret lives in the repo):
#   ASC_KEY_PATH   path to the .p8 key (never in the repo), named AuthKey_<ASC_KEY_ID>.p8
#   ASC_KEY_ID     key id (default: taken from the file name)
#   ASC_ISSUER_ID  issuer id (required)
#   ASC_APP_ID     Apple ID of the app in App Store Connect (required for upload)
#   VERSION        default: MARKETING_VERSION from Config/UwUPulse.xcconfig
#   BUILD_NUMBER   default: UTC timestamp YYMMDDhhmm (always increasing)
#   SIGNING        automatic (default) | manual
set -euo pipefail

upload=1
for arg in "$@"; do
  case "$arg" in
    --no-upload) upload=0 ;;
    *) echo "Unknown option: $arg" >&2; exit 2 ;;
  esac
done

key_path="${ASC_KEY_PATH:?ASC_KEY_PATH missing (AuthKey_<ID>.p8)}"
key_file="$(basename "$key_path")"
key_id="${ASC_KEY_ID:-$(echo "$key_file" | sed -n 's/^AuthKey_\(.*\)\.p8$/\1/p')}"
issuer_id="${ASC_ISSUER_ID:?ASC_ISSUER_ID missing}"
[ -n "$key_id" ] || { echo "ASC_KEY_ID missing and not derivable from $key_file." >&2; exit 1; }
[ -r "$key_path" ] || { echo "Cannot read $key_path." >&2; exit 1; }
[ "$key_file" = "AuthKey_$key_id.p8" ] || { echo "The .p8 must be named AuthKey_$key_id.p8 (altool requirement)." >&2; exit 1; }
if [ "$upload" -eq 1 ]; then asc_app_id="${ASC_APP_ID:?ASC_APP_ID missing (or use --no-upload)}"; fi

root="$(cd "$(dirname "$0")/.." && pwd)"
config="$root/Config/UwUPulse.xcconfig"
build="$root/build"
setting() { sed -n "s/^$1 *= *//p" "$config" | head -1; }

version="${VERSION:-$(setting MARKETING_VERSION)}"
number="${BUILD_NUMBER:-$(date -u +%y%m%d%H%M)}"
team="$(setting DEVELOPMENT_TEAM)"
bundle_id="$(setting UWUPULSE_BUNDLE_ID)"
watch_bundle_id="$bundle_id.watchkitapp"
[ -n "$team" ] || { echo "DEVELOPMENT_TEAM missing in $config." >&2; exit 1; }

echo "UwUPulse iOS $version (build $number), team $team"

command -v xcodegen >/dev/null || { echo "xcodegen missing (brew install xcodegen)" >&2; exit 1; }
(cd "$root" && xcodegen generate --quiet)

rm -rf "$build/UwUPulse.xcarchive" "$build/export" "$build/ExportOptions.plist" "$build"/archive.log "$build"/export.log "$build"/upload.log
mkdir -p "$build"

archive_with() { # $1 = automatic | manual
  rm -rf "$build/UwUPulse.xcarchive"
  local signing=(-allowProvisioningUpdates -authenticationKeyPath "$key_path" -authenticationKeyID "$key_id" -authenticationKeyIssuerID "$issuer_id")
  if [ "$1" = manual ]; then
    signing=(UWUPULSE_SIGNING=Manual)
  fi
  xcodebuild archive \
    -project "$root/UwUPulse.xcodeproj" \
    -scheme UwUPulse \
    -configuration Release \
    -destination 'generic/platform=iOS' \
    -archivePath "$build/UwUPulse.xcarchive" \
    "${signing[@]}" \
    MARKETING_VERSION="$version" \
    CURRENT_PROJECT_VERSION="$number" \
    > "$build/archive.log" 2>&1
}
if ! archive_with "${SIGNING:-automatic}"; then
  if [ "${SIGNING:-automatic}" = automatic ] && grep -qE "No Accounts|Authentication failed|No profiles for" "$build/archive.log"; then
    echo "Automatic signing failed, archiving with the own distribution certificate."
    SIGNING=manual
    archive_with manual \
      || { grep -E 'error:' "$build/archive.log" | head -20; echo "Archive failed, see $build/archive.log" >&2; exit 1; }
  else
    grep -E 'error:' "$build/archive.log" | head -20; echo "Archive failed, see $build/archive.log" >&2; exit 1
  fi
fi
echo "Archive done."

write_export_options() { # $1 = automatic | manual
  {
    cat <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>export</string>
  <key>teamID</key><string>$team</string>
  <key>signingStyle</key><string>$1</string>
  <key>manageAppVersionAndBuildNumber</key><false/>
  <key>uploadSymbols</key><true/>
PLIST
    if [ "$1" = manual ]; then
      cat <<PLIST
  <key>signingCertificate</key><string>Apple Distribution</string>
  <key>provisioningProfiles</key>
  <dict>
    <key>$bundle_id</key><string>UwUPulse AppStore $bundle_id (API)</string>
    <key>$watch_bundle_id</key><string>UwUPulse AppStore $watch_bundle_id (API)</string>
  </dict>
PLIST
    fi
    echo "</dict>"; echo "</plist>"
  } > "$build/ExportOptions.plist"
}

export_with() { # $1 = signing style, $2 = non-empty to allow provisioning updates
  write_export_options "$1"
  rm -rf "$build/export"
  xcodebuild -exportArchive \
    -archivePath "$build/UwUPulse.xcarchive" \
    -exportOptionsPlist "$build/ExportOptions.plist" \
    -exportPath "$build/export" \
    ${2:+-allowProvisioningUpdates} \
    > "$build/export.log" 2>&1
}
if ! export_with "${SIGNING:-automatic}" yes; then
  if [ "${SIGNING:-automatic}" = automatic ] && grep -q "No Accounts" "$build/export.log"; then
    echo "No Xcode account, exporting with the own distribution certificate."
    export_with manual \
      || { grep -E 'error' "$build/export.log" | head -20; echo "Export failed, see $build/export.log" >&2; exit 1; }
  else
    grep -E 'error' "$build/export.log" | head -20; echo "Export failed (signed in to Xcode?), see $build/export.log" >&2; exit 1
  fi
fi

ipa="$(ls "$build/export"/*.ipa | head -1)"
if [ "$upload" -eq 0 ]; then
  echo "IPA: $ipa"
  exit 0
fi

API_PRIVATE_KEYS_DIR="$(cd "$(dirname "$key_path")" && pwd)" xcrun altool --upload-package "$ipa" \
  -t ios \
  --apiKey "$key_id" \
  --apiIssuer "$issuer_id" \
  --apple-id "$asc_app_id" \
  --bundle-id "$bundle_id" \
  --bundle-version "$number" \
  --bundle-short-version-string "$version" \
  > "$build/upload.log" 2>&1 \
  || { tail -20 "$build/upload.log"; echo "Upload failed, see $build/upload.log" >&2; exit 1; }

echo "Uploaded UwUPulse iOS $version ($number). TestFlight processes the build in a few minutes."

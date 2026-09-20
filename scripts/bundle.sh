#!/usr/bin/env bash
#
# Assemble dist/Hourglass.app around an already-compiled binary.
#
# Launching through a bundle is not just cosmetic: LSUIElement only takes effect
# from Info.plist, and LaunchServices starts the app detached from whatever
# shell invoked it, so it outlives the terminal session.
#
# Usage: scripts/bundle.sh [debug|release]   (default: release)

set -euo pipefail

profile="${1:-release}"
case "$profile" in
    debug | release) ;;
    *)
        echo "usage: $0 [debug|release]" >&2
        exit 2
        ;;
esac

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Respect CARGO_TARGET_DIR so sandboxed or shared target directories still work.
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
binary="$target_dir/$profile/hourglass"
if [[ ! -x "$binary" ]]; then
    echo "no binary at $binary; run: cargo build -p hourglass --profile $profile" >&2
    exit 1
fi

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
app="$repo_root/dist/Hourglass.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/hourglass"

icon_entry=""
if [[ -f assets/icon.icns ]]; then
    cp assets/icon.icns "$app/Contents/Resources/icon.icns"
    icon_entry="
    <key>CFBundleIconFile</key>
    <string>icon</string>"
fi

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>hourglass</string>
    <key>CFBundleIdentifier</key>
    <string>app.hourglass.mac</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>Hourglass</string>
    <key>CFBundleDisplayName</key>
    <string>Hourglass</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>$version</string>
    <key>CFBundleVersion</key>
    <string>$version</string>$icon_entry
    <key>LSMinimumSystemVersion</key>
    <string>13.0</string>
    <!-- Hourglass lives in the menu bar: no Dock tile, no app switcher entry. -->
    <key>LSUIElement</key>
    <true/>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
PLIST

# An ad-hoc signature keeps macOS from re-prompting for permissions every build.
codesign --force --sign - --timestamp=none "$app" 2>/dev/null || true

echo "built $app ($profile, v$version)"

#!/usr/bin/env bash
#
# build_local_dmg.sh — local smoke of the macOS .app + DMG path.
#
# Mirrors the macOS section of .github/actions/native-bundle/action.yml
# so you can verify the bundle path end-to-end on your Mac without CI.
#
# Prereqs:
#   brew install create-dmg
#   cargo build --release
#
# Output:
#   ./bundle-out/Solarxy-<ver>-<arch>.dmg
#
# Usage:
#   ./scripts/build_local_dmg.sh                 # defaults to current arch, v0.5.0
#   V=0.5.0 TARGET=x86_64-apple-darwin ./scripts/build_local_dmg.sh
#
set -euo pipefail

# ---- Defaults (can be overridden via env) ---------------------------------
: "${V:=0.5.0}"
if [ -z "${TARGET:-}" ]; then
    case "$(uname -m)" in
        arm64)  TARGET="aarch64-apple-darwin" ;;
        x86_64) TARGET="x86_64-apple-darwin"  ;;
        *) echo "Unsupported arch: $(uname -m)"; exit 1 ;;
    esac
fi
: "${BINARY:=target/release/solarxy}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [ ! -x "$BINARY" ]; then
    echo "Binary not found at $BINARY."
    echo "Run 'cargo build --release' first."
    exit 1
fi
if ! command -v create-dmg >/dev/null 2>&1; then
    echo "create-dmg not found."
    echo "Run 'brew install create-dmg' first."
    exit 1
fi

ARCH="${TARGET%%-*}"
STAGE="$(mktemp -d)/stage"
mkdir -p "$STAGE"

APP="$STAGE/Solarxy.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BINARY" "$APP/Contents/MacOS/solarxy"
chmod +x "$APP/Contents/MacOS/solarxy"
cp res/bundle/solarxy.icns "$APP/Contents/Resources/Solarxy.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>        <string>solarxy</string>
    <key>CFBundleIdentifier</key>        <string>dev.koljam.solarxy</string>
    <key>CFBundleName</key>              <string>Solarxy</string>
    <key>CFBundleDisplayName</key>       <string>Solarxy</string>
    <key>CFBundleShortVersionString</key><string>${V}</string>
    <key>CFBundleVersion</key>           <string>${V}</string>
    <key>CFBundleIconFile</key>          <string>Solarxy.icns</string>
    <key>CFBundlePackageType</key>       <string>APPL</string>
    <key>LSMinimumSystemVersion</key>    <string>11.0</string>
    <key>NSHighResolutionCapable</key>   <true/>
    <key>LSApplicationCategoryType</key> <string>public.app-category.developer-tools</string>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$APP"
plutil -lint "$APP/Contents/Info.plist"

cp "res/bundle/macos/Install CLI.command" "$STAGE/"
cp "res/bundle/macos/README.txt" "$STAGE/"

mkdir -p bundle-out
OUT="bundle-out/Solarxy-${V}-${ARCH}.dmg"
rm -f "$OUT"

create-dmg \
    --volname "Solarxy ${V}" \
    --window-size 600 340 \
    --icon-size 96 \
    --app-drop-link 450 185 \
    --icon "Solarxy.app" 150 185 \
    --hide-extension "Solarxy.app" \
    --no-internet-enable \
    "$OUT" \
    "$STAGE/"

echo
echo "=== DMG ready ==="
ls -la "$OUT"
echo
echo "Next:"
echo "  open \"$OUT\""
echo "  # drag Solarxy.app to /Applications, eject, launch from Launchpad"

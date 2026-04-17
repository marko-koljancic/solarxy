#!/bin/bash
# Solarxy CLI installer — run after dragging Solarxy.app to /Applications.
#
# Two things happen here:
#   1. Clear the `com.apple.quarantine` extended attribute from
#      /Applications/Solarxy.app. macOS sets this on every file extracted
#      from a downloaded DMG; with it set, the first launch triggers
#      Gatekeeper ("Solarxy cannot be verified"). Clearing it means the
#      next Launchpad/Finder open just works, no System Settings dance.
#   2. Symlink /usr/local/bin/solarxy-cli → the CLI binary inside
#      Solarxy.app so you can invoke `solarxy-cli` from any terminal.
#      Prompts for sudo on first run because /usr/local/bin is root-owned.
#
# The GUI itself (Solarxy.app) is launched from Launchpad / Finder; only
# the companion CLI needs to be on PATH for terminal use.

set -euo pipefail

APP_BUNDLE="/Applications/Solarxy.app"
GUI_BIN="$APP_BUNDLE/Contents/MacOS/solarxy"
CLI_BIN="$APP_BUNDLE/Contents/MacOS/solarxy-cli"
if [ ! -x "$GUI_BIN" ] || [ ! -x "$CLI_BIN" ]; then
    osascript -e 'display dialog "Solarxy.app is not in /Applications, or is missing its bundled CLI. Drag Solarxy.app to /Applications, then re-run Install CLI." buttons {"OK"} default button "OK" with icon stop'
    exit 1
fi

# `|| true` because xattr -dr errors if the attribute is already absent
# (e.g. user already cleared it via System Settings → Open Anyway).
xattr -dr com.apple.quarantine "$APP_BUNDLE" 2>/dev/null || true

sudo mkdir -p /usr/local/bin
sudo ln -sf "$CLI_BIN" /usr/local/bin/solarxy-cli

osascript -e 'display dialog "Solarxy CLI installed — and Gatekeeper has been cleared for Solarxy.app.\n\nOpen Solarxy from Launchpad without any extra prompts.\nOr open a new Terminal window and run:\n    solarxy-cli --help" buttons {"Done"} default button "Done"'

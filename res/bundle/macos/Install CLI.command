#!/bin/bash
# Solarxy CLI installer — run after dragging Solarxy.app to /Applications.
#
# Two things happen here:
#   1. Clear the `com.apple.quarantine` extended attribute from
#      /Applications/Solarxy.app. macOS sets this on every file extracted
#      from a downloaded DMG; with it set, the first launch triggers
#      Gatekeeper ("Solarxy cannot be verified"). Clearing it means the
#      next Launchpad/Finder open just works, no System Settings dance.
#   2. Symlink /usr/local/bin/solarxy → the binary inside Solarxy.app so
#      you can invoke `solarxy` from any terminal. Prompts for sudo on
#      first run because /usr/local/bin is root-owned.

set -euo pipefail

APP_BUNDLE="/Applications/Solarxy.app"
APP="$APP_BUNDLE/Contents/MacOS/solarxy"
if [ ! -x "$APP" ]; then
    osascript -e 'display dialog "Solarxy.app is not in /Applications. Drag Solarxy.app there first, then re-run Install CLI." buttons {"OK"} default button "OK" with icon stop'
    exit 1
fi

# `|| true` because xattr -dr errors if the attribute is already absent
# (e.g. user already cleared it via System Settings → Open Anyway).
xattr -dr com.apple.quarantine "$APP_BUNDLE" 2>/dev/null || true

sudo mkdir -p /usr/local/bin
sudo ln -sf "$APP" /usr/local/bin/solarxy

osascript -e 'display dialog "Solarxy CLI installed — and Gatekeeper has been cleared for Solarxy.app.\n\nOpen Solarxy from Launchpad without any extra prompts.\nOr open a new Terminal window and run:\n    solarxy --help" buttons {"Done"} default button "Done"'

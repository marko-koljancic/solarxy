#!/bin/bash
# Solarxy CLI installer — run after dragging Solarxy.app to /Applications.
#
# Symlinks /usr/local/bin/solarxy → the binary inside Solarxy.app so you can
# invoke `solarxy` from any terminal. Prompts for sudo on first run because
# /usr/local/bin is root-owned.

set -euo pipefail

APP="/Applications/Solarxy.app/Contents/MacOS/solarxy"
if [ ! -x "$APP" ]; then
    osascript -e 'display dialog "Solarxy.app is not in /Applications. Drag Solarxy.app there first, then re-run Install CLI." buttons {"OK"} default button "OK" with icon stop'
    exit 1
fi

sudo mkdir -p /usr/local/bin
sudo ln -sf "$APP" /usr/local/bin/solarxy

osascript -e 'display dialog "Solarxy CLI installed.\n\nOpen a new Terminal window and run:\n    solarxy --help" buttons {"Done"} default button "Done"'

# Solarxy GUI cask. Lives in the koljam/homebrew-solarxy tap.
#
# Usage:
#   brew install --cask koljam/solarxy/solarxy
#
# This cask handles the macOS Gatekeeper friction that the bare DMG
# download cannot. The `postflight` block strips
# com.apple.quarantine from the installed .app and writes the
# install-source marker so the GUI's "Check for Updates" suggests
# `brew upgrade --cask` rather than the GitHub releases page.
#
# Note: stripping quarantine via postflight is rare in cask but
# legitimate here — Solarxy is ad-hoc signed (not Apple-Developer-ID
# signed), and the alternative is requiring users to right-click-Open
# every time they install or upgrade. Cask is the only distribution
# channel where we can do this without a separate user gesture.

cask "solarxy" do
  version :latest
  sha256 :no_check

  on_arm do
    url "https://github.com/marko-koljancic/solarxy/releases/latest/download/Solarxy-#{version}-aarch64.dmg"
  end

  on_intel do
    url "https://github.com/marko-koljancic/solarxy/releases/latest/download/Solarxy-#{version}-x86_64.dmg"
  end

  name "Solarxy"
  desc "3D model viewer and validator (Rust + wgpu)"
  homepage "https://github.com/marko-koljancic/solarxy"

  app "Solarxy.app"
  binary "#{appdir}/Solarxy.app/Contents/MacOS/solarxy-cli"

  postflight do
    require "fileutils"
    system_command "/usr/bin/xattr",
                   args: ["-dr", "com.apple.quarantine", "#{appdir}/Solarxy.app"],
                   sudo: false

    marker_dir = "#{Dir.home}/Library/Application Support/Solarxy"
    FileUtils.mkdir_p(marker_dir)
    File.write("#{marker_dir}/install-source", "homebrew-cask\n")
  end

  uninstall delete: "#{appdir}/Solarxy.app"

  zap trash: [
    "~/Library/Application Support/Solarxy",
    "~/Library/Preferences/dev.koljam.solarxy.plist",
    "~/Library/Saved Application State/dev.koljam.solarxy.savedState",
    "~/.config/solarxy",
  ]
end

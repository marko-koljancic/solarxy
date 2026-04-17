# Solarxy CLI formula. Lives in the koljam/homebrew-solarxy tap.
#
# Usage:
#   brew install koljam/solarxy/solarxy-cli
#
# Cross-platform: macOS arm64 + macOS x86_64 + Linux x86_64 + Linux
# aarch64. Each variant downloads the cargo-dist-produced tarball that
# matches the host triple. Sha256 values are auto-bumped per release by
# .github/workflows/homebrew-bump.yml.

class SolarxyCli < Formula
  desc "Solarxy CLI: terminal companion to the Solarxy 3D model viewer"
  homepage "https://github.com/marko-koljancic/solarxy"
  version "0.5.0-rc.7"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/marko-koljancic/solarxy/releases/download/v#{version}/solarxy-cli-aarch64-apple-darwin.tar.xz"
      sha256 "REPLACE_WITH_AARCH64_DARWIN_SHA256"
    end
    on_intel do
      url "https://github.com/marko-koljancic/solarxy/releases/download/v#{version}/solarxy-cli-x86_64-apple-darwin.tar.xz"
      sha256 "REPLACE_WITH_X86_64_DARWIN_SHA256"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/marko-koljancic/solarxy/releases/download/v#{version}/solarxy-cli-x86_64-unknown-linux-gnu.tar.xz"
      sha256 "REPLACE_WITH_X86_64_LINUX_SHA256"
    end
    on_arm do
      url "https://github.com/marko-koljancic/solarxy/releases/download/v#{version}/solarxy-cli-aarch64-unknown-linux-gnu.tar.xz"
      sha256 "REPLACE_WITH_AARCH64_LINUX_SHA256"
    end
  end

  def install
    bin.install "solarxy-cli"

    # Write the install-source marker so `solarxy-cli --update` refuses
    # to run axoupdater (which would corrupt this brew-managed install).
    marker_dir = if OS.mac?
      "#{Dir.home}/Library/Application Support/Solarxy"
    else
      "#{ENV.fetch("XDG_DATA_HOME", "#{Dir.home}/.local/share")}/solarxy"
    end
    mkdir_p marker_dir
    (Pathname.new(marker_dir) / "install-source").write("homebrew-formula\n")
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/solarxy-cli --version")
  end
end

# Homebrew tap files

These two files belong in a separate tap repo —
[`koljam/homebrew-solarxy`](https://github.com/koljam/homebrew-solarxy) — not this
repo. They live here as the source of truth so changes to the tap go
through the same review process as the rest of the project, and the
`homebrew-bump.yml` workflow copies them into the tap repo on release.

## Files

- `Casks/solarxy.rb` — GUI installer. Distributes the `.dmg` that the
  native-bundle CI step produces. The cask's `postflight` block strips
  `com.apple.quarantine` automatically so users don't need to run
  `Install CLI.command` or do the System Settings dance.
- `Formula/solarxy-cli.rb` — cross-platform CLI installer (macOS
  arm64/x86_64 + Linux arm64/x86_64). Reads from the cargo-dist
  tarballs uploaded to GitHub Releases.

## One-time tap setup

1. Create a public GitHub repo `koljam/homebrew-solarxy` (the
   `homebrew-` prefix is required for `brew tap` to find it).
2. Copy the contents of this directory into the root of that repo:
   ```bash
   git clone git@github.com:koljam/homebrew-solarxy.git
   cd homebrew-solarxy
   cp -r ../solarxy/packaging/homebrew/Casks .
   cp -r ../solarxy/packaging/homebrew/Formula .
   git add Casks Formula
   git commit -m "initial Solarxy tap"
   git push
   ```
3. Verify with `brew tap koljam/solarxy && brew search solarxy`.

## Per-release maintenance

`.github/workflows/homebrew-bump.yml` runs on every GitHub release:
1. Downloads the new release artifacts and computes their SHA256.
2. Patches `version` and `sha256` in both files.
3. Pushes a commit to `koljam/homebrew-solarxy` (no PR — single-author
   tap, direct push is fine).

## Manual update

If the bump workflow fails:

```bash
cd homebrew-solarxy
sed -i '' "s/version \".*\"/version \"X.Y.Z\"/" Casks/solarxy.rb Formula/solarxy-cli.rb
# update sha256 values too
git commit -am "bump solarxy to X.Y.Z"
git push
```

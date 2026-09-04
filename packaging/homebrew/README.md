# Homebrew tap files

These two files belong in a separate tap repo,
[`marko-koljancic/homebrew-solarxy`](https://github.com/marko-koljancic/homebrew-solarxy), not this
repo. They live here as the reviewed copy, so a structural change to the cask
or the formula goes through the same review as the rest of the project, and so
the one-time tap setup below has something to copy from.

**They are not what ships, and nothing copies them automatically.**
`homebrew-bump.yml` checks out the tap repo into `tap/` and edits the files
*there* in place, seding `version` and each `sha256` from the release
artifacts. It never reads this directory. So:

- The `version` here tracks the release it was last refreshed for. The tap is
  authoritative for the live one.
- The hashes here are placeholders (`sha256 :no_check` in the cask,
  `REPLACE_WITH_*` in the formula). Real values only ever exist in the tap.
- A change to the *shape* of either file has to be carried into the tap by
  hand. Only version and hashes are automated.

## Files

- `Casks/solarxy.rb` — GUI installer. Distributes the `.dmg` that the
  native-bundle CI step produces. The cask's `postflight` block strips
  `com.apple.quarantine` automatically so users don't need to run
  `Install CLI.command` or do the System Settings dance.
- `Formula/solarxy-cli.rb` — cross-platform CLI installer (macOS
  arm64/x86_64 + Linux arm64/x86_64). Reads from the cargo-dist
  tarballs uploaded to GitHub Releases.

## One-time tap setup

1. Create a public GitHub repo `marko-koljancic/homebrew-solarxy` (the
   `homebrew-` prefix is required for `brew tap` to find it).
2. Copy the contents of this directory into the root of that repo:
   ```bash
   git clone git@github.com:marko-koljancic/homebrew-solarxy.git
   cd homebrew-solarxy
   cp -r ../solarxy/packaging/homebrew/Casks .
   cp -r ../solarxy/packaging/homebrew/Formula .
   git add Casks Formula
   git commit -m "initial Solarxy tap"
   git push
   ```
3. Verify with `brew tap marko-koljancic/solarxy && brew search solarxy`.

## Per-release maintenance

`.github/workflows/homebrew-bump.yml` runs on every GitHub release:
1. Downloads the new release artifacts and computes their SHA256.
2. Patches `version` and `sha256` in both files.
3. Pushes a commit to `marko-koljancic/homebrew-solarxy` (no PR — single-author
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

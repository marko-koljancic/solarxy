# Winget packaging

Submits Solarxy (the GUI binary) to the [Microsoft winget-pkgs](https://github.com/microsoft/winget-pkgs)
community repository so Windows users can install via `winget install Koljam.Solarxy`.

## Layout

```
manifests/k/Koljam/Solarxy/<version>/
├── Koljam.Solarxy.yaml              # version manifest (singleton)
├── Koljam.Solarxy.locale.en-US.yaml # default locale manifest
└── Koljam.Solarxy.installer.yaml   # installer manifest (per arch)
```

## Placeholders

`Koljam.Solarxy.installer.yaml` contains one release-time placeholder:

| Placeholder            | Source                                |
| ---------------------- | ------------------------------------- |
| `{{INSTALLER_SHA256}}` | SHA-256 of the produced MSI artifact. |

The `winget-release.yml` workflow performs this substitution on every
release tag.

`ProductCode` is intentionally omitted from the manifest: it is an
optional field, the WiX ProductCode rotates every build, and winget reads
it from the MSI itself at install time.

## Local validation

```powershell
# Windows 11+
winget validate manifests/k/Koljam/Solarxy/<version>
```

This will fail until `{{INSTALLER_SHA256}}` is substituted. For local
pre-flight validation, temporarily replace it with a valid throw-away
SHA-256, validate, then revert.

## UpgradeCode

UpgradeCode is stable across versions (so winget can recognize upgrades
correctly) and lives in the root `Cargo.toml` under `[package.metadata.wix]`:

```toml
upgrade-guid = "F201EA19-A29E-4B9E-A3CE-85CEB9BAF9CE"
```

Cargo-dist/cargo-wix rebuild the MSI with this UpgradeCode on every release.

## Submission

The bump workflow `.github/workflows/winget-release.yml` fires on each
non-prerelease GitHub Release, substitutes placeholders, and opens a PR
against `microsoft/winget-pkgs` via the `WINGET_RELEASE_PAT` secret.
Microsoft moderation review typically takes 3-7 days.

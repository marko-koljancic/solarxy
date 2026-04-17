# Winget submission

Three template manifests used by `.github/workflows/winget-release.yml`
to submit the Solarxy MSI to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs)
on every release.

## Files

- `Koljam.Solarxy.installer.yaml` — installer URL + SHA256 + ProductCode.
- `Koljam.Solarxy.locale.en-US.yaml` — name, description, tags, license.
- `Koljam.Solarxy.yaml` — version pointer.

`${VERSION}`, `${INSTALLER_SHA256}`, and `${RELEASE_DATE}` are substituted
at PR-creation time.

## Manual submission

If the auto-bump workflow fails, run [wingetcreate](https://github.com/microsoft/winget-create)
locally:

```powershell
wingetcreate update Koljam.Solarxy `
  --urls "https://github.com/marko-koljancic/solarxy/releases/download/vX.Y.Z/solarxy-X.Y.Z-x86_64-pc-windows-msvc.msi" `
  --version X.Y.Z `
  --submit
```

Wingetcreate fetches the MSI, computes the hash, and opens a PR. After
the first manual submission the auto-bump workflow takes over.

## ProductCode

The `ProductCode` GUID matches `wix/main.wxs`:
`F201EA19-A29E-4B9E-A3CE-85CEB9BAF9CE`. Do **not** change this — winget
uses it to detect already-installed versions for `winget upgrade`.

# Writ winget Manifests

This directory holds the winget manifests for Writ. End users install Writ on Windows via:

```powershell
winget install --id ibrahemid.Writ -e
```

## Launch state

The first submission of `ibrahemid.Writ` to `microsoft/winget-pkgs` is a **manual PR**, not automated. Until the upstream PR is merged and the public winget source replicates (typically 1-3 days after merge), `winget install --id ibrahemid.Writ -e` returns "No package found matching input criteria." Until then, point Windows users at the direct `Writ_<version>_x64_en-US.msi` download from GitHub Releases.

The `microsoft/winget-pkgs` validation bot also requires the real `ProductCode` GUID extracted from the MSI; the placeholder zero GUID in this directory will fail upstream validation. Extract the GUID manually (or extend `.github/workflows/packages.yml` to pull it via `lessmsi` / `msiexec`) before opening the PR.

## Layout

Manifests follow the upstream layout used by `microsoft/winget-pkgs`:

```
packaging/winget/manifests/i/ibrahemid/Writ/<version>/
  ibrahemid.Writ.yaml                 # version manifest
  ibrahemid.Writ.installer.yaml       # installer manifest (InstallerType wix, x64 MSI)
  ibrahemid.Writ.locale.en-US.yaml    # default locale manifest
```

Schema version: 1.6.0.

## Placeholders

The installer manifest uses placeholders that the post-release workflow rewrites:

- `__SHA256_MSI__` replaced with the SHA256 of the released `Writ_<version>_x64_en-US.msi`.
- `ProductCode` is set to a zero GUID until we extract the actual product code from the MSI. The post-release workflow can be extended to pull this from the MSI using `msiexec` or `lessmsi`. Until then, upstream validation will flag this field; correct it manually in the PR before merging into `microsoft/winget-pkgs`.

## Installer URL

The URL points at the GitHub Releases download for the x64 MSI:

```
https://github.com/ibrahemid/writ/releases/download/v<version>/Writ_<version>_x64_en-US.msi
```

Writ does not currently ship an ARM64 MSI. When an ARM64 artifact is added (post v0.1.0), append a second entry to the `Installers` list with `Architecture: arm64`.

## Local validation

Validate a manifest folder locally before submitting upstream. Requires Windows 10+ with winget installed:

```powershell
winget validate --manifest packaging\winget\manifests\i\ibrahemid\Writ\0.1.0
```

For a fuller check, run the upstream sandbox test from a clone of `microsoft/winget-pkgs`:

```powershell
.\Tools\SandboxTest.ps1 packaging\winget\manifests\i\ibrahemid\Writ\0.1.0
```

## Submitting to microsoft/winget-pkgs

1. Fork `https://github.com/microsoft/winget-pkgs`.
2. Copy the entire version folder:
   ```
   cp -R packaging/winget/manifests/i/ibrahemid/Writ/0.1.0 \
     <fork>/manifests/i/ibrahemid/Writ/0.1.0
   ```
3. Commit with the message required by the upstream bot: `New version: ibrahemid.Writ version 0.1.0`.
4. Push the branch and open a PR against `microsoft/winget-pkgs:master`.
5. The winget validation bot runs automatically. Fix any issues it reports (product code, installer switches, dependencies).
6. Once merged, `winget install ibrahemid.Writ` works from any Windows machine within 24 hours.

## Updating on a new release

The post-release workflow `.github/workflows/packages.yml` copies the `0.1.0` folder to the new version, rewrites `PackageVersion`, `InstallerUrl`, and `InstallerSha256`, and opens a PR in this repo with the bumps. After the PR merges, repeat the submission steps above against `microsoft/winget-pkgs` for the new version folder.

A follow-up improvement is to automate the upstream PR using `wingetcreate submit`, which requires a GitHub PAT with fork access stored as a repo secret.

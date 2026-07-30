# Writ winget Manifests

This directory holds the winget manifests for Writ. End users install Writ on Windows via:

```powershell
winget install --id ibrahemid.Writ -e
```

## Status

Versions 0.1.0 and 0.2.0 are merged into `microsoft/winget-pkgs`, so the command above installs Writ on any Windows machine. Every new version is another manual PR upstream; nothing in this repo submits it. A merged version takes 1-3 days to replicate to the public source.

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

Before a release fills it in, `InstallerSha256` holds the placeholder `__SHA256_MSI__`. The post-release workflow replaces it with the SHA256 of the released `Writ_<version>_x64_en-US.msi`.

## ProductCode

The 0.2.0 installer manifest carries the real `ProductCode` GUID, and the upstream 0.2.0 manifest carries the same one. Only the dead 0.1.0 folder still holds the zero placeholder `{00000000-0000-0000-0000-000000000000}`, so a grep across this directory can look as though the field was never filled in.

`scripts/packaging_bump_winget.py` rewrites `PackageVersion`, `InstallerUrl`, `InstallerSha256`, `ReleaseDate`, and `ReleaseNotesUrl`. It does not touch `ProductCode`, so a new version inherits the previous version's GUID.

Whether inheriting it is correct is unresolved: it depends on whether Tauri's WiX bundler mints a fresh ProductCode per version. Until that is settled, read the GUID out of the built MSI and paste it into the new manifest before opening the upstream PR. That is safe either way, and the upstream validation bot rejects a wrong one.

On Windows:

```powershell
lessmsi l -t Property Writ_<version>_x64_en-US.msi
```

On macOS or Linux, with msitools installed:

```bash
msiinfo export Writ_<version>_x64_en-US.msi Property
```

## Installer URL

The URL points at the GitHub Releases download for the x64 MSI:

```
https://github.com/ibrahemid/writ/releases/download/v<version>/Writ_<version>_x64_en-US.msi
```

Writ does not ship an ARM64 MSI. When an ARM64 artifact is added, append a second entry to the `Installers` list with `Architecture: arm64`.

## Local validation

Validate a manifest folder locally before submitting upstream. Requires Windows 10+ with winget installed:

```powershell
winget validate --manifest packaging\winget\manifests\i\ibrahemid\Writ\<version>
```

For a fuller check, run the upstream sandbox test from a clone of `microsoft/winget-pkgs`:

```powershell
.\Tools\SandboxTest.ps1 packaging\winget\manifests\i\ibrahemid\Writ\<version>
```

## Submitting to microsoft/winget-pkgs

1. Fork `https://github.com/microsoft/winget-pkgs`.
2. Copy the entire version folder:
   ```
   cp -R packaging/winget/manifests/i/ibrahemid/Writ/<version> \
     <fork>/manifests/i/ibrahemid/Writ/<version>
   ```
3. Commit with the message required by the upstream bot: `New version: ibrahemid.Writ version <version>`.
4. Push the branch and open a PR against `microsoft/winget-pkgs:master`.
5. The winget validation bot runs automatically. Fix any issues it reports (product code, installer switches, dependencies).
6. Once merged, `winget install ibrahemid.Writ` works from any Windows machine within 24 hours.

## Updating on a new release

The post-release workflow `.github/workflows/packages.yml` copies the highest existing version folder to the new version, rewrites `PackageVersion`, `InstallerUrl`, `InstallerSha256`, `ReleaseDate`, and `ReleaseNotesUrl`, and opens a PR in this repo with the bumps. After the PR merges, repeat the submission steps above against `microsoft/winget-pkgs` for the new version folder.

A follow-up improvement is to automate the upstream PR using `wingetcreate submit`, which requires a GitHub PAT with fork access stored as a repo secret.

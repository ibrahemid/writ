# ADR-059: Rename the app to Jotra

## Status

Proposed, 2026-11-18. Amends ADR-007, ADR-017, ADR-028, ADR-030, ADR-039, ADR-041 and ADR-052. Earlier ADRs stay as written.

## Context

Version 1.0.1 ships as Writ through direct dmg and pkg downloads, Homebrew, winget, direct MSI, deb, AppImage, the AUR (`writ-bin`) and `install.sh`. Every channel except the AUR takes updates from the in-app updater (ADR-007), which reads `https://github.com/ibrahemid/writ/releases/latest/download/latest.json`. Every 1.0.1 binary holds that URL.

The name reaches four kinds of state:

- **Operating system identity.** The bundle identifier `com.writ.editor` keys TCC grants, the keychain ACL on service `com.writ.ai`, LaunchServices handlers, the WebKit and WebView2 stores, Application Support and the single-instance lock. `productName` `Writ` names the macOS bundle folder, the login item, the MSI upgrade code, the Windows install folder, shortcuts and ProgIds, the deb package and the Linux desktop entry. Five exported UTIs `com.writ.*` hold per-type default-app choices.
- **Values third parties hold.** These include the `writ` command and its absolute paths in MCP client configs, hooks and scripts. Client configs and approval lists hold the MCP server name `writ` and the tool `open_in_writ`. Notes and bookmarks hold `writ://` links. Shell profiles and client configs hold `WRIT_*` variables. Homebrew, winget and the AUR hold package ids. The repository slug and the site host are also held.
- **Values on disk.**
  - `~/.writ` holds config, the index `writ.db`, the ADR-044 journal and version history, and logs.
  - The notes folder `~/Writ` is never written to config (ADR-039).
  - Config ids `writ-light`, `writ-dark` and `writ-blue` are stored in config. An unknown accent blocks config writes (ADR-048).
  - Minted names follow `writ-YYMMDD-HHMM` (ADR-041).
  - Chat history holds the `writ-proposal` fence.
  - Interrupted writes can leave files with the temp prefixes `.writ-tmp-`, `.writ-move-`, `.writ-rename-` and `.writ-install-`.
  - Three localStorage keys hold UI state.
- **Internal identifiers with no outside holder.** About 3,100 crate paths, 263 `--writ-` CSS properties, 26 event channels and the preview schemes.

Several platform mechanics constrain the design.

**macOS**
- tauri-plugin-updater swaps the new bundle into the existing path with `renamex_np`. It relaunches with `open -n` on the bundle root, so a 1.0.1 install updates into `/Applications/Writ.app` and keeps that folder name.
- tauri-utils `StartingBinary` refuses to start when any component of the executable path is a symlink.
- pkgbuild component bundles are relocatable before macOS 27 unless a component plist says otherwise. `BundlePreviousNames` exists only on 27 and later.

**Windows**
- The MSI upgrade code is uuid5 of `<productName>.exe.app.x64` unless `bundle.windows.wix.upgradeCode` is set. For `Writ` it is `ab97cc4d-eea3-53ee-90d8-3c2767967676`.
- Tauri's WiX template:
  - schedules `MajorUpgrade` after `InstallInitialize`, so `AppSearch` still sees the old product's registry values;
  - reads INSTALLDIR from `HKCU\Software\<publisher>\<productName>\InstallDir`;
  - names ProgIds `<productName>.<ext>`;
  - gives the Start Menu shortcut the bundle identifier as AUMID, while the app sets no process AUMID;
  - skips `RemoveShortcuts` during an upgrade, so the old product's shortcuts survive it.
- The updater runs `msiexec /i /passive /promptrestart AUTOLAUNCHAPP=True` and exits.

**Linux**
- The deb `Package` field is the kebab-cased productName.
- With `Conflicts` plus `Replaces`, dpkg removes `writ` in the same transaction. The bundler exposes `conflicts`, `replaces` and `provides`, and no `breaks`.
- The deb updater runs `pkexec dpkg -i` and relaunches the old executable path.
- The AppImage updater rewrites the file in place under its current name.
- `mimeapps.list` stores desktop ids and skips ids that no longer exist.

**GitHub and package channels**
- After a repository rename, GitHub redirects web, git and release-download traffic until a repository takes the old name.
- Pages project URLs are not redirected; custom domains are not affected.
- Attestation certificates keep the old repository name.
- Homebrew renames casks through `cask_renames.json` and migrates the Caskroom on `brew update`. An `auto_updates` cask upgrades only with `--greedy`.
- winget has no rename and correlates installs through `AppsAndFeaturesEntries`.
- The AUR has no rename, and AUR helpers do not apply `replaces`.

## Decision

### 1. The name is a parameter

The name is `Jotra`. The scripted pass (decision 13) takes it as `--name` and derives `Jotra` for display, `jotra` for identifiers and `JOTRA` for environment variables. Every new value in this record follows from that input, so a later change of name changes only the input.

### 2. Operating system identity stays

- **Kept values.** The bundle identifier `com.writ.editor` stays, along with:
  - the keychain service `com.writ.ai`;
  - the five UTIs `com.writ.rust-source`, `com.writ.jsx-source`, `com.writ.typescript-source`, `com.writ.go-source` and `com.writ.config-text`;
  - the identifier-derived single-instance id;
  - the CLI link signature `com.writ.editor/writ-command`;
  - the updater mechanism, its minisign key, the manifest name `latest.json` and its platform keys.
- **Renamed and pinned.** `productName` becomes `Jotra`. `bundle.windows.wix.upgradeCode` is pinned to `ab97cc4d-eea3-53ee-90d8-3c2767967676`.
- **Rejected: a new identifier.** It drops TCC grants, the keychain ACL, Finder default-app choices and WebKit storage, and the app cannot move TCC grants.
- **Rejected: an unpinned upgrade code.** It installs Jotra beside Writ, with two PATH entries.

### 3. Data folder: two lineages, never a move

One resolver in `jotra-core` replaces `resolve_writ_dir` in `src-tauri/src/state.rs` and `writ_dir` in `crates/writ-cli/src/main.rs`. The app, the CLI and the MCP server all call it. It returns the folder, the index path and a lineage, `Legacy` or `Current`, and applies these rules in order:

1. `JOTRA_DATA_DIR`, then `WRIT_DATA_DIR`. The folder is used as given, with lineage `Current`. Its index is `jotra.db` if that exists, else `writ.db` if that exists, else `jotra.db`.
2. `~/.jotra` with `Current` when `~/.jotra/jotra.db` exists.
3. `~/.writ` with `Legacy` when `~/.writ/writ.db` exists.
4. `~/.jotra` when that folder exists, else `~/.writ` when that folder exists. This covers an index deleted for a rebuild.
5. Otherwise `~/.jotra` is created, with `Current`.

The index inside `~/.writ` is always `writ.db`, and the index inside `~/.jotra` is always `jotra.db`.

**No move.** Nothing is moved, copied or renamed.

**Logs and the refusal.** New log and crash files are named `jotra.log`, `jotra-crash-*.log` and `jotra-crash-*.txt`. The startup report scanner and the log viewer read both prefixes. `NotesRootRefusal::HoldsWritData` refuses a notes folder that contains either data folder or either index.

**Reason, against ADR-028.** The folder holds the ADR-044 journal and version history, which are the only copy of earlier text. A move is a multi-file operation. It races MCP servers that clients start from the old command path, which hold the index and `write.lock`, and it breaks backup and exclude rules that name `~/.writ`. All it gains is a folder name. With both lineages, the rename performs no data operation.

**A never-refreshed older CLI copy** resolves `~/.writ/writ.db`. Such copies include a 1.0.1 `writ-cli` beside an AppImage, an AUR install still on 1.0.1, or a copied binary.
- On a `Legacy` machine that index is the live one, so the old copy reads and writes the same data. After a schema change it refuses with the ADR-017 message.
- On a machine whose only data is `~/.jotra`, it creates an empty `~/.writ` and sees no notes. Rule 2 still selects `~/.jotra` for the current app.

**Test.** A resolver table test over a temporary home covers each rule, both variable names, both index names inside an override, and both folders holding an index.

### 4. Notes folder default: never moves

`JOTRA_NOTES_DIR`, then `WRIT_NOTES_DIR`, then the configured folder take precedence as today. Without them, the default depends on the lineage:

| Case | Default |
|---|---|
| `Legacy` lineage | `~/Writ` |
| `Current` lineage | `~/Jotra` |
| Data-folder override | `<data>/Writ` when it exists, else `<data>/Jotra` |

- An override folder carries no name, so an existing `Writ` subfolder is the marker there.
- The default is never written to config, so ADR-039 holds.
- Deleting the data folder is a reset and yields the `Current` lineage.
- `cli_notes_default` calls the same function.

**Rejected: writing `~/Writ` into config on first launch.** That turns a derived value into a recorded user choice, against ADR-039.

**Rejected: testing whether `~/Writ` exists.** An unrelated `~/Writ`, or one left after a reset, would capture a fresh install.

**Test.** The resolver table covers:
- a legacy home;
- a current home;
- a fresh home with an unrelated `~/Writ`;
- an override with and without `Writ`;
- both variable names.

### 5. Values read under both names, written under the new one

| Value | Written | Read |
|---|---|---|
| User variables `DATA_DIR`, `NOTES_DIR`, `GUI_BIN`, `COMMAND`, `INSTALL_SKIP_SIGNATURE` | `JOTRA_*` in docs and generated wrappers | `JOTRA_*` first, then `WRIT_*` |
| Link scheme | `jotra://` | `jotra://` and `writ://`, both registered on every platform |
| Chat fence | `jotra-proposal` | both |
| Minted note stem | `jotra-%y%m%d-%H%M` | both prefixes count as minted |
| Temp and marker prefixes | `.jotra-tmp-`, `.jotra-move-`, `.jotra-rename-`, `.jotra-install-` | the watcher ignore set and crash recovery match both |
| Log and crash files | `jotra.*` | both |
| Activity action | `open_in_jotra` | both, mapped to one label |
| Preview tokens | `--jotra-preview-*` | the generated preview layer also declares each `--writ-preview-*` name as `var(--jotra-preview-*)`, because previewed HTML documents can read them |

- **Legacy modules.** Every old literal lives in `crates/jotra-core/src/legacy_names.rs` or `src/lib/legacy-names.ts`. No other source file spells the old name, apart from the allowlist in decision 13.
- **Renamed outright, with no fallback.** These have no holder outside the repository:
  - the development and CI variables `WRIT_UPDATER_ENDPOINT` and `WRIT_UPDATER_PUBKEY` (debug builds only), `WRIT_DEV_PORT` and `VITE_WRIT_PLATFORM`;
  - the internal schemes `writ-preview://`, `http://writ-preview.document`, `writ-note:` and `writ-workspace://`;
  - the `writ://*` IPC event channels.
- **Test.** Each reader has a case with legacy input, in files named `*legacy*`.

### 6. Config ids stay as written

- **Kept.** `writ-light`, `writ-dark` and `writ-blue` stay as the serialized values, in new configs too. Token preset files keep their id-based file names.
- **Labels.** Display labels use the new name.
- **Reason.** One config is read by builds of different versions, for example a stale CLI or a second machine through synced dotfiles. Any build that meets an unknown accent refuses config writes (ADR-048).
- **Test.**
  - A config round-trip test asserts the three serialized ids.
  - The banned-words tests exempt exactly these values.

### 7. The command and the MCP server

**Two names, one binary.** The command is built once from `jotra-cli` and ships as `jotra` and `writ` (`jotra.exe` and `writ.exe`). Release jobs copy that one build to both `externalBin` entries.
- Package-owned locations expose both names: the Windows INSTALLDIR on PATH, `/usr/bin` for deb and AUR, and `~/.local/bin` for `install.sh`.
- User-requested links, meaning macOS `/usr/local/bin` and the AppImage's `~/.local/bin` from Settings, create `jotra`. They keep an existing `writ` pointed at the current copy.
- `may_replace` and the signature check govern both names, and `is_writ_command_target` accepts both.

**Dialect by invoked name.** The binary reads the name it was invoked as: the variable `JOTRA_COMMAND_NAME` when it is set, then the file stem of `argv[0]`. Wrapper scripts that start the binary under another file name (`install.sh`'s `~/.local/bin/writ`, the AUR `/usr/bin/writ`) export `JOTRA_COMMAND_NAME=writ`. The stems `writ` and `writ-cli` select the legacy dialect; any other stem selects the current dialect.

| | Legacy dialect | Current dialect |
|---|---|---|
| `serverInfo.name` | `writ` | `jotra` |
| Listed tool | `open_in_writ` | `open_in_jotra` |
| Resource scheme | `writ-folder://` | `jotra-folder://` |

- Both dialects accept calls to either tool name and either resource scheme.
- Help and output text use the new name in both dialects.
- Reason: client approvals and permission lists key on server name and tool name, for example `mcp__writ__open_in_writ`. An existing registration therefore keeps both.

**Registrations**
- New registrations use the server name `jotra` and the `jotra` command path.
- Repair keeps the entry's existing server name and points the entry at the command whose file name equals that name, in the canonical install location. A repair never changes the dialect.
- Status reports an entry as connected when its name is `writ` or `jotra` and its command resolves to a copy that carries the CLI signature. That includes paths through the macOS compatibility link and through `C:\Program Files\Writ`.
- The probe client name becomes `jotra-connection-test`, and the server treats both probe names as probes.

**Claude Desktop bundle**
- The file is `Jotra.mcpb`, with `display_name` and `author` `Jotra` and the homepage on the new host.
- The manifest `name` stays `writ`, so an installed extension updates in place and keeps its tool settings.
- Its launcher reads `JOTRA_COMMAND`, then `WRIT_COMMAND`, and runs the `writ` command, matching the extension name.

**Opening the GUI from the CLI**
- On macOS the CLI runs `open -b com.writ.editor`, unchanged.
- Elsewhere it looks for the sibling `jotra-tauri`, then `writ-tauri`.

**Test.** Unit tests cover:
- dialect selection by `JOTRA_COMMAND_NAME` and by file stem;
- both tool names in both dialects;
- repair that keeps the name;
- status for each legacy path form;
- a snapshot of the mcpb manifest.

### 8. macOS bundle self-rename

**Where it runs.** The rename runs at the start of `main` in `src-tauri`, before the Tauri builder, in release builds only.

**The plan.** A pure function `plan_bundle_rename` in `jotra-core` decides what happens. Its input is a snapshot of:
- the running bundle path, identifier and version;
- the sibling `Jotra.app` or `Writ.app`, with its identifier and version;
- whether the path is translocated, on a read-only volume or inside a `Caskroom`;
- `JOTRA_SKIP_BUNDLE_RENAME`;
- the migration record (decision 12).

| Situation | Plan |
|---|---|
| The executable path has a symlink component | Open the canonical bundle and exit. Tauri refuses to start through a link. |
| Any guard set, the record says the rename already ran, or the record holds a decline for this version | Stay. |
| Running from `Writ.app` with identifier `com.writ.editor`, nothing at `Jotra.app` | Rename. |
| Running from `Writ.app`; `Jotra.app` has the same identifier and an equal or newer version | Open `Jotra.app` and exit. |
| Running from `Writ.app`; `Jotra.app` has the same identifier and an older version | Move `Jotra.app` to the Trash, then rename. |
| Running from `Jotra.app`; a real `Writ.app` has the same identifier and an equal or older version | Move `Writ.app` to the Trash and create the link. This covers a dmg drag beside an old install. |
| A different identifier at the sibling path | Stay. |

**Rename steps**
1. `rename(2)` `Writ.app` to `Jotra.app` in the same folder. When that folder is `/Applications` and not writable, run steps 1 and 2 in one admin osascript, the same mechanism the updater uses. A decline or failure is recorded with the version. The app then continues from `Writ.app` and asks again only for a later version.
2. Create the relative symlink `Writ.app -> Jotra.app` in the same folder and set `UF_HIDDEN` on the link with `lchflags`. Absolute paths held elsewhere keep resolving: `/Applications/Writ.app/Contents/MacOS/writ` in client configs, the `/usr/local/bin/writ` link, and scripts.
3. Call `LSRegisterURL` on `Jotra.app` with update.
4. Run `open -n -a <Jotra.app> --args <original arguments>` and exit with status 0. The process exits before Tauri starts, so the single-instance lock is never held twice.

**Per-user steps.** These are gated by the record and run on the first launch from `Jotra.app` for each user:
- **Login item.** When `~/Library/LaunchAgents/Writ.plist` exists, remove it. When `general.launch_at_login` is true, register `Jotra.plist` through the login item service.
- **Command links.** Re-point `/usr/local/bin/writ` at `Jotra.app/Contents/MacOS/writ` when it carries the signature, and add `/usr/local/bin/jotra`. Both happen only when the folder is writable without a prompt; otherwise Settings offers the install action.
- **Third-party client configs** are not edited.

**Failure behaviour.**
- Every step logs the operation name and its error.
- No step blocks launch.
- A failed rename leaves a working app at the old path.

**Rejected alternatives**
- A post-update hook in 1.0.1: that build is shipped.
- `UpdaterBuilder::executable_path`: it changes only builds that already carry the new name.
- Leaving the folder as `Writ.app`: Finder, Spotlight and Launchpad would show the old name for good.
- Rewriting client config paths: those files belong to other programs.

**Test**
- Unit tests of `plan_bundle_rename` cover each row and each guard.
- An executor test over a temporary folder with fake bundles checks the rename, the link, the hidden flag and the sibling removal through a trash trait.
- Real-Mac tests are listed in decision 16.

### 9. macOS packages

**dmg.** It contains `Jotra.app`. A drag beside an old `Writ.app` is resolved on first launch by decision 8.

**pkg `Jotra_<v>_universal.pkg`**
- **Identity.** The identifier stays `com.writ.editor`. The component is `Jotra.pkg` and the Distribution title is `Jotra`.
- **No relocation.** A component plist sets `BundleIsRelocatable=false`, so Installer never redirects the payload into a found `Writ.app`.
- **preinstall**
  - Quits the app by identifier with `tell application id "com.writ.editor"`.
  - Kills processes whose path contains `Writ.app/Contents/MacOS/` or `Jotra.app/Contents/MacOS/`.
  - When `/Applications/Writ.app` is a real bundle with identifier `com.writ.editor`, renames it to `Jotra.app` if that is absent, or removes it if `Jotra.app` is present.
- **postinstall**
  - Creates the hidden link when `Writ.app` is absent.
  - Opens `/Applications/Jotra.app` as the console user.
- **Test.** `scripts/test_build_mac_pkg.py` asserts the component plist, both process patterns and each preinstall branch.

**Cask**
- `Casks/jotra.rb` lives in the existing tap `ibrahemid/writ` and installs `Jotra_<v>_universal.pkg`, with `auto_updates true`.
- Uninstall uses `pkgutil: "com.writ.editor"` and `quit: "com.writ.editor"`, and deletes both `/Applications/Jotra.app` and `/Applications/Writ.app`.
- `zap` covers both name sets.
- `cask_renames.json` maps `writ` to `jotra`, and `Casks/writ.rb` is removed.

### 10. Windows MSI

**Product.** `productName` is `Jotra` and the upgrade code is pinned (decision 2). The major upgrade removes the `Writ` product and installs `Jotra`.

**Install folder.** The fragment `src-tauri/wix/legacy-install.wxs` sets INSTALLDIR before `CostFinalize` when the template's own search found nothing. It takes the value from `HKCU\Software\<publisher>\Writ\InstallDir`, or else from a file search for `[ProgramFiles64Folder]Writ\writ-tauri.exe`.
- Upgraded installs stay in their existing folder for good. By default that is `C:\Program Files\Writ`, and a custom folder is respected.
- Fresh installs use `C:\Program Files\Jotra`.
- The template records the folder under the new key, so later upgrades find it directly.
- Each fragment holds a component listed in `componentRefs`, which links the whole fragment, including its `SetProperty` and search entries.

**Executables in INSTALLDIR**
- `jotra-tauri.exe` is the GUI.
- `jotra.exe` and `writ.exe` are the command.
- `writ-tauri.exe` is a std-only launcher crate with the Windows subsystem. It starts the sibling `jotra-tauri.exe` with its arguments and exits, which keeps working whatever holds `writ-tauri.exe`: taskbar pins, the Run value, ProgId commands and scripts.

**AUMID.** The app calls `SetCurrentProcessExplicitAppUserModelID` with `com.writ.editor`, matching the Start Menu shortcut, so windows group with pins made from either shortcut.

**Shortcuts.** The fragment removes `Writ.lnk` from `ProgramMenuFolder` and `DesktopFolder` with `RemoveFile` at install, and the template creates `Jotra.lnk`. Taskbar pins belong to the user and stay.

**ProgIds.** The fragment `src-tauri/wix/legacy-progids.wxs` registers `Writ.<ext>` with the same open command as `Jotra.<ext>`. That keeps valid any `UserChoice` hash that names `Writ.<ext>`. `scripts/gen-legacy-progids.py` generates the fragment from the file associations in `tauri.conf.json`, and CI runs it with `--check`.

**URL protocols.** Both `jotra` and `writ` are registered.

**PATH.**
- A new component, `JotraCommandOnPath`, appends `[INSTALLDIR]`. It has a new GUID and the key path `HKCU\Software\Jotra\Command`.
- The old component goes with the old product: `WritCommandOnPath`, GUID `E48693D8-AA17-4D27-9EAB-F2A34A39862D`, with `HKCU\Software\Writ\Command` and its PATH part.

**Login item.** A per-user step moves `HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Writ` to `Run\Jotra` and copies its `StartupApproved\Run` state.

**Test**
- `scripts/test_wix_legacy.py` asserts the searches, the sequencing, the `RemoveFile` entries, ProgIds that match the config, and the new PATH GUID.
- A Rust test runs the Run-value migration over a registry trait.
- Windows VM tests are listed in decision 16.

### 11. Linux and the package channels

**deb `jotra`**
- `bundle.linux.deb.conflicts`, `replaces` and `provides` are each `["writ"]`, so dpkg removes `writ` during the in-app update.
- The package ships:
  - `/usr/bin/jotra-tauri`;
  - `/usr/bin/writ-tauri`, a package-owned symlink, because the updater relaunches that path and autostart entries hold it;
  - `/usr/bin/jotra` and `/usr/bin/writ`;
  - `/usr/share/applications/Jotra.desktop`;
  - `Writ.desktop` with `NoDisplay=true` and the same `MimeType` and `Exec`, so `mimeapps.list` entries that name it keep resolving.
- `DESKTOP_ENTRY_NAMES` adds `Jotra.desktop` and `jotra.desktop`. Set-default writes `Jotra.desktop`, and the default-app status accepts all four ids.

**Autostart.** A per-user step renames `~/.config/autostart/Writ.desktop` to `Jotra.desktop` with the new `Exec`, keeping `Hidden` and `X-GNOME-Autostart-enabled`.

**AppImage**
- The updater rewrites the image in place, so its file name stays: `Writ.AppImage` in `install.sh` layouts, or whatever name the user chose.
- A `writ.desktop` the app wrote keeps its id, with `Name` and `Exec` rewritten.
- On start, an AppImage build refreshes every command copy it made that carries the CLI signature and differs in version from the one in the running image. That covers `~/.local/bin/writ`, `~/.local/bin/jotra`, and `writ-cli` or `jotra-cli` beside the image.

**AUR**
- The package name stays `writ-bin`, with `provides=(jotra writ)` and `conflicts=(jotra writ)`.
- The payload goes to `/opt/jotra`, carrying `.package-manager`.
- It installs `/usr/bin/jotra`, `/usr/bin/writ`, the `/usr/bin/writ-tauri` link and the deb's two desktop entries.
- `pkgdesc` names Jotra, so AUR search finds the package.

**install.sh**
- It is published at the new slug, and the old raw URL redirects to it.
- When `~/.local/share/writ/Writ.AppImage` exists, it updates that layout in place and adds `~/.local/bin/jotra` beside `~/.local/bin/writ`.
- Otherwise it installs `~/.local/share/jotra/{Jotra.AppImage,jotra-cli,jotra-gui}` and both wrappers.
- The wrappers export `JOTRA_GUI_BIN`.

**winget**
- The id stays `ibrahemid.Writ`.
- The manifest takes `PackageName` `Jotra`, `Moniker` `jotra`, the tag `writ`, an `InstallerUrl` on the new slug, and `AppsAndFeaturesEntries` with `DisplayName` `Jotra` and `UpgradeCode` `{AB97CC4D-EEA3-53EE-90D8-3C2767967676}`.

**Test**
- A release check reads `dpkg-deb -c` for the link and both commands, and `dpkg-deb -f` for the relationship fields.
- A CI job on Ubuntu installs the 1.0.1 deb, then the new deb with `dpkg -i`. It asserts that `writ` is gone and that `writ` and `jotra` both run.
- `scripts/test_install_sh.py` covers both layouts.
- An Arch container builds `writ-bin`.

### 12. Per-user migration record and UI storage

**The record.** `name-migration.json` in the data folder records each per-user step:
- the bundle rename, done or declined with a version;
- the macOS login item;
- the command links;
- the Linux autostart entry;
- the Windows Run value.

Each step runs once and is idempotent. It logs failures with the step name and never blocks launch.

**localStorage.** The WebKit and WebView2 stores are keyed by the kept identifier, so the old keys stay readable.
- The inline script in `index.html` copies `writ-theme-vars-v3` to `jotra-theme-vars-v3`, rewriting `--writ-` property names to `--jotra-`, and deletes the old key.
- The stores copy `writ.editor.panes.v1` and `writ.tree.expanded.v1` to their `jotra.` names when the new key is absent, then delete the old keys.
- **Test.** Vitest covers each key, including the case where both keys exist and the new one wins.

### 13. Scripted pass

**Script.** `scripts/rename_product.py --name Jotra --stage app|channels [--check]` runs four steps in order:
1. The legacy modules (decision 5) and the alias files listed below exist first. They are written by hand, so the old literals they need are in place before the mechanical pass.
2. Paths are renamed: every `crates/writ-*` directory and package; `src-tauri` package `writ-tauri` to `jotra-tauri`, with lib `jotra_tauri_lib`; and file names that contain the name. Content is renamed after that.
3. The generators run.
4. The guard runs.

**Rename map.** It is case-preserving: `Writ` to `Jotra`, `writ` to `jotra`, `WRIT` to `JOTRA`. Matches:

```
(?<![A-Za-z])[Ww]rit(?![a-z])      words, kebab, snake, Pascal prefixes, `--writ-`
(?<=[a-z0-9])Writ(?![a-z])         camelCase interiors such as `isWritCommand`
(?<![A-Za-z])WRIT(?![A-Z])         environment and constant names
```

**Compounds** covered by the map:
- crate names in kebab and snake form;
- the `--writ-` CSS prefix;
- `writ://` event channels;
- `VITE_WRIT_`;
- in stage `channels` only, the slug `ibrahemid/writ` and the host `writ.ibrahemid.com`.

**Kept tokens.** These are masked before the scan, anywhere in the tree:
- `com.writ.editor`, `com.writ.ai` and the five `com.writ.*` UTIs;
- `writ-light`, `writ-dark` and `writ-blue`;
- `ibrahemid.Writ`, `writ-bin`, `homebrew-writ` and the tap name `ibrahemid/writ`;
- `ab97cc4d-eea3-53ee-90d8-3c2767967676`.

**Allowlist** (`scripts/old-name-allowlist.txt`). One path glob per line, each with its reason:
- **History**
  - `docs/adr/0[0-5][0-9]-*.md`, meaning ADRs up to and including this one;
  - the released sections of `CHANGELOG.md`: the guard skips everything from the first released version heading down;
  - `packaging/winget/manifests/i/ibrahemid/Writ/**`;
  - the site changelog data.
- **Legacy and alias files**
  - `crates/jotra-core/src/legacy_names.rs` and `src/lib/legacy-names.ts`;
  - `src-tauri/wix/legacy-*.wxs` and the deb `Writ.desktop` alias;
  - `packaging/aur/writ-bin/**` and `cask_renames.json`;
  - the cask's uninstall and zap lines;
  - the legacy branches of `install.sh` and `scripts/build-mac-pkg.sh`;
  - the `writ-tauri` launcher crate;
  - the `--writ-preview-*` alias source in the token build;
  - the legacy key in the `index.html` inline script.
- **Tests** named `*legacy*`.
- **Other**
  - third-party text: license notices and vendored files;
  - token preset files named by kept ids;
  - maintainer-local key names `~/.tauri/writ.key` and `writ-tauri-signing`;
  - the Azure signing account fixtures.

**Generated files**, regenerated by their generators and never hand-edited:

| Output | Generator |
|---|---|
| CSS tokens, including the preview layer | `pnpm tokens:build` |
| `src-tauri/gen/schemas` | tauri-build on `cargo build` |
| `Cargo.lock` | cargo |
| Third-party notices | `scripts/gen-third-party-notices.py` |
| `src-tauri/wix/legacy-progids.wxs` | `scripts/gen-legacy-progids.py` |
| `.SRCINFO` and the PKGBUILD version | `scripts/packaging_bump_aur.py` |
| winget manifests | `scripts/packaging_bump_winget.py` |
| Version fields | `bump_version.py` |
| `site/src/data/release.json` | the `site.yml` sync |
| The `preview-palette-adr-030.css` fixture | its fixture generator |
| Stills, loops and README GIFs | `scripts/capture/run.sh` |
| `og-1-0.png` | its source drawing, redrawn |

**Guard**
- `scripts/check_old_name.py` scans the paths and text of `git ls-files` with the three patterns after masking kept tokens. It fails on any hit outside the allowlist. `--stage app` also permits the slug and the old host; `--stage channels` does not.
- `scripts/test_check_old_name.py` covers:
  - matches;
  - non-matches such as `write`, `rewrite`, `writable` and `WRITE`;
  - masking;
  - globs;
  - stages.
- The guard runs as check `names` in `scripts/gate.sh` and as a step in `ci.yml`.
- The banned-words tests `src-tauri/tests/user_facing_strings_tests.rs` and `src/__tests__/architecture/banned-words.test.ts` add the old name for user-facing strings, exempting the legacy modules.

### 14. Repository, updater endpoint and site

**Repository and redirect**
- `ibrahemid/writ` is renamed to `ibrahemid/jotra`.
- 1.0.1 installs keep fetching the old endpoint through GitHub's redirect. `latest.json` builds its URLs from `github.repository`, so they name the new slug.
- No repository named `writ` is ever created under `ibrahemid`, since that would end the redirect for every 1.0.1 install.

**Endpoint and release**
- New builds read `https://github.com/ibrahemid/jotra/releases/latest/download/latest.json`.
- `release.yml` gates on `github.repository_id` instead of the slug, and `test_release_workflow.py` follows.
- Release assets are named `Jotra_<v>_*`; assets of earlier releases keep their names.
- Attestation docs verify with `--owner ibrahemid`, which covers assets from before and after the rename.

**Site**
- The site moves to `jotra.ibrahemid.com`. `writ.ibrahemid.com` answers every path with a 301 to the same path on `jotra.ibrahemid.com`.
- These move to the new host and slug:
  - Umami `data-domains`;
  - the title pattern in `llms-text.ts`;
  - the sitemap and structured data;
  - the URLs compiled into the app: docs and issues in `src/lib/help-links.ts`, the changelog in `src/stores/global/update.ts`, and the mcpb homepage.

### 15. Two changes, in order

**Change one: the app**
- **Contents.** It covers:
  - decisions 2 to 13;
  - packaging config: `tauri.conf.json`, the WiX fragments, the launcher crate, the deb config, the pkg script, `Casks/jotra.rb`, the PKGBUILD template and the `install.sh` logic;
  - the `release.yml` repository-id gate;
  - `README.md`, `docs/` and the text and identifiers of `site/`, which shares tokens and code with the app and cannot lag it.
- **Held back.** The slug, the endpoint, the host and channel publication keep their current values.
- **Guard.** It runs with `--stage app`.

**Change two: with the release**
- **Contents.** It covers:
  - the endpoint and slug;
  - the compiled URLs;
  - the site host and its redirect;
  - `cask_renames.json` and the tap push;
  - the winget manifest;
  - the AUR push;
  - the published `install.sh` URL.
- **Guard.** It runs with `--stage channels`.

**Sequence**
1. Merge change one.
2. Rename the repository.
3. Merge change two.
4. Tag the release.

### 16. Tests on real machines, before the release is published

**Mac**

Each Mac test runs from a 1.0.1 install, on macOS 12, 15 and 27.
- **mac-1.** In-app update in `/Applications`, admin-owned and user-owned. Expect the rename, the hidden link, the relaunch, one Dock icon and one instance.
- **mac-2.** mac-1 with App Management restrictions on macOS 15 and 27.
- **mac-3.**
  - A Dock pin.
  - Spotlight and Launchpad, with no entry for the link.
  - Finder "Open With" for `.md` and `.rs`.
- **mac-4.** TCC grants, and a read of a stored API key without a keychain prompt.
- **mac-5.**
  - The pkg over `Writ.app`, over `Jotra.app` and over both.
  - A dmg drag beside `Writ.app`.
- **mac-6.** Through the link:
  - `open /Applications/Writ.app`;
  - `open -b com.writ.editor`;
  - `/Applications/Writ.app/Contents/MacOS/writ mcp` with a registered client.
- **mac-7.** The login item: the old agent is gone, the new agent runs at login, and the background-item notice appears.
- **mac-8.** `brew update`, `brew upgrade --greedy` and `brew uninstall --zap` over a renamed install.
- **mac-9.** A document open as the first launch after the update.

**Windows**

The Windows test runs in a VM from a 1.0.1 MSI, with a default and a custom install folder. It checks:
- the Start Menu and desktop shortcuts;
- a taskbar pin and window grouping;
- `UserChoice` for `.md`;
- PATH, with `writ` and `jotra` in a new shell;
- INSTALLDIR retention;
- the Run value and `StartupApproved`;
- both URL protocols;
- `winget upgrade` correlation;
- the launcher.

**Linux**
- `xdg-mime query default text/markdown` on GNOME and KDE after a deb update.
- The autostart entry.
- An AppImage in-app update over an `install.sh` layout, with the command copies refreshed.

**Updater**

After the repository rename, a 1.0.1 install on each channel is offered the release and installs it.

### 17. Identity table

| Element | Old | New | Existing install holds | Migration or alias | Test |
|---|---|---|---|---|---|
| Bundle identifier | `com.writ.editor` | kept | TCC, keychain ACL, LaunchServices, WebKit store, Application Support | none | mac-4 |
| Keychain service | `com.writ.ai` | kept | API keys | none | mac-4 |
| Exported UTIs | five `com.writ.*` | kept | per-type Open With | none | mac-3 |
| Single-instance id | from identifier | kept | none | none | mac-1 |
| Updater key, manifest, platform keys | minisign key, `latest.json` | kept | baked into 1.0.1 | none | updater test |
| Updater endpoint | slug `ibrahemid/writ` | slug `ibrahemid/jotra` | URL in 1.0.1 | GitHub redirect; no new `writ` repository | updater test |
| macOS bundle folder | `Writ.app` | `Jotra.app` | absolute paths, Dock pins | self-rename, hidden link (8) | plan tests, mac-1 to mac-3, mac-6 |
| Main binary | `writ-tauri` | `jotra-tauri` | Windows pins, Run value, ProgId commands; deb relaunch path; autostart `Exec`; pkg process match | `writ-tauri.exe` launcher; `/usr/bin/writ-tauri` link; pkg matches both | launcher test, deb check, Windows VM |
| Command | `writ`, `writ.exe` | `jotra`, with `writ` kept | scripts, hooks, MCP command paths, PATH, links | ships under both names (7) | dialect tests, release asset check |
| CLI link signature | `com.writ.editor/writ-command` | kept | links and copies made by 1.0.1 | none | `cli_install` tests |
| Data folder | `~/.writ` | kept; `~/.jotra` on fresh installs | config, index, journal, versions, logs | lineage resolver (3) | resolver table |
| Index | `writ.db` | kept in `~/.writ`; `jotra.db` elsewhere | index | resolver | resolver table |
| Data variable | `WRIT_DATA_DIR` | `JOTRA_DATA_DIR` | shell profiles, client configs | both read | env tests |
| Notes default | `~/Writ` | kept; `~/Jotra` on fresh installs | notes | lineage (4) | resolver table |
| Notes variable | `WRIT_NOTES_DIR` | `JOTRA_NOTES_DIR` | shell profiles | both read | env tests |
| Logs and crash files | `writ.log`, `writ-crash-*` | `jotra.*` | earlier logs | readers accept both | scanner test |
| Lock files | `instance.lock`, `write.lock` | kept | none | none | none |
| Config ids | `writ-light`, `writ-dark`, `writ-blue` | kept; labels new | `config.toml` | none (6) | round-trip test |
| Minted names | `writ-YYMMDD-HHMM` | `jotra-YYMMDD-HHMM` | note files | both recognised | notes tests |
| Chat fence | `writ-proposal` | `jotra-proposal` | chat history | both parsed | proposals test |
| Temp prefixes | `.writ-tmp-`, `.writ-move-`, `.writ-rename-`, `.writ-install-` | `.jotra-*` | leftovers after a crash | watcher and recovery match both | workspace and move tests |
| localStorage keys | `writ-theme-vars-v3`, `writ.editor.panes.v1`, `writ.tree.expanded.v1` | `jotra` forms | WebKit and WebView2 store | copy, translate, delete (12) | vitest |
| Link scheme | `writ://` | `jotra://` | links in notes, bookmarks, scripts | both registered | deep-link config test, mac-6, Windows VM |
| Preview tokens | `--writ-preview-*` | `--jotra-preview-*` | HTML documents | alias declarations | token build test |
| Preview origin | `writ-preview://`, `http://writ-preview.document` | `jotra-preview` forms | document-script storage | none | CSP test |
| MCP server name | `writ` | `jotra` for new registrations | client configs, approval lists | repair keeps name; status accepts both (7) | `mcp_clients` tests |
| MCP server identity | `writ`, `open_in_writ`, `writ-folder://` | `jotra`, `open_in_jotra`, `jotra-folder://` | approvals, cached resource URIs | dialect by invoked name; calls accept both | server tests |
| Probe client | `writ-connection-test` | `jotra-connection-test` | none | server accepts both | server tests |
| Activity action | `open_in_writ` | `open_in_jotra` | activity rows | both mapped | activity test |
| Claude Desktop bundle | `Writ.mcpb`, name `writ`, `WRIT_COMMAND` | `Jotra.mcpb`, name `writ` kept, `JOTRA_COMMAND` | installed extension | launcher reads both | manifest snapshot |
| GUI variable | `WRIT_GUI_BIN` | `JOTRA_GUI_BIN` | `install.sh` and AUR wrappers | both read | CLI tests |
| Install variable | `WRIT_INSTALL_SKIP_SIGNATURE` | `JOTRA_INSTALL_SKIP_SIGNATURE` | CI scripts | both read | `test_install_sh.py` |
| Development variables | `WRIT_UPDATER_*`, `WRIT_DEV_PORT`, `VITE_WRIT_PLATFORM` | `JOTRA_*` | nothing outside the repository | none | guard |
| macOS login item | `LaunchAgents/Writ.plist` | `Jotra.plist` | agent at the old binary | per-user step | mac-7, unit test |
| Windows login item | `Run\Writ`, `StartupApproved` | `Run\Jotra` | registry | per-user step | registry test, Windows VM |
| Linux autostart | `autostart/Writ.desktop` | `Jotra.desktop` | autostart entry | per-user step | unit test |
| MSI upgrade code | derived `ab97cc4d-eea3-53ee-90d8-3c2767967676` | pinned, same value | installed product | pinned | config test |
| Windows install folder | `C:\Program Files\Writ` | kept on upgrade; `Jotra` when fresh | absolute paths, PATH | `legacy-install.wxs` | WiX test, Windows VM |
| Shortcuts | `Writ.lnk` | `Jotra.lnk` | Start Menu, desktop | `RemoveFile`; pins untouched | WiX test, Windows VM |
| ProgIds | `Writ.<ext>` | `Jotra.<ext>`; `Writ.<ext>` kept | `UserChoice` | `legacy-progids.wxs` | generator `--check`, Windows VM |
| Apps and Features entry | `Writ` | `Jotra` | uninstall entry | major upgrade | Windows VM |
| PATH component | `WritCommandOnPath`, `E48693D8-AA17-4D27-9EAB-F2A34A39862D`, `HKCU\Software\Writ\Command` | `JotraCommandOnPath`, new GUID, `HKCU\Software\Jotra\Command` | PATH entry | old removed with the old product | WiX test, Windows VM |
| AUMID | derived from the exe path | `com.writ.editor` explicit | pins | none | Windows VM |
| deb package | `writ` | `jotra` | dpkg database | `Conflicts`, `Replaces`, `Provides` | CI dpkg job |
| Desktop entries | `Writ.desktop`; app-written `writ.desktop` | `Jotra.desktop`; hidden `Writ.desktop`; `writ.desktop` kept | `mimeapps.list` | alias entry | Linux `xdg-mime` |
| Linux resources | `/usr/lib/Writ/` | `/usr/lib/Jotra/` | nothing outside the package | none | notices test |
| AppImage file | `Writ.AppImage` or the user's name | kept | launchers | updater writes in place | `test_install_sh.py` |
| install.sh layout | `~/.local/share/writ`, `writ-cli`, `writ-gui`, `~/.local/bin/writ` | kept when present; `jotra` layout when fresh | wrappers | legacy branch; command copy refresh | `test_install_sh.py` |
| pkg | `com.writ.editor`, `Writ.pkg` | identifier kept; `Jotra.pkg` | receipt | preinstall rename | `test_build_mac_pkg.py`, mac-5 |
| Cask | `ibrahemid/writ/writ` | `ibrahemid/writ/jotra` | Caskroom | `cask_renames.json` | mac-8 |
| winget | `ibrahemid.Writ`, `Writ`, `writ` | id kept; `Jotra`, `jotra` | winget correlation | `AppsAndFeaturesEntries` with the upgrade code | winget validate, Windows VM |
| AUR | `writ-bin` | kept | pacman database | `provides` | Arch container build |
| Repository slug | `ibrahemid/writ` | `ibrahemid/jotra` | clones, raw `install.sh` URL, URLs of earlier releases | GitHub redirect | `curl -IL` in change two |
| Release assets | `Writ_<v>_*` | `Jotra_<v>_*` | earlier releases | none | `release_assets.py` tests |
| Attestations | `--repo ibrahemid/writ` | `--owner ibrahemid` | issued certificates | docs | none |
| Site host | `writ.ibrahemid.com` | `jotra.ibrahemid.com` | links, bookmarks, search results | 301 per path | `curl -IL` per path |
| Compiled URLs | docs, issues, changelog, mcpb homepage | new host and slug | 1.0.1 binaries | host and repository redirects | help-links test |
| Signing | Apple team, Azure account, `~/.tauri/writ.key`, `writ-tauri-signing` | kept | none | none | none |
| Internal identifiers | crates, events, CSS, `writ-note:`, `writ-workspace://` | renamed | none | none | guard |

### 18. Channels

| Channel | What the user does | What the user sees |
|---|---|---|
| macOS in-app update (dmg, pkg or cask install) | Accepts the update | The app relaunches as `Jotra.app`, after one admin prompt when `/Applications` is not writable. Settings, keys, approvals and file associations carry over. |
| Direct dmg | Drags `Jotra.app` to Applications | First launch moves an older `Writ.app` to the Trash and leaves the hidden link. |
| Direct pkg | Runs `Jotra_<v>_universal.pkg` | The installer quits the app, renames or replaces `Writ.app`, installs `Jotra.app` and opens it. |
| Homebrew | Runs `brew update`; `brew upgrade --greedy` when wanted | The cask shows as `jotra`. `brew install --cask ibrahemid/writ/jotra` works for new users. |
| winget | Runs `winget upgrade ibrahemid.Writ` or accepts the in-app update | `Jotra` in Apps and Features, in the same folder, with the same shortcuts by their new name. |
| Direct MSI | Accepts the in-app update or runs the new MSI | One product named `Jotra` in the existing folder. PATH, pins, file types and both commands work. |
| deb | Accepts the in-app update or runs `apt install ./Jotra_<v>_amd64.deb` | The `writ` package is replaced by `jotra`. The menu shows Jotra, and both commands and file associations work. |
| AppImage | Accepts the in-app update | The same file now runs Jotra, and its command copies are refreshed. |
| AUR | Runs the usual AUR update of `writ-bin` | Jotra installed under `/opt/jotra`, with both commands. |
| install.sh | Re-runs the published command, or never re-runs it | A re-run updates the existing layout and adds `jotra`. Without a re-run, the in-app update applies. |

### 19. Permanent values

- `com.writ.editor`, `com.writ.ai` and the five `com.writ.*` UTIs.
- The upgrade code `ab97cc4d-eea3-53ee-90d8-3c2767967676` and the updater key.
- `writ-light`, `writ-dark` and `writ-blue`.
- The `writ` command, `writ://`, the `WRIT_*` variables in decision 5, and reading `writ-proposal` and `writ-` minted names.
- The legacy MCP dialect, and the mcpb name `writ`.
- `Writ.<ext>` ProgIds, `writ-tauri.exe` and `/usr/bin/writ-tauri`.
- `ibrahemid.Writ`, `writ-bin` and the tap `ibrahemid/writ`.
- No repository named `writ` under `ibrahemid`.
- The 301 from `writ.ibrahemid.com`.

## Consequences

- **Old folder names stay on upgraded installs.** Upgraded Windows installs keep `C:\Program Files\Writ` and a `writ-tauri.exe` launcher. Legacy installs keep `~/.writ` and `~/Writ`, and the docs name both locations.
- **The hidden `Writ.app` link carries third-party paths.** Removing it breaks absolute paths in third-party configs until a repair from Settings rewrites the MCP entries.
- **One document open can be lost.** When the first launch after the update is a document open, the rename exits before the open event arrives. The app opens without that document.
- **Preview storage starts empty.** Document-script storage under the preview origin starts empty after the update.
- **Some config files and integrations keep the old name.** Config files keep `writ-light`, `writ-dark` and `writ-blue`. Claude Desktop keeps the extension name `writ` and lists `open_in_writ`. Client configs created before the update keep the server name `writ`.
- **Legacy names are permanent code.** The legacy modules, WiX fragments, launcher crate and alias desktop entry have no removal date.
- **A vendored old `install.sh` fails.** A copy kept outside the repository resolves the new version and then fails on the asset name. The published URL serves the current script.
- **The guard enforces the rename.** A new spelling of the old name outside the allowlist fails `scripts/gate.sh` and CI.

### Open items

Each item is blocked on the external dependency named.

- **`jotra.ibrahemid.com`.** A DNS record for the subdomain at the DNS provider for `ibrahemid.com`, plus a host there that serves the per-path 301 from `writ.ibrahemid.com`. No domain is registered.
- **Repository rename.** An owner action on GitHub, between change one and change two.
- **winget.** Moderator review in `microsoft/winget-pkgs` of the manifest with the new `PackageName`, `Moniker` and `AppsAndFeaturesEntries`.
- **AUR.** The push of `writ-bin` needs the AUR SSH key on the maintainer machine.
- **Tap.** Write access for the push to `homebrew-writ`.
- **Test machines.** Real Macs on 12, 15 and 27 with 1.0.1 installs, a Windows VM, and GNOME and KDE desktops for decision 16.
- **Capture media.** An unlocked GUI session for `scripts/capture/run.sh`.
- **Updater check.** The check from a 1.0.1 install on each channel can run only after the repository rename.

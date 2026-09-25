<div align="center">

<img src="site/public/brand/icon-128.png" width="72" alt="">

# Writ

Writ is a text editor for macOS, Windows and Linux that opens any text file and searches every file in its folder. Markdown files render inline as you type, and connections, the graph, tags and chat are apps you switch on in Settings.

[**Download**](https://github.com/ibrahemid/writ/releases/latest) · [**Website**](https://writ.ibrahemid.com)

</div>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/media/hero-dark.gif">
  <img src="docs/media/hero-light.gif" alt="A Markdown file open in Writ, rendered inline, with the file tree in the sidebar" width="100%">
</picture>

## Install

```sh
brew trust ibrahemid/writ                                            # macOS 12 or later
brew install --cask ibrahemid/writ/writ
winget install -e --id ibrahemid.Writ                                # Windows
curl -fsSL https://github.com/ibrahemid/writ/raw/main/install.sh | sh # Linux
yay -S writ-bin                                                      # Arch
```

Or download a `.pkg` or `.dmg`, `.msi`, `.AppImage` or `.deb` from [Releases](https://github.com/ibrahemid/writ/releases/latest).

## The `writ` command

Install it from Settings, Advanced, Terminal command.

```sh
writ todo.txt README.md                  # open files
writ .                                   # open a folder
cargo test 2>&1 | writ --title results   # save piped text as a file and open it
writ new Groceries                       # create a file in the Writ folder and print its path
writ rename Groceries Shopping           # rename a file inside its folder
```

Every verb and its exit codes are in [docs/cli-verbs.md](docs/cli-verbs.md).

## Build from source

Prerequisites: Rust 1.89+, Node.js `^22.22.2 || ^24.15.0 || >=26`, pnpm 9+, and the [Tauri v2 prerequisites](https://tauri.app/start/prerequisites/) for your OS.

```bash
git clone https://github.com/ibrahemid/writ.git
cd writ
pnpm install
cargo build -p writ-cli
HOST_TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
cp target/debug/writ "src-tauri/binaries/writ-$HOST_TRIPLE"
pnpm tauri dev
```

On Windows PowerShell, after `pnpm install`:

```powershell
cargo build -p writ-cli
$hostTriple = (rustc -vV | Select-String '^host: ').Line -replace '^host: ', ''
Copy-Item target/debug/writ.exe "src-tauri/binaries/writ-$hostTriple.exe"
pnpm tauri dev
```

For a release build, build the sidecar with `cargo build -p writ-cli --release`, copy `target/release/writ` (or `writ.exe`) to the same `src-tauri/binaries/` name, and run `pnpm tauri build`. The installer or app bundle is written to `target/release/bundle/`.

[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) covers the system design and [docs/adr/](docs/adr/) the decision records.

## License

MIT. See [LICENSE](LICENSE).

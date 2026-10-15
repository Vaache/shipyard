# Shipyard for Linux

A native GNOME build of Shipyard: Rust, GTK 4 and libadwaita, with VTE for the
Claude Code terminal, WebKitGTK for conversations and READMEs, and GtkSourceView
for code. It talks to the same GitHub GraphQL/REST endpoints as the macOS app and
stores the token in the Secret Service keyring.

Baseline: Ubuntu 24.04 / GNOME 46 (GTK 4.14, libadwaita 1.5, VTE 0.76,
WebKitGTK 2.44). The Flatpak uses the GNOME 47 runtime.

## Install

Every CI run (and every `v*` release) produces three artifacts:

| File | Use |
| --- | --- |
| `shipyard_<version>_<arch>.deb` | Debian, Ubuntu 24.04+ and derivatives |
| `Shipyard-linux-x86_64.flatpak` | Any distribution with Flatpak |
| `Shipyard-linux-<arch>.tar.gz` | Portable build for distributions with GTK 4.14+ |

### .deb

```bash
sudo apt install ./shipyard_2.5.0_amd64.deb
```

### Flatpak

```bash
flatpak install --user Shipyard-linux-x86_64.flatpak
flatpak run app.shipyard.Shipyard
```

The Flatpak needs the GNOME 47 runtime from Flathub
(`flatpak remote-add --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo`).
It has access to your home folder so the Claude Code workspace can open your
projects, and it runs `git`, `gh` and `claude` on the host through `flatpak-spawn`.

### Tarball

```bash
tar xzf Shipyard-linux-x86_64.tar.gz
./Shipyard-linux-x86_64/install.sh          # installs to ~/.local (PREFIX=... to change)
./Shipyard-linux-x86_64/uninstall.sh
```

Runtime libraries for the tarball (Debian/Ubuntu names):
`libgtk-4-1 libadwaita-1-0 libvte-2.91-gtk4-0 libwebkitgtk-6.0-4 libgtksourceview-5-0 libssl3`.

Optional tools: [GitHub CLI](https://cli.github.com) for one-click sign-in and
[Claude Code](https://docs.claude.com/en/docs/claude-code) for the workspace.

## Updates

The app checks `releases/latest` every 6 hours (Preferences → Updates).
A tarball install in a folder you can write to downloads the new
`Shipyard-linux-<arch>.tar.gz`, swaps the binary and restarts. Package and Flatpak
installs open the release page instead.

## Build from source

```bash
sudo apt install build-essential pkg-config libssl-dev librsvg2-bin \
  libgtk-4-dev libadwaita-1-dev libvte-2.91-gtk4-dev libwebkitgtk-6.0-dev libgtksourceview-5-dev
curl https://sh.rustup.rs -sSf | sh       # Rust 1.92 or newer
cd linux
cargo build --release
./target/release/shipyard
```

Fedora: `gtk4-devel libadwaita-devel vte291-gtk4-devel webkitgtk6.0-devel gtksourceview5-devel openssl-devel`.
Arch: `gtk4 libadwaita vte4 webkitgtk-6.0 gtksourceview5`.

Packaging, the same steps CI runs:

```bash
./packaging/render.sh                      # desktop file, AppStream metadata, icons, Flatpak manifest → dist/
cargo install cargo-deb && cargo deb --no-build
flatpak-builder --user --install build-dir dist/flatpak/app.shipyard.Shipyard.yml
```

Checks:

```bash
cargo test
./target/release/shipyard --version
xvfb-run -a dbus-run-session ./target/release/shipyard --smoke      # builds every screen offline
SHIPYARD_TOKEN=$(gh auth token) ./target/release/shipyard --selftest  # read-only API test
```

`SHIPYARD_DEMO_REPO=owner/name` and `SHIPYARD_DEMO_FOLDER=/path` work as on macOS.
`SHIPYARD_DEBUG=1` logs skipped GraphQL nodes and enables the WebKit inspector.

## Renaming

The product name, app id, binary name, URL scheme, environment prefix and
GitHub owner/repo live in [`brand.env`](brand.env). `build.rs` turns it into
constants (`src/brand.rs`) and `packaging/render.sh` into the desktop file,
AppStream metadata, icon names and Flatpak manifest. The only other places are
the package and binary names in [`Cargo.toml`](Cargo.toml) (`name` under
`[package]`, `[[bin]]` and `[package.metadata.deb]`, plus the binary path in the
deb `assets`).

## Layout

```
src/
  api/        GitHub GraphQL + REST client, queries, models
  claude/     Claude Code: accounts, session history, chat protocol, git, workspace
  ui/         GTK views: lists, PR detail and diff, repos, projects, Claude workspace, settings
  app.rs      state, routing, polling, notifications, deep links
  brand.rs    product constants generated from brand.env
data/         desktop file and AppStream templates
packaging/    render, install and Flatpak templates
```

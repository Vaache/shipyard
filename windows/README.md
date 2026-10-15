# PullYard for Windows

A native Windows build of PullYard: C# / .NET 8, WinUI 3 (Windows App SDK 1.8), MVVM with CommunityToolkit.Mvvm. It is a port of the macOS app in `Sources/`, not a cross-platform wrapper.

## Install

Download from the [latest release](https://github.com/pullyard/pullyard/releases/latest):

- `PullYard-windows-x64-setup.exe` or `PullYard-windows-arm64-setup.exe`: per-user installer. No administrator rights needed. It adds a Start menu entry, registers `pullyard://` links and comes with an uninstaller (Settings → Apps).
- `PullYard-windows-<arch>-portable.zip`: unzip anywhere and run `PullYard.exe`.

The builds are not code-signed yet, so SmartScreen may warn on first launch: click **More info → Run anyway**.

**Requirements:** Windows 10 1809 or later (Windows 11 recommended for Mica), the Microsoft Edge WebView2 Runtime (preinstalled on Windows 11). Optional: [GitHub CLI](https://cli.github.com) for one-click sign-in, [Claude Code](https://docs.claude.com/en/docs/claude-code) for the Claude workspace, Git for Windows for the workspace's change view.

Installed copies update themselves: PullYard checks the latest GitHub release every 6 hours, downloads the installer for your architecture and restarts into the new version. The portable build opens the release page instead.

## What's in it

Everything from the macOS app, adapted to Windows:

| macOS | Windows |
| --- | --- |
| Sidebar, list and detail columns | `NavigationView` + resizable list column + detail pane with back navigation (Alt+Left) |
| PR conversation (`WKWebView`) | WebView2 with the same HTML renderer, links open inside the app |
| Diff table (`NSTableView`) | WebView2 diff with sticky file headers, lazy bodies for big files, inline threads, pending comments, "viewed" checkboxes |
| Review, merge, edit, reviewers, reply, job log, Fix with Claude sheets | `ContentDialog`s |
| Menu bar extra with review requests | Notification-area (tray) icon with the same menu content; closing the window keeps it running there (can be turned off in Settings) |
| Keychain | Windows Credential Manager |
| `UserDefaults` | `%LOCALAPPDATA%\PullYard\settings.json` |
| SwiftTerm terminal | ConPTY pseudo console + xterm.js in WebView2 (bundled, no CDN) |
| Native Claude chat | Same `stream-json` protocol; the transcript renders in WebView2 (markdown via bundled marked), composer is native with `/` and `@` completion, image paste and drag & drop |
| User notifications | `AppNotificationManager` |
| Pull to refresh | `RefreshContainer` + Ctrl+R / F5 |

Keyboard: Ctrl+R refresh, Ctrl+L open by link, Ctrl+1…6 sections, Ctrl+Shift+1…4 PR tabs, Ctrl+P go to file in a repository, Ctrl+, settings, Alt+Left back, Ctrl+Enter send a comment, Enter send in Claude chat (Shift+Enter for a new line), Shift+Tab cycle the Claude permission mode.

## Build

Needs Windows 10/11 with the .NET 8 SDK. Visual Studio 2022 (with the "Windows application development" workload) is optional: open `windows/src/PullYard/PullYard.csproj`.

```powershell
dotnet publish windows/src/PullYard/PullYard.csproj -c Release -r win-x64 -p:Platform=x64 -o windows/dist/x64/app
.\windows\dist\x64\app\PullYard.exe
```

For ARM64 use `-r win-arm64 -p:Platform=ARM64`. The output is self-contained (the .NET runtime and Windows App SDK are bundled), so it runs on machines without either installed.

Unit tests for the platform-independent core (API models, URL parsing, diff parsing, prompt building, updater logic):

```powershell
dotnet test windows/tests/PullYard.Core.Tests/PullYard.Core.Tests.csproj
```

Smoke checks the CI also runs:

```powershell
.\PullYard.exe --version      # prints the version
.\PullYard.exe --self-test    # offline checks: resources, web assets, parsers; exit code 0 on success
```

Set `PULLYARD_TOKEN` (the old `SHIPYARD_` names still work) before `--self-test` to also run the read-only GitHub API checks from the macOS `--selftest`. `PULLYARD_DEMO_REPO` and `PULLYARD_DEMO_FOLDER` work as on macOS. `PULLYARD_DEVTOOLS=1` enables the WebView2 developer tools.

### Installer

The installer is built with [Inno Setup 6](https://jrsoftware.org/isinfo.php) from `windows/installer/setup.iss`:

```powershell
iscc /DArch=x64 /DAppVersion=2.5.0 /DSourceDir=$PWD\windows\dist\x64\app /DOutputDir=$PWD\windows\dist\out windows\installer\PullYard.iss
```

### CI

`.github/workflows/windows.yml` runs the unit tests, publishes x64 and arm64, runs the smoke tests (version, self test including a ConPTY round trip, opening the window, silent install and uninstall), drives a short UI tour against the repository's own pull requests (`--smoke-tour`, screenshots uploaded as the `ui-tour-screenshots` artifact) and uploads `PullYard-windows-<arch>-setup.exe` and `PullYard-windows-<arch>-portable.zip` as artifacts. On a `v*` tag it also attaches them to the existing GitHub release for that tag (it does not create releases).

## Renaming

The product name (PullYard), app ID (`PullYard.PullYard`), executable name, URL scheme (`pullyard://`), environment variable prefix (`PULLYARD_`) and GitHub owner/repo (`pullyard/pullyard`) live in one file, `windows/Branding.props` (`Brand*` properties). The C# code reads them at runtime from assembly metadata (`PullYard.Core.AppInfo`), the CI passes them to the installer, and the updater uses them for the release URL and asset names. After a rename, also update the text in this README.

Settings, the token and WebView2 data are stored under the display name (`%LOCALAPPDATA%\<name>`, credential `<AppId>:github-token`), so a rename starts with a fresh profile.

## Layout

```
windows/
  Branding.props            name, app id, exe, owner/repo, version
  src/PullYard.Core/        platform-independent logic: GitHub API + models, HTML renderers, git, Claude (CLI launcher, sessions, chat protocol, ConPTY), updater
  src/PullYard/             WinUI 3 app: views, view models, services (credentials, tray, notifications, theme), Web/ (HTML/JS for WebView2)
  tests/PullYard.Core.Tests xUnit tests
  installer/setup.iss    Inno Setup script
  tools/                    icon packer, resource generator
```

Localized strings are in `src/PullYard/Strings/<language>/Resources.resw`. Code calls `L.T("Key", "English default")`; `python windows/tools/strings.py` regenerates the English file from those defaults (CI checks it is up to date). To add a language, copy `en-US` to another folder and translate the values.

The app icon is `src/PullYard/Assets/AppIcon.ico`, generated from `Resources/Brand/icon.svg` with `scripts/make_icon.swift` and `tools/make-ico.py`.

## Third-party code

- [xterm.js](https://github.com/xtermjs/xterm.js) and addons (MIT)
- [marked](https://github.com/markedjs/marked) (MIT)

See `src/PullYard/Web/vendor/THIRD-PARTY-NOTICES.txt`.

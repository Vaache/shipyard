<p align="center">
  <img src="Resources/Brand/icon-1024.png" width="128" height="128" alt="PullYard icon">
</p>

<h1 align="center">PullYard</h1>

<p align="center">
  <b>Pull requests, without the browser.</b><br>
  A tiny native GitHub client for macOS, Windows and Linux, with a built-in Claude Code workspace.
</p>

<p align="center">
  <b>Download for</b>
  <a href="https://github.com/pullyard/pullyard/releases/latest/download/PullYard.dmg"><b>macOS</b></a>
  ·
  <a href="#windows"><b>Windows</b></a>
  ·
  <a href="#linux"><b>Linux</b></a>
  ·
  <a href="https://pullyard.github.io/pullyard/">Website</a>
  ·
  <a href="https://github.com/pullyard/pullyard/releases">Releases</a>
</p>

<p align="center">
  <img src="docs/img/pr-files.webp" alt="PullYard showing the changed files of a pull request" width="900">
</p>

---

GitHub in a browser tab can take up a gigabyte of memory to show a single diff. PullYard is a small native app for everyday review: SwiftUI on macOS, WinUI 3 on Windows, GTK 4 and libadwaita on Linux. It starts fast, scrolls big diffs without stutter, and can pass review comments to Claude Code.

## Features

**Pull requests**
- Review requests, your own PRs and anything you're involved in, filtered by state
- "My PRs by project": your pull requests grouped by repository, in sections you can collapse
- Conversation, review threads, commits, checks and changed files
- Comment, approve, request changes and merge
- A diff viewer built on `NSTableView`, with file headers that stay pinned as you scroll
- Stacked pull requests: the PR header shows the whole stack, you can jump between PRs in it, and lists mark what each PR is stacked on
- Lists and the open PR refresh every 30 seconds (you can set 15 s to 15 min in Settings) and again when you switch back to the app. New pull requests show up at the top, briefly highlighted
- Pull to refresh on lists, boards and PR tabs: drag down with two fingers on the trackpad. `⌘R` does the same. The footer says when the list was last updated, and shows a Retry link if a refresh fails
- Re-request review in one click after you push. PullYard compares the reviewed commit with the branch head, so it notices rebases and force pushes too ("You pushed 3 commits after @sofia reviewed"). Outdated review threads don't count as unresolved

**Beyond pull requests**
- Notifications and issues
- Repositories with a file browser and syntax highlighting
- GitHub Projects boards
- A menu bar badge with the number of reviews waiting on you
- The PullYard theme in light and dark, or the standard macOS look
- Open any GitHub link inside the app (`⌘L`)
- Updates from inside the app. PullYard looks for a new release every 6 hours and lets you know with a notification and an "Update to …" item in the sidebar and menu bar. Installing it replaces the app and restarts it. You can also check by hand with **PullYard → Check for Updates…**

**Claude Code workspace**
- A native chat for the official Claude Code CLI, right next to the PR, its changed files and its branch
- **Fix with Claude Code**: one click starts a new session with the review feedback. If the reviewer left a "Fix prompt", PullYard uses it; otherwise it writes one from the comments
- Session history grouped by project, showing the branch and the PR for each session
- Change the model, effort and permission mode, approve tool calls in the chat, and see a diff for every edit
- Idle sessions go to sleep to free memory and pick up again on your next message
- An overlay with context and plan usage
- Several Claude accounts (say, personal and work), each with its own sign-in and history
- A plain terminal mode if you'd rather use the CLI as is

## Install

| Platform | Download | Requirements |
| --- | --- | --- |
| macOS | [`PullYard.dmg`](https://github.com/pullyard/pullyard/releases/latest/download/PullYard.dmg) (universal: Apple Silicon and Intel) | macOS 14 Sonoma or later |
| Windows x64 | [`PullYard-windows-x64-setup.exe`](https://github.com/pullyard/pullyard/releases/latest/download/PullYard-windows-x64-setup.exe) or [portable `.zip`](https://github.com/pullyard/pullyard/releases/latest/download/PullYard-windows-x64-portable.zip) | Windows 10 1809 or later, or Windows 11; WebView2 Runtime |
| Windows ARM64 | [`PullYard-windows-arm64-setup.exe`](https://github.com/pullyard/pullyard/releases/latest/download/PullYard-windows-arm64-setup.exe) or [portable `.zip`](https://github.com/pullyard/pullyard/releases/latest/download/PullYard-windows-arm64-portable.zip) | Windows 10 1809 or later, or Windows 11; WebView2 Runtime |
| Linux x86_64 | [`pullyard_<version>_amd64.deb`](https://github.com/pullyard/pullyard/releases/latest), [`PullYard-linux-x86_64.flatpak`](https://github.com/pullyard/pullyard/releases/latest/download/PullYard-linux-x86_64.flatpak) or [`PullYard-linux-x86_64.tar.gz`](https://github.com/pullyard/pullyard/releases/latest/download/PullYard-linux-x86_64.tar.gz) | GTK 4.14 and libadwaita (Ubuntu 24.04 or later), or any distribution with Flatpak |

Optional on every platform: [GitHub CLI](https://cli.github.com) for one-click sign-in, [Claude Code](https://docs.claude.com/en/docs/claude-code) for the Claude workspace.

### macOS

1. Download [`PullYard.dmg`](https://github.com/pullyard/pullyard/releases/latest/download/PullYard.dmg).
2. Open it and drag **PullYard** to Applications.
3. The app is not notarized yet, so macOS blocks the first launch. Open **System Settings → Privacy & Security → Open Anyway** (the DMG includes a *How to Install* guide), or run:

   ```bash
   xattr -dr com.apple.quarantine /Applications/PullYard.app
   ```

You only do this once. PullYard removes the quarantine flag from itself on launch, and later updates install from inside the app (**PullYard → Check for Updates…**), so there's no disk image to open again.

### Windows

1. Download the setup for your PC: `PullYard-windows-x64-setup.exe` or `PullYard-windows-arm64-setup.exe`. It installs for your user only, with no admin rights, adds a Start menu entry and registers `pullyard://` links.
2. The builds are not code-signed yet, so SmartScreen may warn on the first launch: click **More info → Run anyway**.

Installed copies update themselves: PullYard checks for a new release every 6 hours, downloads the installer and restarts. If you'd rather not install, unzip `PullYard-windows-<arch>-portable.zip` anywhere and run `PullYard.exe`; the portable build opens the release page when there's an update.

### Linux

Pick one:

```bash
sudo apt install ./pullyard_*.deb                     # Ubuntu 24.04+, Debian and derivatives
flatpak install --user PullYard-linux-x86_64.flatpak  # any distribution, uses the GNOME runtime from Flathub
tar xzf PullYard-linux-x86_64.tar.gz && ./PullYard-linux-x86_64/install.sh  # portable, installs to ~/.local
```

The tarball install updates itself the same way the Windows installer does. The `.deb` and Flatpak builds tell you about a new release and open the release page.

## Sign in to GitHub

Pick one on the welcome screen:

- **GitHub CLI**: uses the token from `gh auth login`.
- **Personal access token**: classic token with `repo`, `read:org`, `notifications`, `workflow`, and `read:project` / `project` for Projects.
- **Device Flow**: enter the Client ID of your own [GitHub OAuth App](https://github.com/settings/developers) with Device Flow enabled.

The token is stored in the macOS Keychain, Windows Credential Manager or the Secret Service keyring on Linux.

## Privacy

PullYard has no backend and no analytics. It talks to `api.github.com` directly and runs the `claude` CLI you installed. It never reads your Claude credentials; you sign in through `claude auth login` in your browser.

## Build from source

The Windows and Linux apps have their own build instructions in [`windows/README.md`](windows/README.md) and [`linux/README.md`](linux/README.md). The macOS app requires Xcode 15.3+ (Swift 5.10).

```bash
git clone https://github.com/pullyard/pullyard.git
cd pullyard
./scripts/build.sh            # builds build/PullYard.app
./scripts/build.sh --install  # builds and copies it to /Applications
```

Other scripts:

- `UNIVERSAL=1 ./scripts/build.sh`: universal binary (arm64 + x86_64)
- `./scripts/release.sh 2.1.0`: universal build packed into `build/PullYard-2.1.0.dmg` with a drag-to-Applications window (needs `pip3 install --user dmgbuild`; Finder applies the background so it also shows on macOS 26)
- `swift scripts/make_icon.swift Resources/Brand/icon.svg build/AppIcon.iconset`: regenerate the icon

The build is signed with the first "Apple Development" identity in your keychain, or ad-hoc if there is none. Set `SIGN_ID` to choose another one.

Read-only API self test:

```bash
swift build -c release
PULLYARD_TOKEN=$(gh auth token) .build/release/PullYard --selftest
```

For screenshots without personal data, set `PULLYARD_DEMO_REPO=owner/name` to fill every pull request and issue section from a public repository with a placeholder user, and `PULLYARD_DEMO_FOLDER=/path` to limit the Claude Code history to one folder.

### Project layout

```
Sources/PullYard/
  App/        app entry, routing, settings, deep links (pullyard://)
  API/        GitHub GraphQL + REST client and models
  Auth/       sign-in flows, Keychain
  Features/   PR, Lists, Issue, Repos, Projects, Notifications, MenuBar, Claude, ClaudeFix
  Shared/     diff table, HTML view, tab strip, localization, utilities
linux/        Linux app (Rust, GTK 4, libadwaita)
windows/      Windows app (C#, WinUI 3)
docs/         website (GitHub Pages)
```

## Acknowledgements

- The terminal mode uses [SwiftTerm](https://github.com/migueldeicaza/SwiftTerm) by Miguel de Icaza (MIT License).

## License

[MIT](LICENSE)

PullYard is an independent project and is not affiliated with or endorsed by GitHub, Inc. or Anthropic. GitHub and Claude are trademarks of their respective owners.

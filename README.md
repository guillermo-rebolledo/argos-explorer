# argos-explorer

`argos-explorer` is a read-only, Windows-first terminal workspace inspector built with Rust. It combines an expandable file tree, full-screen file previews, Git status and unified diffs, and a scalable fuzzy file finder.

## Features

- Navigate a Workspace with an expandable File Tree using the keyboard or mouse.
- Open text files in a full-screen, syntax-highlighted File Preview.
- Render Markdown headings, emphasis, lists, task markers, links, quotes, code blocks, rules, and tables as a styled preview instead of raw source.
- Page and search large files without loading the entire file into memory.
- Inspect conflicted, staged, unstaged, and untracked Git changes.
- View unified diffs, including binary, renamed, deleted, conflicted, and large untracked files.
- Find files quickly with the global Quick Open fuzzy finder.
- Open the Workspace Root in a new VS Code window when the `code` CLI is available.
- Automatically refresh after filesystem changes while preserving navigation state.
- Choose Nerd Font, emoji, or icon-free rendering.
- Use ANSI, high-contrast, monochrome, ASCII, and no-color modes.

`argos-explorer` never edits files or changes Git state.

## Requirements

- Windows 10 or Windows 11, x64.
- Windows Terminal, PowerShell, or the modern Windows console host.
- Git for Windows is optional. File browsing works without Git; Changes View and Diff View require it.
- VS Code is optional. The VSCode top-bar button appears only when the `code` CLI is available on `PATH`.
- Rust and Cargo are not required when using the packaged installer or portable ZIP.

## Installation

### Recommended: per-user installer

Distribute these files from `dist/`:

```text
argos-explorer-setup.exe
argos-explorer-setup.exe.sha256
```

Optionally verify the installer before running it:

```powershell
certutil -hashfile .\argos-explorer-setup.exe SHA256
```

Compare the displayed hash with `argos-explorer-setup.exe.sha256`, then double-click `argos-explorer-setup.exe`.

The installer:

- requires no administrator privileges;
- installs to `%LOCALAPPDATA%\Programs\argos-explorer`;
- adds that directory to the current user's `PATH`;
- checks that the installed executable starts correctly;
- detects whether Git for Windows is available; and
- installs a double-clickable uninstaller.

Open a new terminal after installation so it receives the updated `PATH`.

> The installer is not currently Authenticode-signed. Microsoft SmartScreen may display an "Unknown publisher" warning. Distribute the installer and checksum through a trusted channel.

### Portable ZIP

Extract `dist\argos-explorer-windows-x64.zip` to any directory and run:

```powershell
.\argos-explorer.exe C:\path\to\workspace
```

The portable package contains:

```text
argos-explorer.exe
config.example.toml
INSTALL-WINDOWS.txt
THIRD-PARTY-NOTICES.txt
```

### Build from source

Building requires Rust `1.97` or newer:

```powershell
cargo build --release
.\target\release\argos-explorer.exe .
```

To build both the installer and portable release archive:

```cmd
scripts\package.cmd
```

Only the computer producing the release needs Rust. Teammates installing the generated executable do not.

## Automated releases

Every pull request merged into `main` triggers the `Build merged PR release` GitHub Actions workflow. The workflow runs formatting checks, Clippy, and the complete test suite on `windows-latest`, then builds and publishes a traceable GitHub prerelease.

Download builds from the [GitHub Releases page](https://github.com/guillermo-rebolledo/argos-explorer/releases). Each automated release is tagged as `preview-v<version>-build-<run-number>-<commit>` and includes:

```text
argos-explorer-setup.exe
argos-explorer-setup.exe.sha256
argos-explorer-windows-x64.zip
argos-explorer.exe
```

Most users should download the installer and checksum. The portable ZIP and standalone executable are available for users who prefer not to install. Release builds can also be started manually from the repository's Actions tab through `workflow_dispatch`.

Automated builds are marked as prereleases. Stable releases are created manually through the `Publish stable release` workflow after the version in `Cargo.toml` is updated. Stable tags use `v<version>` and are immutable.

## Basic usage

Inspect the current directory:

```powershell
argos-explorer .
```

Inspect another directory:

```powershell
argos-explorer C:\src\my-workspace
```

The selected directory becomes the Workspace Root. Navigation cannot move above it. Non-Git directories remain fully usable; Changes View displays a clear non-repository state.

### Command-line options

```text
Usage: argos-explorer.exe [OPTIONS] [DIRECTORY] [COMMAND]

Arguments:
  [DIRECTORY]  Workspace root. Defaults to the current directory

Commands:
  update  Check for or install a newer GitHub release
  help    Print command help

Options:
      --config <FILE>       Read configuration from this TOML file
      --icons <ICONS>       nerd-font, emoji, or none
      --ascii               Disable Unicode box-drawing characters
      --no-color            Disable all color output
      --no-mouse            Disable mouse capture and navigation
      --log-level <FILTER>  Enable diagnostic logging
  -h, --help                Print help
  -V, --version             Print version
```

Supported environment variables:

```text
ARGOS_EXPLORER_CONFIG
ARGOS_EXPLORER_ICONS
ARGOS_EXPLORER_LOG_LEVEL
NO_COLOR
```

## Views

### Files View

Displays the Workspace as an expandable tree. Activating a file opens its full-screen File Preview. Returning restores the previous tree selection, expansion, and scroll state.

### File Preview

Displays text files with line numbers and syntax highlighting. Markdown files (`.md`, `.markdown`, `.mdown`, `.mkd`, and `.mkdn`) open as a styled semantic preview with wrapping enabled; `/` searches the rendered text. Large files use bounded, paged reads, so Markdown files above the configured small-file limit fall back to the paged text viewer. Binary files display metadata instead of writing binary content to the terminal.

### Changes View

Lists Git changes under Conflicts, Staged Changes, Unstaged Changes, and Untracked Files. A path can appear in both staged and unstaged groups when Git reports both states.

### Diff View

Displays the selected Change Entry as a full-screen unified diff with old and new line numbers and hunk navigation.

### Quick Open

Press `Ctrl+P` to search file paths throughout the Workspace. Results are published while indexing continues. Opening a result displays its File Preview; returning restores the query and selection.

### VS Code

When the `code` CLI is available on `PATH`, the top bar displays a `VSCode` button. Click it or press `Ctrl+O` to run `code --new-window <Workspace Root>`. The button is hidden when the CLI is unavailable.

## Keyboard and mouse controls

| Action | Controls |
| --- | --- |
| Move | Arrow keys or `j` / `k` |
| Activate file or directory | `Enter` or left-click |
| Expand | Right arrow or `l` |
| Collapse or move back | Left arrow or `h` |
| Files View | `Ctrl+1` or `1` |
| Changes View | `Ctrl+2` or `2` |
| Quick Open | `Ctrl+P` |
| Open Workspace Root in VS Code | `Ctrl+O` or click `VSCode` |
| Local filter or text search | `/` |
| Next or previous match | `n` / `p` |
| Next or previous diff hunk | `]` / `[` |
| Page | `PageDown` / `PageUp` |
| Beginning or end | `Home` / `End`, or `g` / `G` |
| Toggle line wrapping | `w` |
| Reload active screen | `r` |
| Full Workspace and Git refresh | `Ctrl+R` |
| Reveal or hide `.git` | `H` |
| Contextual help | `?` |
| Back or dismiss | `Esc` |
| Quit | `q` outside Quick Open; `Ctrl+C` always |

The mouse can activate files and directories, switch views, open diffs, launch VS Code, use Back, and scroll. Use `Shift+wheel` for horizontal scrolling where supported.

## Configuration

The default configuration file is:

```text
%APPDATA%\argos-explorer\config.toml
```

Copy `config.example.toml` there and change only the settings you need:

```toml
icons = "nerd-font" # "nerd-font", "emoji", or "none"
theme = "ansi"      # "ansi", "high-contrast", or "monochrome"
ascii = false
color = true
mouse = true
tab_width = 4
small_file_limit_mib = 8
page_cache_mib = 64
index_memory_mib = 512
```

Configuration precedence is:

1. command-line options;
2. environment variables;
3. `%APPDATA%\argos-explorer\config.toml`;
4. built-in defaults.

Unknown configuration keys produce an actionable error instead of being ignored.

## Icons and fonts

Nerd Font icons are enabled by default. Configure Windows Terminal to use a Nerd Font, or choose a fallback:

```powershell
argos-explorer --icons emoji .
argos-explorer --icons none .
```

Use `--ascii` for terminals without box-drawing support and `--no-color` or `NO_COLOR` when color is undesirable.

## Diagnostics

Diagnostic logging is disabled by default. Enable it with:

```powershell
argos-explorer --log-level debug .
```

Logs are written under:

```text
%LOCALAPPDATA%\argos-explorer\logs\argos-explorer.log
```

File contents and diff bodies are not written to diagnostic logs.

## Updating

Check the stable channel without installing:

```powershell
argos-explorer update --check
```

Install the latest stable release:

```powershell
argos-explorer update
```

Merged-PR builds are available through the opt-in preview channel:

```powershell
argos-explorer update --preview --check
argos-explorer update --preview
```

The updater downloads `argos-explorer-setup.exe` and its SHA-256 file from GitHub Releases, rejects insecure URLs and checksum mismatches, waits for the running process to exit, and then updates either an installer-managed or portable executable. Configuration is preserved.

To update a local source checkout instead:

```powershell
git pull --ff-only origin main
cargo test --locked
scripts\package.cmd
```

To publish a stable update, change the package version in `Cargo.toml`, merge that change to `main`, then run `Publish stable release` from the Actions tab with the same semantic version.

## Uninstalling

Double-click:

```text
%LOCALAPPDATA%\Programs\argos-explorer\argos-explorer-uninstall.exe
```

The uninstaller removes the executable and user `PATH` entry. Configuration under `%APPDATA%\argos-explorer` is preserved.

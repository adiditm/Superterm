# Superterm v0.1.1 release binaries

Built from commit `b56275b` (`main`) on **2026-11-20 16:30 WIB** by the
multi-arch CI workflow: <https://github.com/adiditm/Superterm/actions/runs/44711331906>

Every binary was produced with `cargo build --release` (`opt-level = "z"`,
LTO, `strip`, `panic = "abort"`) on a native runner for its platform.

| Platform | Architecture | File | Size |
|---|---|---|---|
| Windows | x86_64 (MSVC) | `windows-x86_64/superterm.exe` | 5.1 MB |
| Windows | ARM64 (MSVC) | `windows-arm64/superterm.exe` | 4.6 MB |
| Linux | x86_64 (glibc) | `linux-x86_64/superterm` | 8.9 MB |
| Linux | ARM64 (glibc) | `linux-arm64/superterm` | 7.5 MB |
| macOS | Intel x86_64 | `macos-x86_64/Superterm.app` | 5.1 MB |
| macOS | Apple Silicon arm64 | `macos-arm64/Superterm.app` | 4.4 MB |

Changes since v0.1.0: external clipboard copy/paste on Linux (Wayland and
X11), native Browse Folder / File dialogs for the vault path, and modal /
input width locking that stops the window from growing without bound.

Verify the downloads with:

```sh
shasum -a 256 -c SHA256SUMS.txt
```

## Running

- **Windows** — run `superterm.exe` directly (no installer).
- **Linux** — `chmod +x superterm && ./superterm`. Needs an X11/Wayland
  session; GUI libraries such as `libxkbcommon`, `libwayland-client` and
  `libxcb-*` are expected from the distribution.
- **macOS** — open `Superterm.app`. The build is unsigned (no Apple
  Developer ID), so the first launch needs **right click → Open**, or:

  ```sh
  xattr -dr com.apple.quarantine Superterm.app
  ```

  The x86_64 bundle also runs on Apple Silicon through Rosetta 2.

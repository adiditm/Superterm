# Superterm v0.1.2 release binaries

Built from commit `9b5b75c` (`main`) on **2026-11-20 17:05 WIB** by the
multi-arch CI workflow: <https://github.com/adiditm/Superterm/actions/runs/44716120053>

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

`v0.1.2` is a rebuild of the same source as `v0.1.1`; no behaviour change.
Features since `v0.1.0`:

- Sync: native **Browse Folder** and **File** dialogs for the export/import
  `.stvault` path (no more typing the path by hand).
- Clipboard: external copy/paste works on Linux (Wayland and X11).
- Sync: modal and input widths are locked so the window no longer grows
  without bound while moving the mouse.

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

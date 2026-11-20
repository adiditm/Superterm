# Superterm release binaries

Built from commit `5cceab1` (`main`) on **2026-11-20 15:45 WIB** by the
multi-arch CI workflow: <https://github.com/adiditm/Superterm/actions/runs/44705036264>

Every binary was produced with `cargo build --release` (`opt-level = "z"`,
LTO, `strip`, `panic = "abort"`) on a native runner for its platform.

| Platform | Architecture | File | Size |
|---|---|---|---|
| Windows | x86_64 (MSVC) | `windows-x86_64/superterm.exe` | 5.3 MB |
| Windows | ARM64 (MSVC) | `windows-arm64/superterm.exe` | 4.9 MB |
| Linux | x86_64 (glibc) | `linux-x86_64/superterm` | 8.4 MB |
| Linux | ARM64 (glibc) | `linux-arm64/superterm` | 7.2 MB |
| macOS | Intel x86_64 | `macos-x86_64/Superterm.app` | 5.3 MB |
| macOS | Apple Silicon arm64 | `macos-arm64/Superterm.app` | 4.6 MB |

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

  Universal binary option: copy the matching `Superterm.app` for the CPU you
  are on — the x86_64 bundle also runs on Apple Silicon through Rosetta 2.

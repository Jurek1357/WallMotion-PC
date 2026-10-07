# wallmotion-rs — long-term compiled port (experimental)

The Python app in `wallmotion/` stays as is. This folder grows the
**compiled successor**: one small native binary, no interpreter, no
PyInstaller bootloader — which is also what trips Windows Smart App
Control today.

## Plan (see `docs/STACK_ANALYSIS.md` §Option C)

| Step | Status |
|---|---|
| `core`: platform paths (XDG/Windows) + session detection, tested | ✅ done |
| `win`: WorkerW discovery + canvas create/z-order, tested (incl. live smoke test) | ✅ done |
| `demo`: animated test pattern behind desktop icons (`cargo run -p wallmotion-demo`) | ✅ done |
| `player`: mpv sidecar in our window (`--wid`) + JSON IPC pause/mute/volume, live-tested | ✅ done |
| Settings UI shell (Tauri or `egui`) reusing this core | next |
| Static `libmpv` link instead of the sidecar binary | later |

## Notes

- GUI binaries need an embedded manifest (DPI awareness + Win10+ OS
  support): without it, `GetSystemMetrics` reports virtualized sizes
  and layered `Progman` children fail with error 87. The demo embeds
  `crates/demo/app.manifest` via `embed-manifest` in `build.rs`.

## Layout

- `Cargo.toml` — workspace
- `crates/core` — `wallmotion-core`: `paths`, `session`, `VERSION`, unit tests

## Build & test

```bash
cargo test -p wallmotion-core
cargo clippy -p wallmotion-core -- -D warnings
```

Windows needs a linker: MSVC Build Tools, or MinGW-w64
(`winget install BrechtSanders.WinLibs.POSIX.UCRT`) with the
`stable-x86_64-pc-windows-gnu` toolchain.

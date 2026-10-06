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
| Video decode/render via `libmpv` (kills the H.264-only ceiling) | next |
| Linux backends (layer-shell / X11, same matrix as Python) | later |
| Settings UI shell (Tauri or `egui`) reusing this core | later |

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

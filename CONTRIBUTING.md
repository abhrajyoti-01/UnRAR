# Contributing to UnRAR

Thanks for your interest in UnRAR — an open-source, WinRAR-style archive
manager written in Rust. All contributions are welcome: bug reports,
features, docs, tests, and UI polish.

## Getting started

Requirements:

- Rust stable (via `rustup`) — MSVC target on Windows
- Visual Studio BuildTools (provides `link.exe` for the MSVC toolchain)

```bash
git clone https://github.com/abhrajyoti-01/UnRAR.git
cd UnRAR
cargo build            # debug build -> target/debug/UnRAR.exe
cargo build --release  # optimized -> target/release/UnRAR.exe
```

`build_exe.bat` additionally copies the binary to `dist\UnRAR.exe`.

## Project layout

| Path | Purpose |
|---|---|
| `src/engine.rs` | archive engine: format detection, list/extract/test/read/write, SFX |
| `src/app.rs` | egui application: menus, toolbar, table, dialogs, workers |
| `src/viewer.rs` | internal text/hex viewer window |
| `src/icons.rs` | embedded icon assets |
| `src/main.rs` | entry point: SFX self-extract -> CLI verbs -> GUI |
| `assets/` | committed icon PNGs (regenerate via `tools/gen_icons.py`) |

## Reporting issues / requesting features

Please open an issue and include:

- UnRAR version (`UnRAR.exe i` shows the build, About dialog shows 1.0)
- Steps to reproduce, expected vs actual behavior
- For archive problems: format, how it was created, and a minimal
  sample file if it can be shared
- Screenshots for UI issues

## Submitting pull requests

1. Fork the repo, create a feature branch (`git checkout -b fix/my-fix`).
2. Keep changes focused; match the existing code style
   (`cargo fmt --check`, `cargo clippy` clean where practical).
3. Verify the engine paths you touched still work:
   `UnRAR.exe l/t/x/a/i` on a ZIP and a RAR fixture.
4. Open a PR against `main` — describe *what* and *why*, and list what
   you tested.

## Scope notes

- **RAR creation is proprietary** — UnRAR reads RAR via the vendored
  UnRAR engine but will never write `.rar`. Archive-writing features
  should target ZIP/TAR.
- Keep the app dependency-light and native; avoid new heavyweight deps
  without discussion in an issue first.
- No code, resources, or artwork copied from WinRAR/RARLAB.

## License

By contributing you agree your work is licensed under the project's
MIT license.

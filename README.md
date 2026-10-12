# UnRAR

An open-source archive manager for Windows with a WinRAR-style workflow,
written in **Rust** (egui/eframe GUI). MIT licensed — contains no RARLAB code.

![License](https://img.shields.io/badge/license-MIT-blue)

## Features

- **File manager + archive browser** — opens into the filesystem like WinRAR;
  double-click an archive to descend into it, folders inside archives are
  navigable.
- **Signature-based format detection** (not extensions): RAR4/RAR5/REV, ZIP,
  7z, TAR, GZip, BZip2, XZ, Zstandard, CAB, SFX stubs — the same magic-byte
  checks WinRAR performs.
- **ZIP full support** — create/add/extract/test/delete, Store..Best levels,
  AES-256 encryption, archive comments, SFX builder.
- **RAR/RAR5** — list, extract, CRC test, member read — natively in-process
  via the vendored UnRAR engine (`unrar` crate). Multivolume `.rar/.r00`,
  `.rev`, solid/locked/recovery metadata reported in Info.
- **TAR/GZ/BZ2/XZ/ZST** — list/extract/read members.
- **7z** — list/extract/read members (LZMA/LZMA2 etc via `sevenz-rust`).
- **Internal viewer** — text + hex modes (double-click / Enter / View),
  no temp files.
- **Toolbar + menus mirroring WinRAR**: Add, Extract to, Test, View, Delete,
  Find, Comment, Lock, Scan (Windows Defender), SFX, Info, About.
- **Dialogs**: "Archive name and parameters" (format, level, AES password,
  comment, SFX), "Extraction path and options", archive information,
  comment editor, about.
- **Sortable columns**: Name / Size / Packed / Type / Modified / CRC32 /
  Ratio / Method. Encrypted entries shown in red.
- Drag & drop files into the window (open archives / add to ZIP),
  right-click context menu, Ctrl+A/F/I/O/T/E, Del, Enter, Backspace,
  find-as-you-type filter bar, recent-archives menu, two-pane status bar.

## Formats

| Format | List | Extract | Test | Create | Notes |
|---|---|---|---|---|---|
| ZIP | ✔ | ✔ | ✔ | ✔ | AES-256 encrypt/decrypt, comments, SFX |
| RAR/RAR5, .rev | ✔ | ✔ | ✔ | ✔ | read: vendored UnRAR (in-process); write: **built-in RAR4 store writer** (no deps) or `Rar.exe` for real compression when WinRAR is installed |
| 7z | ✔ | ✔ | ✔ | ✘ | via `sevenz-rust` |
| TAR/GZ/BZ2/XZ/ZST | ✔ | ✔ | ✔ | ✔ | native `tar` + codec writers |
| CAB | detect | — | — | — | listed as supported-only |

**RAR writing is now fully independent**: `RAR (store, built-in)` writes
spec-compliant RAR4 store archives in-process — validated byte-level
against unrar's own header parser and tested by RARLAB's `UnRAR.exe`
(All OK). No compression (members stored raw) since the compressor is
proprietary. The `RAR (via WinRAR)` option additionally offers real
compression (`-m0..-m5`, `-p`, `-s`, `-rr3`, `-k`, `-md`, `-v`) when an
installed `Rar.exe` is found.

## Build

Requirements: Rust toolchain (`rustup`, MSVC target) + Visual Studio
BuildTools linker.

```bat
build_exe.bat        rem cargo build --release -> dist\UnRAR.exe
```

or:

```bat
cargo build --release
target\release\UnRAR.exe
```

## Command line (same exe)

`UnRAR.exe` also works from a terminal (attaches to the parent console):

```
UnRAR.exe l <archive>          list
UnRAR.exe t <archive>          test (CRC verify)
UnRAR.exe x <archive> [dest]   extract all
UnRAR.exe a <zip> <files...>   add to zip
UnRAR.exe i <archive>          info
UnRAR.exe <archive>            open GUI on archive
```

## Self-extracting archives

"Create SFX" appends a ZIP to a copy of `UnRAR.exe`. Running the result
prompts for a destination and extracts itself — no UnRAR install needed
on the target machine.

## Layout

```
src/engine.rs   archive engine (detect/list/extract/test/read/write/SFX)
src/app.rs      egui application — menus, toolbar, table, dialogs, workers
src/viewer.rs   internal viewer window (text + hex)
src/icons.rs    embedded icon assets
src/main.rs     entry point: SFX check -> CLI -> GUI
assets/         generated pixel-art icons (tools/gen_icons.py)
unrar.rc        Windows version + icon resources (build.rs/embed-resource)
```

## License

MIT — see `LICENSE`. `unrar` crate vendors the UnRAR source
(usage terms apply to the RAR format; this project contains no
RARLAB-authored Rust code).

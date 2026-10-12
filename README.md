# UnRAR

A WinRAR-style archive manager for Windows — GUI + CLI, released as
**open source under the MIT license**.

Ships as two executables built from the *same* source tree:

- **`UnRAR.exe`** — graphical archive manager (windowed)
- **`unrar-cli.exe`** — console tool with real-unrar-style commands
  (named `unrar-cli` because Windows is case-insensitive: `unrar.exe`
  would overwrite `UnRAR.exe`)

Prebuilt binaries live in [`dist/`](dist/). No Python required to run them.

## Features

| Action | ZIP | RAR / .rev / volumes | TAR/GZ/BZ2/XZ | 7z / CAB / ZST |
|--------|-----|----------------------|----------------|-----------------|
| List   | ✔ native | ✔ via UnRAR.exe backend | ✔ native | detected, needs external tool |
| Extract| ✔ native (incl. ZipCrypto) | ✔ via UnRAR.exe backend | ✔ native | — |
| Test   | ✔ CRC32 verify | ✔ via backend | ✔ stream-verify | — |
| Create / Add / Delete | ✔ ZIP (Store–Best) | ✖ proprietary | — | — |

- **Content-signature format detection** — reads magic bytes
  (`Rar!\x1a\x07`, `Rar!\x1aRev`, `PK\x03\x04`, `ustar`, …), not extensions.
- RAR reading delegates to the freeware `UnRAR.exe` backend — found
  automatically in `C:\Program Files\WinRAR`, on `PATH`, next to the app, or
  via the `UNRAR_EXE` environment variable.
- GUI: sortable Name/Size/Packed/Ratio/Modified/CRC32/Method columns,
  Add / Extract-to / Test / View / Delete / Info, keyboard shortcuts,
  drag-free file-open, threaded operations with a status bar.

## Run the executables

```console
UnRAR.exe                  :: GUI
UnRAR.exe archive.zip      :: GUI opens an archive directly

unrar-cli.exe                  :: usage (like real unrar)
unrar-cli.exe l archive.zip    :: list
unrar-cli.exe t archive.zip    :: test CRCs
unrar-cli.exe x a.rar outdir   :: extract all
unrar-cli.exe x a.zip f.txt    :: extract one member
unrar-cli.exe a new.zip f dir  :: create/add to ZIP
unrar-cli.exe i archive.rar    :: info
unrar-cli.exe gui archive.zip  :: launch the GUI from the CLI exe
```

## Run from source

```console
python unrar.py gui archive.zip
python unrar.py l archive.zip
```

## Build the executables

```console
python -m pip install -r requirements-build.txt
build_exe.bat
```

Produces `dist\UnRAR.exe` (windowed) and `dist\unrar.exe` (console) with
embedded version info.

## Source layout

- `unrar.py` — CLI + entry point
- `unrar_gui.py` — Tkinter GUI (`UnRARApp`)
- `unrar_engine.py` — signature detection + archive operations
- `build_exe.bat`, `version.txt`, `requirements-build.txt` — build tooling
- `UnRAR.spec`, `unrar.spec` — generated PyInstaller specs

## Limitations

RAR *creation* is proprietary — only extraction is possible without
RARLAB's licensed encoder, so UnRAR writes standard ZIP. WinZip-AES zips,
7z, and CAB are detected but need their own backends.

## License

MIT — see [LICENSE](LICENSE). Contains no RARLAB code; the optional
`UnRAR.exe` backend is distributed separately by RARLAB under its own
freeware license.

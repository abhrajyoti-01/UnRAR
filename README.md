# UnRAR

A WinRAR-style archive manager for Windows — GUI + CLI, released as
**open source under the MIT license**.

Ships as a single executable:

- **`UnRAR.exe`** — graphical archive manager (windowed), prebuilt in
  [`dist/`](dist/). No Python required.

A console interface with real-unrar-style commands is available from
source: `python unrar.py l|t|x|a|i <args>` (not shipped as an exe).

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
- GUI: sortable Name/Size/Packed/Type/Modified/CRC32/Ratio/Method columns,
  Add / Extract-to / Test / Delete / Info toolbar, Ctrl+F find bar,
  right-click menu, ZIP comment editor, archive lock, recent archives.
- **Internal viewer** — double-click or Enter opens a member directly in a
  viewer window (text auto-detected; hex mode with offset/ASCII dump;
  toggle Text/Hex). No temp extraction or external app needed.

## Run

```console
UnRAR.exe                  :: GUI
UnRAR.exe archive.zip      :: GUI opens an archive directly
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

Produces `dist\UnRAR.exe` (windowed) with embedded version info.

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

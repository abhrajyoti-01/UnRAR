#!/usr/bin/env python3
"""UnRAR — command line entry point.

Usage:
  unrar.py                     open the GUI
  unrar.py gui [archive]       open the GUI (optionally on an archive)
  unrar.py l <archive>         list contents
  unrar.py t <archive>         test integrity
  unrar.py x <archive> [dest]  extract (dest defaults to .\<name>_extracted)
  unrar.py x <archive> file..  extract selected members to cwd
  unrar.py a <archive.zip> <files..>  add files/folders to a ZIP
  unrar.py i <archive>         archive information
"""

from __future__ import annotations

import sys
from pathlib import Path

import unrar_engine as eng
from unrar_gui import fmt_size, fmt_time

USAGE = __doc__


def _need(argv, n=3):
    if len(argv) < n:
        print(USAGE)
        sys.exit(2)


def cmd_list(path: str) -> int:
    info = eng.list_archive(path)
    print(f"{info.format.upper()}  {path}")
    print(f"{'Name':<48} {'Size':>12} {'Packed':>12} {'Ratio':>6} "
          f"{'Modified':<16} {'CRC':>8}  Method")
    print("-" * 110)
    for e in info.entries:
        crc = f"{e.crc:08X}" if e.crc is not None else ""
        size = "<DIR>" if e.is_dir else fmt_size(e.size)
        name = e.name if len(e.name) <= 48 else "…" + e.name[-47:]
        print(f"{name:<48} {size:>12} {'' if e.is_dir else fmt_size(e.packed):>12} "
              f"{e.ratio:>6} {fmt_time(e.mtime):<16} {crc:>8}  {e.method}")
    print(f"\n{len(info.entries)} entries")
    if info.needs_backend:
        print(f"note: {info.needs_backend} contents need an external tool")
    return 0


def cmd_test(path: str) -> int:
    ok, detail = eng.test_archive(path)
    print(detail)
    return 0 if ok else 1


def cmd_extract(argv: list[str]) -> int:
    path = argv[2]
    rest = argv[3:]
    members = None
    if rest:
        # Trailing arg that is not an archive member name = destination dir.
        try:
            names = {e.name for e in eng.list_archive(path).entries}
        except Exception:
            names = set()
        if rest[-1] not in names:
            dest, members = rest[-1], rest[:-1] or None
        else:
            dest, members = f".\\{Path(path).stem}_extracted", rest
    else:
        dest = f".\\{Path(path).stem}_extracted"
    out = eng.extract_archive(path, dest, members)
    print(f"Extracted {len(out)} item(s) -> {dest}")
    return 0


def cmd_add(argv: list[str]) -> int:
    _need(argv, 4)
    n = eng.add_to_zip(argv[2], argv[3:])
    print(f"Added {n} file(s) to {argv[2]}")
    return 0


def cmd_info(path: str) -> int:
    info = eng.list_archive(path)
    total = sum(e.size for e in info.entries)
    packed = sum(e.packed for e in info.entries)
    print(f"Archive:  {info.path}")
    print(f"Format:   {info.format.upper()}")
    print(f"Entries:  {len(info.entries)}")
    print(f"Unpacked: {fmt_size(total)}")
    print(f"Packed:   {fmt_size(packed)}")
    if packed and total:
        print(f"Ratio:    {100 - packed * 100 // total}%")
    if info.comment:
        print(f"Comment:  {info.comment}")
    return 0


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print(USAGE)
        return 0
    if argv[1] == "gui":
        import unrar_gui
        return unrar_gui.main(argv[1:])
    cmd = argv[1].lower()
    try:
        if cmd in ("l", "list"):
            _need(argv)
            return cmd_list(argv[2])
        if cmd in ("t", "test"):
            _need(argv)
            return cmd_test(argv[2])
        if cmd in ("x", "e", "extract"):
            _need(argv)
            return cmd_extract(argv)
        if cmd in ("a", "add"):
            return cmd_add(argv)
        if cmd in ("i", "info"):
            _need(argv)
            return cmd_info(argv[2])
        # No command letter -> treat as archive to open in the GUI
        if Path(argv[1]).exists():
            import unrar_gui
            return unrar_gui.main(["gui", argv[1]])
        print(USAGE)
        return 2
    except eng.EngineError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))

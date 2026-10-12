"""UnRAR archive engine.

Signature-based format detection (same approach WinRAR uses: magic bytes, not
extensions) and archive operations for ZIP / TAR-family natively, and RAR /
REV volumes through the freeware UnRAR.exe backend when it is installed.
"""

from __future__ import annotations

import os
import re
import shutil
import stat
import subprocess
import tarfile
import zipfile
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

# --- format detection -------------------------------------------------------

_RAR4_SIG = b"Rar!\x1a\x07\x00"
_RAR5_SIG = b"Rar!\x1a\x07\x01\x00"
_REV_SIG = b"Rar!\x1aRev"          # RAR recovery volume (.rev)
_ZIP_LOC = b"PK\x03\x04"
_ZIP_EOCD = b"PK\x05\x06"
_ZIP_SPAN = b"PK\x07\x08"
_7Z_SIG = b"7z\xbc\xaf\x27\x1c"
_XZ_SIG = b"\xfd7zXZ\x00"
_GZ_SIG = b"\x1f\x8b"
_BZ2_SIG = b"BZh"
_ZST_SIG = b"\x28\xb5\x2f\xfd"
_CAB_SIG = b"MSCF"
_USTAR_MAGIC = b"ustar"

ZIP_METHODS = {
    0: "Store", 8: "Deflate", 9: "Deflate64", 12: "BZIP2", 14: "LZMA",
    93: "Zstandard", 95: "XZ", 96: "JPEG", 97: "WavPack", 98: "PPMd",
    99: "WinZip AES",
}

_COMPRESSLEVELS = {           # WinRAR-style names -> zlib level
    "Store": 0, "Fastest": 1, "Fast": 3, "Normal": 6, "Good": 7, "Best": 9,
}

RAR_EXTS = {".rar", ".rev", ".r00", ".r01", ".r02", ".r03"}
ZIP_EXTS = {".zip", ".zipx", ".jar"}


class EngineError(Exception):
    pass


@dataclass
class Entry:
    name: str
    size: int = 0
    packed: int = 0
    mtime: datetime | None = None
    crc: int | None = None
    method: str = ""
    is_dir: bool = False
    encrypted: bool = False

    @property
    def ratio(self) -> str:
        if self.is_dir or not self.size or not self.packed:
            return ""
        pct = 100 - int(round(self.packed * 100 / self.size))
        return f"{pct}%" if pct >= 0 else "-"


@dataclass
class ArchiveInfo:
    path: str
    format: str
    entries: list[Entry] = field(default_factory=list)
    comment: str = ""
    solid: bool = False
    locked: bool = False
    encrypted_names: bool = False
    recovery: bool = False
    needs_backend: str = ""     # set when an external tool is required


def detect_format(path: str) -> str:
    """Identify an archive by content signature first, extension as fallback."""
    try:
        with open(path, "rb") as fh:
            head = fh.read(512)
    except OSError:
        return "unknown"
    if head.startswith((_RAR4_SIG, _RAR5_SIG)):
        return "rar"
    if head.startswith(_REV_SIG):
        return "rev"
    if head.startswith((_ZIP_LOC, _ZIP_EOCD, _ZIP_SPAN)):
        return "zip"
    if head.startswith(_7Z_SIG):
        return "7z"
    if head.startswith(_XZ_SIG):
        return "xz"
    if head.startswith(_BZ2_SIG):
        return "bz2"
    if head.startswith(_ZST_SIG):
        return "zst"
    if head.startswith(_CAB_SIG):
        return "cab"
    if head[:2] == _GZ_SIG:
        return "gz"
    if len(head) > 262 and head[257:262] == _USTAR_MAGIC:
        return "tar"
    ext = Path(path).suffix.lower()
    if ext in RAR_EXTS:
        return "rar"
    if ext in ZIP_EXTS:
        return "zip"
    if ext == ".tar":
        return "tar"
    if ext in (".7z", ".gz", ".bz2", ".xz", ".zst", ".cab"):
        return ext[1:]
    return "unknown"


# --- UnRAR.exe backend -------------------------------------------------------

def find_unrar() -> str | None:
    """Locate a freeware UnRAR/Rar command-line backend (.exe only)."""
    self_dir = Path(__file__).resolve().parent
    candidates = [
        os.environ.get("UNRAR_EXE", ""),
        shutil.which("unrar.exe") or "",
        shutil.which("UnRAR.exe") or "",
        shutil.which("rar.exe") or "",
        str(self_dir / "UnRAR.exe"),
        str(self_dir / "unrar.exe"),
        r"C:\Program Files\WinRAR\UnRAR.exe",
        r"C:\Program Files (x86)\WinRAR\UnRAR.exe",
        r"C:\Program Files\WinRAR\Rar.exe",
        r"C:\Program Files (x86)\WinRAR\Rar.exe",
    ]
    for c in candidates:
        if c and c.lower().endswith((".exe", ".com")) and os.path.isfile(c):
            return c
    return None


def _run_unrar(args: list[str]) -> subprocess.CompletedProcess:
    exe = find_unrar()
    if not exe:
        raise EngineError(
            "RAR backend not found. Install WinRAR/UnRAR or set UNRAR_EXE.")
    proc = subprocess.run(
        [exe, *args], capture_output=True, text=True, errors="replace")
    if proc.returncode not in (0, 1):     # 1 = warnings, still usable output
        raise EngineError(proc.stderr.strip() or proc.stdout.strip()
                          or f"UnRAR exited with code {proc.returncode}")
    return proc


_RAR_FIELD = re.compile(r"^\s{0,2}([A-Za-z0-9 _]+?):\s+(.*)$")


def _rar_list(path: str) -> list[Entry]:
    """Parse `unrar lt` technical listing into entries."""
    proc = _run_unrar(["lt", "--", path])
    entries: list[Entry] = []
    cur: dict = {}
    started = False

    def flush():
        nonlocal cur
        if cur.get("name"):
            entries.append(Entry(
                name=cur["name"],
                size=cur.get("size", 0),
                packed=cur.get("packed", 0),
                mtime=cur.get("mtime"),
                crc=cur.get("crc"),
                method=cur.get("method", ""),
                is_dir=cur.get("type", "").lower() != "file",
                encrypted="encrypted" in cur.get("flags", "").lower(),
            ))
        cur = {}

    for line in proc.stdout.splitlines():
        if line.strip().startswith("Archive:"):
            started = True
            continue
        if not started:
            continue
        m = _RAR_FIELD.match(line)
        if not m:
            continue
        key, val = m.group(1).strip().lower(), m.group(2).strip()
        if key == "name" and cur.get("name"):
            flush()
        if key == "name":
            cur["name"] = val
        elif key == "type":
            cur["type"] = val
        elif key == "size":
            cur["size"] = _to_int(val)
        elif key == "packed size":
            cur["packed"] = _to_int(val)
        elif key in ("mtime", "modified"):
            cur["mtime"] = _to_dt(val)
        elif key in ("crc32", "blake2"):
            try:
                cur["crc"] = int(val.split()[0], 16)
            except ValueError:
                pass
        elif key == "compression":
            cur["method"] = val.split(",")[0].strip()
        elif key == "flags":
            cur["flags"] = val
    flush()
    return entries


def _to_int(s: str) -> int:
    digits = re.sub(r"[^\d]", "", s)
    return int(digits) if digits else 0


def _to_dt(s: str) -> datetime | None:
    # UnRAR prints e.g. "2026-10-12 21:58:07,239495300" — drop fraction/tz.
    s = re.sub(r"[,.]\d+", "", s).split("+")[0].split(" UTC")[0].strip()
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%d.%m.%Y %H:%M:%S"):
        try:
            return datetime.strptime(s, fmt)
        except ValueError:
            continue
    return None


def _rar_extract(path: str, dest: str, pwd: str | None = None) -> list[str]:
    args = ["x", "-o+", "-idq"]
    if pwd:
        args.append(f"-p{pwd}")
    args += ["--", path, dest + os.sep]
    proc = _run_unrar(args)
    return [dest]


def _rar_test(path: str) -> str:
    proc = _run_unrar(["t", "--", path])
    return proc.stdout.strip().splitlines()[-1] if proc.stdout.strip() else "OK"


# --- ZIP / TAR native ---------------------------------------------------------

def _zip_entries(zf: zipfile.ZipFile) -> list[Entry]:
    out = []
    for i in zf.infolist():
        try:
            mt = datetime(*i.date_time)
        except ValueError:
            mt = None
        out.append(Entry(
            name=i.filename,
            size=i.file_size,
            packed=i.compress_size,
            mtime=mt,
            crc=i.CRC,
            method=ZIP_METHODS.get(i.compress_type, f"type {i.compress_type}"),
            is_dir=i.is_dir(),
            encrypted=bool(i.flag_bits & 0x1),
        ))
    return out


def _tar_entries(tf: tarfile.TarFile) -> list[Entry]:
    out = []
    for m in tf.getmembers():
        out.append(Entry(
            name=m.name, size=m.size, packed=m.size,
            mtime=datetime.fromtimestamp(m.mtime) if m.mtime else None,
            method="tar", is_dir=m.isdir(),
        ))
    return out


def list_archive(path: str) -> ArchiveInfo:
    fmt = detect_format(path)
    info = ArchiveInfo(path=path, format=fmt)
    if fmt == "zip":
        with zipfile.ZipFile(path) as zf:
            info.entries = _zip_entries(zf)
            if zf.comment:
                info.comment = zf.comment.decode("utf-8", "replace")
    elif fmt == "rar" or fmt == "rev":
        info.entries = _rar_list(path)
    elif fmt in ("tar", "gz", "bz2", "xz"):
        try:
            with tarfile.open(path, "r:*") as tf:
                info.entries = _tar_entries(tf)
        except tarfile.TarError:
            if fmt == "gz":  # plain .gz single stream
                info.entries = [Entry(
                    name=Path(path).stem, size=os.path.getsize(path),
                    method="GZIP")]
            else:
                raise EngineError(f"Cannot read {fmt} archive")
    elif fmt in ("7z", "cab", "zst"):
        info.needs_backend = fmt
    else:
        raise EngineError(f"Unknown or unsupported archive format: {path}")
    return info


def extract_archive(path: str, dest: str, members: list[str] | None = None,
                    pwd: str | None = None) -> list[str]:
    """Extract `members` (or all) to dest. Returns top-level written paths."""
    fmt = detect_format(path)
    os.makedirs(dest, exist_ok=True)
    if fmt == "zip":
        written = []
        with zipfile.ZipFile(path) as zf:
            names = members or zf.namelist()
            try:
                zf.extractall(dest, members=names,
                              pwd=pwd.encode() if pwd else None)
            except RuntimeError as e:
                raise EngineError(str(e)) from e
            written = [os.path.join(dest, n) for n in names]
        return written
    if fmt in ("rar", "rev"):
        if members:
            # UnRAR x archive file1 file2 ... dest
            exe = find_unrar()
            if not exe:
                raise EngineError("RAR backend not found (UNRAR_EXE).")
            args = [exe, "x", "-o+", "-idq"]
            if pwd:
                args.append(f"-p{pwd}")
            args += ["--", path, *members, dest + os.sep]
            proc = subprocess.run(args, capture_output=True, text=True,
                                  errors="replace")
            if proc.returncode > 1:
                raise EngineError(proc.stderr.strip() or "Extraction failed")
            return [os.path.join(dest, m) for m in members]
        return _rar_extract(path, dest, pwd)
    if fmt in ("tar", "gz", "bz2", "xz"):
        with tarfile.open(path, "r:*") as tf:
            sel = set(members) if members else None
            targets = [m for m in tf.getmembers()
                       if sel is None or m.name in sel]
            tf.extractall(dest, members=targets, filter="data")
        return [dest]
    raise EngineError(f"Extracting {fmt} needs an external backend")


def read_member(path: str, name: str, pwd: str | None = None,
                limit: int = 32 * 1024 * 1024) -> bytes:
    """Read one archive member into memory (for the internal viewer)."""
    fmt = detect_format(path)
    if fmt == "zip":
        with zipfile.ZipFile(path) as zf:
            try:
                with zf.open(name, pwd=pwd.encode() if pwd else None) as fh:
                    return fh.read(limit)
            except RuntimeError as e:
                raise EngineError(str(e)) from e
    if fmt in ("rar", "rev"):
        exe = find_unrar()
        if not exe:
            raise EngineError("RAR backend not found (UNRAR_EXE).")
        args = [exe, "p", "-inul"]
        if pwd:
            args.append(f"-p{pwd}")
        # UnRAR 'p' needs a * wildcard to match members inside folders and
        # an absolute archive path (rc 10 otherwise).
        args += ["--", os.path.abspath(path),
                 "*" + name.replace("\\", "/").rsplit("/", 1)[-1]]
        proc = subprocess.run(args, capture_output=True)
        if proc.returncode > 1:
            raise EngineError(proc.stderr.decode("utf-8", "replace").strip()
                              or "Cannot read member")
        return proc.stdout[:limit]
    if fmt in ("tar", "gz", "bz2", "xz"):
        with tarfile.open(path, "r:*") as tf:
            fh = tf.extractfile(name)
            if fh is None:
                raise EngineError(f"'{name}' is not a file")
            return fh.read(limit)
    raise EngineError(f"Reading {fmt} members needs an external backend")


def test_archive(path: str) -> tuple[bool, str]:
    """Verify integrity (CRC where available). Returns (ok, detail)."""
    fmt = detect_format(path)
    if fmt == "zip":
        with zipfile.ZipFile(path) as zf:
            bad = zf.testzip()
        if bad:
            return False, f"CRC failed in {bad}"
        return True, f"All OK — {len(zf.namelist())} entries verified (CRC32)"
    if fmt in ("rar", "rev"):
        return True, _rar_test(path)
    if fmt in ("tar", "gz", "bz2", "xz"):
        with tarfile.open(path, "r:*") as tf:
            n = 0
            for m in tf:
                n += 1
                if m.isfile():
                    for _ in tf.extractfile(m):
                        pass
        return True, f"All OK — {n} entries readable"
    return False, f"Testing {fmt} needs an external backend"


def add_to_zip(path: str, files: list[str], level: str = "Normal") -> int:
    """Add files/folders to a ZIP archive. Returns count of files written."""
    lvl = _COMPRESSLEVELS.get(level, 6)
    comp = zipfile.ZIP_STORED if lvl == 0 else zipfile.ZIP_DEFLATED
    written = 0
    mode = "a" if os.path.exists(path) else "w"
    with zipfile.ZipFile(path, mode, compression=comp,
                         compresslevel=None if lvl == 0 else lvl,
                         allowZip64=True) as zf:
        for f in files:
            p = Path(f)
            if p.is_dir():
                for child in sorted(p.rglob("*")):
                    if child.is_file():
                        arc = str(child.relative_to(p.parent))
                        zf.write(child, arc)
                        written += 1
            elif p.is_file():
                zf.write(p, p.name)
                written += 1
    return written


def is_encrypted(info: ArchiveInfo) -> bool:
    return any(e.encrypted for e in info.entries)

"""UnRAR icons — self-contained generated PNG images (no external files).

Each icon is a small pixel-art grid; characters map to palette colors,
'.' is transparent. Grids are 16x16; toolbar usage zooms 2x for a
crisp 32px classic look.
"""

from __future__ import annotations

import base64
import struct
import zlib

PALETTE = {
    "K": (38, 38, 38, 255),      # outline / dark
    "W": (255, 255, 255, 255),   # white
    "S": (176, 184, 193, 255),   # silver gray
    "s": (220, 226, 232, 255),   # light gray
    "B": (43, 108, 176, 255),    # blue
    "b": (107, 166, 220, 255),   # light blue
    "G": (63, 158, 77, 255),     # green
    "g": (124, 200, 135, 255),   # light green
    "Y": (232, 185, 49, 255),    # folder yellow
    "y": (247, 215, 116, 255),   # folder light yellow
    "R": (200, 69, 44, 255),     # red
    "O": (232, 132, 44, 255),    # orange
    "P": (138, 90, 168, 255),    # purple
    "N": (247, 243, 218, 255),   # paper cream
}


def encode_png(rows: list[str]) -> bytes:
    """Encode a grid of palette chars into PNG bytes."""
    h = len(rows)
    w = len(rows[0])
    raw = bytearray()
    for row in rows:
        raw.append(0)
        for ch in row:
            raw += bytes(PALETTE.get(ch, (0, 0, 0, 0)))

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data)))

    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
            + chunk(b"IEND", b""))


ICON_ART: dict[str, list[str]] = {

    # -- tree / list icons (16x16) --

    "folder": [
        "................",
        "..KKKKK.........",
        ".KyyyyyKK.......",
        ".KyyyyyyyKKKK...",
        ".KyyyyyyyyyyyK..",
        ".KyKKKKKKKKKyK..",
        ".KyWWWWWWWWWyK..",
        ".KyyyyyyyyyyyK..",
        ".KyyyyyyyyyyyK..",
        ".KyyyyyyyyyyyK..",
        ".KyyyyyyyyyyyK..",
        ".KyyyyyyyyyyyK..",
        ".KKKKKKKKKKKKK..",
        "................",
        "................",
        "................",
    ],

    "file": [
        "................",
        "....KKKKKKK.....",
        "...KWWWWWWWkK...",
        "...KWWWWWKkKK...",
        "...KWWWWKWWKkK..",
        "...KWWWWWWWWK...",
        "...KWKKKKKWWK...",
        "...KWWWWWWWWK...",
        "...KWKKKKWWWK...",
        "...KWWWWWWWWK...",
        "...KWKKKKKWKK...",
        "...KWWWWWWWWK...",
        "...KWWWWWWWWK...",
        "...KKKKKKKKKK...",
        "................",
        "................",
    ],

    # -- toolbar icons (16x16, drawn zoomed) --

    "open": [
        "................",
        "..KKKKK.........",
        ".KyyyyyKK.......",
        ".KyyyyyyyKKKK...",
        ".KyyyyyyybbbbK..",
        ".KyKKKKKbbbbbK..",
        ".KyWWbbKKKKKKK..",
        ".KyybbbbWWWWWK..",
        ".KbbbbbbbbbbbK..",
        ".KbbbbbbbbbbbK..",
        ".KKKKKKKKKKKKK..",
        "................",
        "................",
        "................",
        "................",
        "................",
    ],

    "new": [
        "................",
        "....KKKKKKK.....",
        "...KWWWWWWWkK...",
        "...KWWWWWKkKK...",
        "...KWWWWKWWKkK..",
        "...KWWWWWWWWK...",
        "...KWKKKKKWWK...",
        "...KWWWWWWWWK...",
        "...KWKKKKWWWKg..",
        "...KWWWWWWWWKg.",
        "...KWKKKKggggK..",
        "...KWWWWKggWWK..",
        "...KKKKKggggKK..",
        "........gKKg....",
        ".........gg.....",
        "................",
    ],

    "add": [
        "................",
        "................",
        ".......GGG......",
        ".......GGG......",
        ".......GGG......",
        ".......GGG......",
        "...GGGGGGGGGGG..",
        "...GGGGGGGGGGG..",
        "...GGGGGGGGGGG..",
        ".......GGG......",
        ".......GGG......",
        ".......GGG......",
        ".......GGG......",
        "................",
        "................",
        "................",
    ],

    "extract": [
        "................",
        "................",
        "........B.......",
        ".......BBB......",
        "......BBBBB.....",
        "....BBBBBBBBBB..",
        "......BBBBB.....",
        ".......BBB......",
        "........B.......",
        "........B.......",
        "..KKKKK.KKKKKK..",
        "..KssssssssssK..",
        "..KssssssssssK..",
        "..KKKKKKKKKKKK..",
        "................",
        "................",
    ],

    "test": [
        "................",
        "................",
        ".............G..",
        "............GG..",
        "...........GG...",
        "..........GG....",
        "..G......GG.....",
        "..GG....GG......",
        "...GG..GG.......",
        "....GGGG........",
        ".....GG.........",
        "................",
        "................",
        "................",
        "................",
        "................",
    ],

    "view": [
        "................",
        "................",
        "................",
        "....KKKKKKKK....",
        "..KKbbbbbbbbKK..",
        ".KbbWWbbWWbbKKK.",
        ".KbWKKKKKKWKbbK.",
        ".KbWKbKKbKWKbbK.",
        ".KbWKKKKKKWKbbK.",
        ".KbbWWbbWWbbKKK.",
        "..KKbbbbbbbbKK..",
        "....KKKKKKKK....",
        "................",
        "................",
        "................",
        "................",
    ],

    "delete": [
        "................",
        "................",
        "..RR........RR..",
        "..RRR......RRR..",
        "...RRR....RRR...",
        "....RRR..RRR....",
        ".....RRRRRR.....",
        "......RRRR......",
        ".....RRRRRR.....",
        "....RRR..RRR....",
        "...RRR....RRR...",
        "..RRR......RRR..",
        "..RR........RR..",
        "................",
        "................",
        "................",
    ],

    "find": [
        "................",
        "....KKKKK.......",
        "...KbbbbbKKK....",
        "..KbWWWWWbbK....",
        "..KbWKKKWbbK....",
        "..KbWKKKWbbK....",
        "..KbWWWWWbbK....",
        "...KbbbbbKKK....",
        "....KKKKK.KKK...",
        "...........KKK..",
        "............KKK.",
        ".............KKK",
        "..............KK",
        "...............K",
        "................",
        "................",
    ],

    "info": [
        "................",
        ".....KKKKK......",
        "....KbbbbbK.....",
        "...KbWWWWbbK....",
        "...KbbbbbbbK....",
        "...KbbWWbbbK....",
        "...KbbWWbbbK....",
        "...KbbWWbbbK....",
        "...KbbWWbbbK....",
        "...KbbWWbbbK....",
        "...KbWWWWbbK....",
        "...KbbbbbbbK....",
        "....KbbbbbK.....",
        ".....KKKKK......",
        "................",
        "................",
    ],

    "comment": [
        "................",
        "..KKKKKKKKKKK...",
        ".KOssssssssssOK.",
        ".KOsKKKKKKsssOK.",
        ".KOssssssssssOK.",
        ".KOssKKKKKsssOK.",
        ".KOssssssssssOK.",
        ".KOsssKKKKsssOK.",
        ".KOssssssssssOK.",
        ".KOssssssssssOK.",
        "..KKKOOOKKKKKK..",
        "....KOOK....K...",
        ".....KO.K.K.....",
        "......K...K.....",
        "................",
        "................",
    ],

    "lock": [
        "................",
        "................",
        ".....KKKKK......",
        "....KKYYYKK.....",
        "....KY...YK.....",
        "....KY...YK.....",
        "...KKKKKKKKK....",
        "...KYYYYYYYK....",
        "...KYYYKKYYK....",
        "...KYYYKKYYK....",
        "...KYYYYYYYK....",
        "...KYYYYYYYK....",
        "...KYYYYYYYK....",
        "...KKKKKKKKK....",
        "................",
        "................",
    ],

    "help": [
        "................",
        ".....KKKKK......",
        "....KbbbbbK.....",
        "...KbWWWWbbK....",
        "...KbbKKKbbK....",
        "...KbbK.KbbK....",
        "...KbbbKbbbK....",
        "...KbbKKbbbK....",
        "...KbbKbbbbbK...",
        "...KbbKbbbbK....",
        "...KbbKKbbbK....",
        "...KbbKKbbbK....",
        "....KbbbbbK.....",
        ".....KKKKK......",
        "................",
        "................",
    ],

    "up": [
        "................",
        "................",
        ".......B........",
        "......BBB.......",
        ".....BBBBB......",
        "....BB.B.BB.....",
        ".......B........",
        ".......B........",
        "..KKKKKBKKKKK...",
        "..KyyyyyyyyyK...",
        "..KyyyyyyyyyK...",
        "..KyyyyyyyyyK...",
        "..KKKKKKKKKKK...",
        "................",
        "................",
        "................",
    ],
}


def png_bytes(name: str) -> bytes:
    return encode_png(ICON_ART[name])


def png_b64(name: str) -> str:
    return base64.b64encode(encode_png(ICON_ART[name])).decode("ascii")


def load(root) -> dict:
    """Return {'name': PhotoImage}. Must be called after Tk() exists."""
    import tkinter as tk
    return {n: tk.PhotoImage(data=png_b64(n)) for n in ICON_ART}


def load_toolbar(root) -> dict:
    """Same icons zoomed 2x (32px) for toolbar buttons."""
    import tkinter as tk
    return {n: tk.PhotoImage(data=png_b64(n)).zoom(2) for n in ICON_ART}

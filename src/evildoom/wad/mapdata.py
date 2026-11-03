"""Doom-format map lumps parsed into plain dataclasses.

Only the Doom (vanilla/Boom/MBF) binary format is supported for now. Hexen-format and UDMF
maps raise NotImplementedError.
"""
import struct
from dataclasses import dataclass, field

from .wad import Wad

NO_SIDEDEF = 0xFFFF

# Linedef flags (vanilla, Boom, MBF21)
ML_BLOCKING = 0x0001
ML_BLOCKMONSTERS = 0x0002
ML_TWOSIDED = 0x0004
ML_SECRET = 0x0020          # shows as one-sided on the automap
ML_SOUNDBLOCK = 0x0040
ML_DONTDRAW = 0x0080
ML_MAPPED = 0x0100
ML_PASSUSE = 0x0200         # Boom: use passes through to lines behind
ML_BLOCKLANDMONSTERS = 0x1000  # MBF21
ML_BLOCKPLAYERS = 0x2000       # MBF21

# Thing flags
MTF_EASY = 0x0001
MTF_NORMAL = 0x0002
MTF_HARD = 0x0004
MTF_AMBUSH = 0x0008
MTF_NOTSINGLE = 0x0010      # multiplayer only
MTF_NOTDM = 0x0020          # Boom
MTF_NOTCOOP = 0x0040        # Boom
MTF_FRIEND = 0x0080         # MBF


@dataclass
class Vertex:
    x: int
    y: int


@dataclass
class Thing:
    x: int
    y: int
    angle: int
    type: int
    flags: int

    def in_skill(self, skill: int) -> bool:
        """skill 1-5 as in ViZDoom / -skill. 1-2 = easy bit, 3 = normal, 4-5 = hard."""
        bit = MTF_EASY if skill <= 2 else MTF_NORMAL if skill == 3 else MTF_HARD
        return bool(self.flags & bit) and not self.flags & MTF_NOTSINGLE


@dataclass
class Sidedef:
    x_offset: int
    y_offset: int
    upper: str
    lower: str
    middle: str
    sector: int


@dataclass
class Linedef:
    index: int
    v1: int
    v2: int
    flags: int
    special: int
    tag: int
    front: int  # sidedef index, NO_SIDEDEF if absent
    back: int

    @property
    def two_sided(self) -> bool:
        return self.back != NO_SIDEDEF


@dataclass
class Sector:
    index: int
    floor: int
    ceiling: int
    floor_tex: str
    ceiling_tex: str
    light: int
    special: int
    tag: int


@dataclass
class Seg:
    v1: int
    v2: int
    angle: int
    linedef: int
    direction: int  # 0 = same side as linedef front, 1 = back
    offset: int


@dataclass
class Subsector:
    seg_count: int
    first_seg: int


@dataclass
class MapData:
    name: str
    things: list[Thing]
    vertexes: list[Vertex]
    linedefs: list[Linedef]
    sidedefs: list[Sidedef]
    sectors: list[Sector]
    segs: list[Seg] = field(default_factory=list)
    subsectors: list[Subsector] = field(default_factory=list)

    def front_sector(self, line: Linedef) -> int | None:
        return self.sidedefs[line.front].sector if line.front != NO_SIDEDEF else None

    def back_sector(self, line: Linedef) -> int | None:
        return self.sidedefs[line.back].sector if line.back != NO_SIDEDEF else None

    def sector_lines(self, sector: int) -> list[Linedef]:
        return [l for l in self.linedefs
                if self.front_sector(l) == sector or self.back_sector(l) == sector]

    def tagged_sectors(self, tag: int) -> list[Sector]:
        return [s for s in self.sectors if tag and s.tag == tag]


def _name(raw: bytes) -> str:
    return raw.split(b"\0", 1)[0].decode("ascii", errors="replace").upper()


def _records(data: bytes, fmt: str):
    size = struct.calcsize(fmt)
    for i in range(len(data) // size):
        yield i, struct.unpack_from(fmt, data, i * size)


def load_map(wad: Wad, map_name: str) -> MapData:
    lumps = wad.map_lumps(map_name)
    if "TEXTMAP" in lumps:
        raise NotImplementedError(f"{map_name}: UDMF maps are not supported yet")
    if "BEHAVIOR" in lumps:
        raise NotImplementedError(f"{map_name}: Hexen-format maps are not supported yet")

    things = [Thing(*r) for _, r in _records(lumps["THINGS"], "<hhhHH")]
    vertexes = [Vertex(*r) for _, r in _records(lumps["VERTEXES"], "<hh")]
    linedefs = [Linedef(i, *r) for i, r in _records(lumps["LINEDEFS"], "<HHHHHHH")]
    sidedefs = [
        Sidedef(xo, yo, _name(up), _name(lo), _name(mid), sec)
        for _, (xo, yo, up, lo, mid, sec) in _records(lumps["SIDEDEFS"], "<hh8s8s8sH")
    ]
    sectors = [
        Sector(i, fl, ce, _name(ft), _name(ct), light, special, tag)
        for i, (fl, ce, ft, ct, light, special, tag)
        in _records(lumps["SECTORS"], "<hh8s8shHH")
    ]

    # Standard nodes only; extended/GL nodes (ZNOD, XNOD, ...) are left empty for now.
    segs, subsectors = [], []
    ssectors_raw = lumps.get("SSECTORS", b"")
    if ssectors_raw and not ssectors_raw.startswith((b"XNOD", b"ZNOD", b"XGLN", b"ZGLN")):
        segs = [Seg(*r) for _, r in _records(lumps.get("SEGS", b""), "<HHhHhh")]
        subsectors = [Subsector(*r) for _, r in _records(ssectors_raw, "<HH")]

    return MapData(map_name.upper(), things, vertexes, linedefs, sidedefs, sectors,
                   segs, subsectors)

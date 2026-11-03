"""Wall texture sizes from TEXTURE1/TEXTURE2.

Each lump: int32 count, int32 offsets[count], then per texture: name[8], int32 masked,
int16 width, int16 height, int32 columndirectory (unused), int16 patchcount, patches[].
Only names and heights are needed (for "raise floor by shortest lower texture").
"""
import struct

from .wad import Wad


def texture_heights(wad: Wad) -> dict[str, int]:
    heights: dict[str, int] = {}
    for lump in wad.lumps:
        if lump.name not in ("TEXTURE1", "TEXTURE2"):
            continue
        data = wad.read(lump)
        (count,) = struct.unpack_from("<i", data, 0)
        for i in range(count):
            (offset,) = struct.unpack_from("<i", data, 4 + 4 * i)
            raw, _masked, _width, height = struct.unpack_from("<8sihh", data, offset)
            name = raw.split(b"\0", 1)[0].decode("ascii", errors="replace").upper()
            heights[name] = height
    return heights

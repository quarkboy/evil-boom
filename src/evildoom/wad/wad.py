"""WAD container: header, lump directory and lump access."""
import struct
from dataclasses import dataclass
from pathlib import Path

# Lumps that may follow a map marker, in any of the three map formats.
MAP_LUMPS = {
    "THINGS", "LINEDEFS", "SIDEDEFS", "VERTEXES", "SEGS", "SSECTORS", "NODES",
    "SECTORS", "REJECT", "BLOCKMAP", "BEHAVIOR", "SCRIPTS", "TEXTMAP", "ZNODES",
    "DIALOGUE", "ENDMAP",
}


@dataclass(frozen=True)
class Lump:
    name: str
    offset: int
    size: int


class Wad:
    def __init__(self, path: str | Path):
        self.path = Path(path)
        self.data = self.path.read_bytes()
        magic, count, dir_offset = struct.unpack_from("<4sii", self.data, 0)
        if magic not in (b"IWAD", b"PWAD"):
            raise ValueError(f"{self.path} is not a WAD (magic {magic!r})")
        self.kind = magic.decode()
        self.lumps: list[Lump] = []
        for i in range(count):
            offset, size, raw = struct.unpack_from("<ii8s", self.data, dir_offset + 16 * i)
            name = raw.split(b"\0", 1)[0].decode("ascii", errors="replace").upper()
            self.lumps.append(Lump(name, offset, size))

    def read(self, lump: Lump) -> bytes:
        return self.data[lump.offset:lump.offset + lump.size]

    def map_names(self) -> list[str]:
        """Map markers are lumps immediately followed by THINGS (Doom/Hexen) or TEXTMAP (UDMF)."""
        return [
            lump.name for lump, nxt in zip(self.lumps, self.lumps[1:])
            if nxt.name in ("THINGS", "TEXTMAP")
        ]

    def map_lumps(self, map_name: str) -> dict[str, bytes]:
        """Return the lumps belonging to a map. Later WADs win, so search from the end."""
        map_name = map_name.upper()
        for i in range(len(self.lumps) - 1, -1, -1):
            if self.lumps[i].name == map_name and i + 1 < len(self.lumps) \
                    and self.lumps[i + 1].name in ("THINGS", "TEXTMAP"):
                out = {}
                for lump in self.lumps[i + 1:]:
                    if lump.name not in MAP_LUMPS or lump.name in out:
                        break
                    out[lump.name] = self.read(lump)
                return out
        raise KeyError(f"map {map_name} not found in {self.path.name}")

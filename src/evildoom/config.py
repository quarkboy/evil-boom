"""Locating game data on the local machine."""
import os
from pathlib import Path

DEFAULT_DOOM_DIR = Path(r"C:\Program Files (x86)\Steam\steamapps\common\Ultimate Doom")

# Short names for the classic v1.9 IWADs inside the Steam install.
IWADS = {
    "doom": "base/DOOM.WAD",
    "doom2": "base/doom2/DOOM2.WAD",
    "tnt": "base/tnt/TNT.WAD",
    "plutonia": "base/plutonia/PLUTONIA.WAD",
}


def doom_dir() -> Path:
    return Path(os.environ.get("EVILDOOM_DOOM_DIR", DEFAULT_DOOM_DIR))


def resolve_wad(name_or_path: str) -> Path:
    """Accept a short IWAD name ("doom2") or a path, absolute or relative to the doom dir."""
    if name_or_path.lower() in IWADS:
        return doom_dir() / IWADS[name_or_path.lower()]
    path = Path(name_or_path)
    if path.exists():
        return path
    return doom_dir() / path

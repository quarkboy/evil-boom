"""Thing types that matter for navigation.

Solid decorations from the vanilla mobjinfo table (MF_SOLID set, not shootable/monster):
thing type -> collision radius. They block movement like a square box of that half-width,
just like the player. Hanging bodies are solid too (types 49-53 and 73-77; 59-63 are the
non-blocking duplicates). Explosive barrels are solid and shootable but don't move.
"""

SOLID_DECORATIONS: dict[int, int] = {
    2035: 10,  # explosive barrel
    70: 16,    # burning barrel
    2028: 16,  # floor lamp
    85: 16,    # tall techno floor lamp
    86: 16,    # short techno floor lamp
    48: 16,    # tall techno column
    30: 16, 31: 16, 32: 16, 33: 16,  # tall/short green and red pillars
    36: 16,    # short green pillar with beating heart
    37: 16,    # short red pillar with skull
    35: 16,    # candelabra
    41: 16,    # evil eye
    42: 16,    # floating skull rock
    43: 16,    # burnt tree
    47: 16,    # brown stump
    54: 32,    # large brown tree
    44: 16, 45: 16, 46: 16,  # tall blue/green/red firesticks
    55: 16, 56: 16, 57: 16,  # short blue/green/red firesticks
    25: 16, 26: 16, 27: 16, 28: 16, 29: 16,  # impaled bodies, skulls on poles, skull pile
    49: 16, 50: 16, 51: 16, 52: 16, 53: 16,  # hanging victims (solid)
    73: 16, 74: 16, 75: 16, 76: 16, 77: 16,  # hanging victims / torsos (Doom II, solid)
}

# Solid things that can be destroyed (shot), so they don't block a route for good.
SHOOTABLE_OBSTACLES = {2035}  # explosive barrel

# Things that can be picked up (vanilla Doom / Doom II): thing type -> (name, category).
PICKUPS: dict[int, tuple[str, str]] = {
    5: ("blue keycard", "key"), 6: ("yellow keycard", "key"), 13: ("red keycard", "key"),
    38: ("red skull key", "key"), 39: ("yellow skull key", "key"), 40: ("blue skull key", "key"),
    2001: ("shotgun", "weapon"), 82: ("super shotgun", "weapon"), 2002: ("chaingun", "weapon"),
    2003: ("rocket launcher", "weapon"), 2004: ("plasma rifle", "weapon"),
    2005: ("chainsaw", "weapon"), 2006: ("BFG9000", "weapon"),
    2007: ("clip", "ammo"), 2048: ("box of bullets", "ammo"), 2008: ("shells", "ammo"),
    2049: ("box of shells", "ammo"), 2010: ("rocket", "ammo"), 2046: ("box of rockets", "ammo"),
    2047: ("cell", "ammo"), 17: ("cell pack", "ammo"), 8: ("backpack", "ammo"),
    2011: ("stimpack", "health"), 2012: ("medikit", "health"), 2014: ("health bonus", "health"),
    2013: ("soulsphere", "health"), 83: ("megasphere", "health"),
    2018: ("green armor", "armor"), 2019: ("blue armor", "armor"), 2015: ("armor bonus", "armor"),
    2022: ("invulnerability", "powerup"), 2023: ("berserk", "powerup"),
    2024: ("partial invisibility", "powerup"), 2025: ("radiation suit", "powerup"),
    2026: ("computer area map", "powerup"), 2045: ("light amplification visor", "powerup"),
}

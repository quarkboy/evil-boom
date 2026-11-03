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

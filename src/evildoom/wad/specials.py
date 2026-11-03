"""Classification of linedef and sector specials into the terms navigation cares about.

Sources: the vanilla linedef type table (1-141), the Boom reference for generalized linedef
bitfields (0x2F80-0x7FFF) and generalized sector specials. Boom extended types (142+) are only
partially classified: exits and teleporters, which matter for navigation. Everything else in
that range comes back as Category.OTHER.
"""
from dataclasses import dataclass
from enum import Enum, IntEnum


class Trigger(Enum):
    W1 = "W1"   # walk over, once
    WR = "WR"   # walk over, repeatable
    S1 = "S1"   # switch (use), once
    SR = "SR"
    G1 = "G1"   # shoot, once
    GR = "GR"
    D1 = "D1"   # door-style use on the line itself, once
    DR = "DR"
    NONE = "--"  # passive / map setup (scrollers, transfers)

    @property
    def repeatable(self) -> bool:
        return self in (Trigger.WR, Trigger.SR, Trigger.GR, Trigger.DR)


GEN_TRIGGERS = [Trigger.W1, Trigger.WR, Trigger.S1, Trigger.SR,
                Trigger.G1, Trigger.GR, Trigger.D1, Trigger.DR]


class Category(Enum):
    DOOR = "door"
    LOCKED_DOOR = "locked_door"
    LIFT = "lift"
    FLOOR = "floor"
    CEILING = "ceiling"
    CRUSHER = "crusher"
    STAIRS = "stairs"
    EXIT = "exit"
    SECRET_EXIT = "secret_exit"
    TELEPORT = "teleport"
    LIGHT = "light"
    OTHER = "other"


class Key(IntEnum):
    """Key items, valued by their thing type."""
    BLUE_CARD = 5
    YELLOW_CARD = 6
    RED_CARD = 13
    RED_SKULL = 38
    YELLOW_SKULL = 39
    BLUE_SKULL = 40

    @property
    def color(self) -> str:
        return self.name.split("_")[0].lower()


class Lock(Enum):
    NONE = "none"
    RED = "red"          # either red key (vanilla behaviour)
    BLUE = "blue"
    YELLOW = "yellow"
    RED_CARD = "red_card"
    BLUE_CARD = "blue_card"
    YELLOW_CARD = "yellow_card"
    RED_SKULL = "red_skull"
    BLUE_SKULL = "blue_skull"
    YELLOW_SKULL = "yellow_skull"
    ANY = "any"          # any one key
    ALL_COLORS = "all_colors"  # one key of each color
    ALL_SIX = "all_six"

    def satisfied(self, keys: set[Key]) -> bool:
        colors = {k.color for k in keys}
        match self:
            case Lock.NONE:
                return True
            case Lock.RED | Lock.BLUE | Lock.YELLOW:
                return self.value in colors
            case Lock.ANY:
                return bool(keys)
            case Lock.ALL_COLORS:
                return colors == {"red", "blue", "yellow"}
            case Lock.ALL_SIX:
                return set(Key) <= keys
            case _:
                return Key[self.name] in keys


@dataclass(frozen=True)
class LineSpecial:
    special: int
    trigger: Trigger
    category: Category
    description: str
    lock: Lock = Lock.NONE
    monsters: bool = False  # can monsters activate it
    players: bool = True    # can players activate it
    generalized: bool = False


# fmt: off
_T = Trigger
_C = Category
# special: (trigger, category, description[, lock, monsters, players])
_VANILLA: dict[int, tuple] = {
    1:   (_T.DR, _C.DOOR, "door open-wait-close", Lock.NONE, True),
    2:   (_T.W1, _C.DOOR, "door open stay"),
    3:   (_T.W1, _C.DOOR, "door close"),
    4:   (_T.W1, _C.DOOR, "door open-wait-close", Lock.NONE, True),
    5:   (_T.W1, _C.FLOOR, "floor raise to lowest ceiling"),
    6:   (_T.W1, _C.CRUSHER, "fast crusher"),
    7:   (_T.S1, _C.STAIRS, "build stairs 8"),
    8:   (_T.W1, _C.STAIRS, "build stairs 8"),
    9:   (_T.S1, _C.FLOOR, "donut"),
    10:  (_T.W1, _C.LIFT, "lift", Lock.NONE, True),
    11:  (_T.S1, _C.EXIT, "exit level"),
    12:  (_T.W1, _C.LIGHT, "light to brightest neighbor"),
    13:  (_T.W1, _C.LIGHT, "light to 255"),
    14:  (_T.S1, _C.FLOOR, "floor raise 32, change texture"),
    15:  (_T.S1, _C.FLOOR, "floor raise 24, change texture"),
    16:  (_T.W1, _C.DOOR, "door close-wait-open"),
    17:  (_T.W1, _C.LIGHT, "light start blinking"),
    18:  (_T.S1, _C.FLOOR, "floor raise to next higher"),
    19:  (_T.W1, _C.FLOOR, "floor lower to highest neighbor"),
    20:  (_T.S1, _C.FLOOR, "floor raise to next higher, change texture"),
    21:  (_T.S1, _C.LIFT, "lift"),
    22:  (_T.W1, _C.FLOOR, "floor raise to next higher, change texture"),
    23:  (_T.S1, _C.FLOOR, "floor lower to lowest"),
    24:  (_T.G1, _C.FLOOR, "floor raise to lowest ceiling"),
    25:  (_T.W1, _C.CRUSHER, "slow crusher"),
    26:  (_T.DR, _C.LOCKED_DOOR, "blue door open-wait-close", Lock.BLUE),
    27:  (_T.DR, _C.LOCKED_DOOR, "yellow door open-wait-close", Lock.YELLOW),
    28:  (_T.DR, _C.LOCKED_DOOR, "red door open-wait-close", Lock.RED),
    29:  (_T.S1, _C.DOOR, "door open-wait-close"),
    30:  (_T.W1, _C.FLOOR, "floor raise by shortest lower texture"),
    31:  (_T.D1, _C.DOOR, "door open stay"),
    32:  (_T.D1, _C.LOCKED_DOOR, "blue door open stay", Lock.BLUE),
    33:  (_T.D1, _C.LOCKED_DOOR, "red door open stay", Lock.RED),
    34:  (_T.D1, _C.LOCKED_DOOR, "yellow door open stay", Lock.YELLOW),
    35:  (_T.W1, _C.LIGHT, "light to 35"),
    36:  (_T.W1, _C.FLOOR, "floor turbo lower to highest neighbor + 8"),
    37:  (_T.W1, _C.FLOOR, "floor lower to lowest, change texture"),
    38:  (_T.W1, _C.FLOOR, "floor lower to lowest"),
    39:  (_T.W1, _C.TELEPORT, "teleport", Lock.NONE, True),
    40:  (_T.W1, _C.CEILING, "ceiling raise to highest"),
    41:  (_T.S1, _C.CEILING, "ceiling lower to floor"),
    42:  (_T.SR, _C.DOOR, "door close"),
    43:  (_T.SR, _C.CEILING, "ceiling lower to floor"),
    44:  (_T.W1, _C.CEILING, "ceiling lower to 8 above floor"),
    45:  (_T.SR, _C.FLOOR, "floor lower to highest neighbor"),
    46:  (_T.GR, _C.DOOR, "door open stay"),
    47:  (_T.G1, _C.FLOOR, "floor raise to next higher, change texture"),
    48:  (_T.NONE, _C.OTHER, "scroll wall left"),
    49:  (_T.S1, _C.CRUSHER, "slow crusher"),
    50:  (_T.S1, _C.DOOR, "door close"),
    51:  (_T.S1, _C.SECRET_EXIT, "secret exit"),
    52:  (_T.W1, _C.EXIT, "exit level"),
    53:  (_T.W1, _C.LIFT, "perpetual lift start"),
    54:  (_T.W1, _C.LIFT, "perpetual lift stop"),
    55:  (_T.S1, _C.FLOOR, "floor raise to lowest ceiling - 8, crush"),
    56:  (_T.W1, _C.FLOOR, "floor raise to lowest ceiling - 8, crush"),
    57:  (_T.W1, _C.CRUSHER, "crusher stop"),
    58:  (_T.W1, _C.FLOOR, "floor raise 24"),
    59:  (_T.W1, _C.FLOOR, "floor raise 24, change texture"),
    60:  (_T.SR, _C.FLOOR, "floor lower to lowest"),
    61:  (_T.SR, _C.DOOR, "door open stay"),
    62:  (_T.SR, _C.LIFT, "lift"),
    63:  (_T.SR, _C.DOOR, "door open-wait-close"),
    64:  (_T.SR, _C.FLOOR, "floor raise to lowest ceiling"),
    65:  (_T.SR, _C.FLOOR, "floor raise to lowest ceiling - 8, crush"),
    66:  (_T.SR, _C.FLOOR, "floor raise 24, change texture"),
    67:  (_T.SR, _C.FLOOR, "floor raise 32, change texture"),
    68:  (_T.SR, _C.FLOOR, "floor raise to next higher, change texture"),
    69:  (_T.SR, _C.FLOOR, "floor raise to next higher"),
    70:  (_T.SR, _C.FLOOR, "floor turbo lower to highest neighbor + 8"),
    71:  (_T.S1, _C.FLOOR, "floor turbo lower to highest neighbor + 8"),
    72:  (_T.WR, _C.CEILING, "ceiling lower to 8 above floor"),
    73:  (_T.WR, _C.CRUSHER, "slow crusher"),
    74:  (_T.WR, _C.CRUSHER, "crusher stop"),
    75:  (_T.WR, _C.DOOR, "door close"),
    76:  (_T.WR, _C.DOOR, "door close-wait-open"),
    77:  (_T.WR, _C.CRUSHER, "fast crusher"),
    79:  (_T.WR, _C.LIGHT, "light to 35"),
    80:  (_T.WR, _C.LIGHT, "light to brightest neighbor"),
    81:  (_T.WR, _C.LIGHT, "light to 255"),
    82:  (_T.WR, _C.FLOOR, "floor lower to lowest"),
    83:  (_T.WR, _C.FLOOR, "floor lower to highest neighbor"),
    84:  (_T.WR, _C.FLOOR, "floor lower to lowest, change texture"),
    86:  (_T.WR, _C.DOOR, "door open stay"),
    87:  (_T.WR, _C.LIFT, "perpetual lift start"),
    88:  (_T.WR, _C.LIFT, "lift", Lock.NONE, True),
    89:  (_T.WR, _C.LIFT, "perpetual lift stop"),
    90:  (_T.WR, _C.DOOR, "door open-wait-close"),
    91:  (_T.WR, _C.FLOOR, "floor raise to lowest ceiling"),
    92:  (_T.WR, _C.FLOOR, "floor raise 24"),
    93:  (_T.WR, _C.FLOOR, "floor raise 24, change texture"),
    94:  (_T.WR, _C.FLOOR, "floor raise to lowest ceiling - 8, crush"),
    95:  (_T.WR, _C.FLOOR, "floor raise to next higher, change texture"),
    96:  (_T.WR, _C.FLOOR, "floor raise by shortest lower texture"),
    97:  (_T.WR, _C.TELEPORT, "teleport", Lock.NONE, True),
    98:  (_T.WR, _C.FLOOR, "floor turbo lower to highest neighbor + 8"),
    99:  (_T.SR, _C.LOCKED_DOOR, "blue blazing door open stay", Lock.BLUE),
    100: (_T.W1, _C.STAIRS, "turbo build stairs 16"),
    101: (_T.S1, _C.FLOOR, "floor raise to lowest ceiling"),
    102: (_T.S1, _C.FLOOR, "floor lower to highest neighbor"),
    103: (_T.S1, _C.DOOR, "door open stay"),
    104: (_T.W1, _C.LIGHT, "light to darkest neighbor"),
    105: (_T.WR, _C.DOOR, "blazing door open-wait-close"),
    106: (_T.WR, _C.DOOR, "blazing door open stay"),
    107: (_T.WR, _C.DOOR, "blazing door close"),
    108: (_T.W1, _C.DOOR, "blazing door open-wait-close"),
    109: (_T.W1, _C.DOOR, "blazing door open stay"),
    110: (_T.W1, _C.DOOR, "blazing door close"),
    111: (_T.S1, _C.DOOR, "blazing door open-wait-close"),
    112: (_T.S1, _C.DOOR, "blazing door open stay"),
    113: (_T.S1, _C.DOOR, "blazing door close"),
    114: (_T.SR, _C.DOOR, "blazing door open-wait-close"),
    115: (_T.SR, _C.DOOR, "blazing door open stay"),
    116: (_T.SR, _C.DOOR, "blazing door close"),
    117: (_T.DR, _C.DOOR, "blazing door open-wait-close"),
    118: (_T.D1, _C.DOOR, "blazing door open stay"),
    119: (_T.W1, _C.FLOOR, "floor raise to next higher"),
    120: (_T.WR, _C.LIFT, "blazing lift"),
    121: (_T.W1, _C.LIFT, "blazing lift"),
    122: (_T.S1, _C.LIFT, "blazing lift"),
    123: (_T.SR, _C.LIFT, "blazing lift"),
    124: (_T.W1, _C.SECRET_EXIT, "secret exit"),
    125: (_T.W1, _C.TELEPORT, "teleport (monsters only)", Lock.NONE, True, False),
    126: (_T.WR, _C.TELEPORT, "teleport (monsters only)", Lock.NONE, True, False),
    127: (_T.S1, _C.STAIRS, "turbo build stairs 16"),
    128: (_T.WR, _C.FLOOR, "floor raise to next higher"),
    129: (_T.WR, _C.FLOOR, "floor turbo raise to next higher"),
    130: (_T.W1, _C.FLOOR, "floor turbo raise to next higher"),
    131: (_T.S1, _C.FLOOR, "floor turbo raise to next higher"),
    132: (_T.SR, _C.FLOOR, "floor turbo raise to next higher"),
    133: (_T.S1, _C.LOCKED_DOOR, "blue blazing door open stay", Lock.BLUE),
    134: (_T.SR, _C.LOCKED_DOOR, "red blazing door open stay", Lock.RED),
    135: (_T.S1, _C.LOCKED_DOOR, "red blazing door open stay", Lock.RED),
    136: (_T.SR, _C.LOCKED_DOOR, "yellow blazing door open stay", Lock.YELLOW),
    137: (_T.S1, _C.LOCKED_DOOR, "yellow blazing door open stay", Lock.YELLOW),
    138: (_T.SR, _C.LIGHT, "light to 255"),
    139: (_T.SR, _C.LIGHT, "light to 35"),
    140: (_T.S1, _C.FLOOR, "floor raise 512"),
    141: (_T.W1, _C.CRUSHER, "silent crusher"),
}

# Boom extended (non-generalized) types relevant to navigation.
_BOOM: dict[int, tuple] = {
    174: (_T.S1, _C.TELEPORT, "teleport", Lock.NONE, True),
    195: (_T.SR, _C.TELEPORT, "teleport", Lock.NONE, True),
    197: (_T.G1, _C.EXIT, "exit level"),
    198: (_T.G1, _C.SECRET_EXIT, "secret exit"),
    207: (_T.W1, _C.TELEPORT, "silent teleport", Lock.NONE, True),
    208: (_T.WR, _C.TELEPORT, "silent teleport", Lock.NONE, True),
    209: (_T.S1, _C.TELEPORT, "silent teleport", Lock.NONE, True),
    210: (_T.SR, _C.TELEPORT, "silent teleport", Lock.NONE, True),
    243: (_T.W1, _C.TELEPORT, "silent line-to-line teleport", Lock.NONE, True),
    244: (_T.WR, _C.TELEPORT, "silent line-to-line teleport", Lock.NONE, True),
}
# fmt: on

_GEN_LOCKS = [Lock.ANY, Lock.RED_CARD, Lock.BLUE_CARD, Lock.YELLOW_CARD,
              Lock.RED_SKULL, Lock.BLUE_SKULL, Lock.YELLOW_SKULL, Lock.ALL_SIX]
_SKULL_IS_CARD = {Lock.RED_CARD: Lock.RED, Lock.BLUE_CARD: Lock.BLUE,
                  Lock.YELLOW_CARD: Lock.YELLOW, Lock.RED_SKULL: Lock.RED,
                  Lock.BLUE_SKULL: Lock.BLUE, Lock.YELLOW_SKULL: Lock.YELLOW,
                  Lock.ALL_SIX: Lock.ALL_COLORS, Lock.ANY: Lock.ANY}
_SPEEDS = ["slow", "normal", "fast", "turbo"]


def _generalized(special: int) -> LineSpecial | None:
    trigger = GEN_TRIGGERS[special & 7]
    speed = _SPEEDS[(special >> 3) & 3]

    def make(category, desc, lock=Lock.NONE, monsters=False):
        return LineSpecial(special, trigger, category, f"{speed} {desc}", lock, monsters,
                           True, generalized=True)

    if special > 0x7FFF:
        return None
    if special >= 0x4000:
        is_floor = special >= 0x6000
        change = (special >> 10) & 3
        monsters = change == 0 and bool(special & 0x20)
        direction = "up" if special & 0x40 else "down"
        crush = ", crush" if special & 0x1000 else ""
        if is_floor:
            return make(Category.FLOOR, f"floor move {direction}{crush}", monsters=monsters)
        return make(Category.CEILING, f"ceiling move {direction}{crush}", monsters=monsters)
    if special >= 0x3C00:
        kind = ["open-wait-close", "open stay", "close-wait-open", "close"][(special >> 5) & 3]
        return make(Category.DOOR, f"door {kind}", monsters=bool(special & 0x80))
    if special >= 0x3800:
        kind = "open stay" if special & 0x20 else "open-wait-close"
        lock = _GEN_LOCKS[(special >> 6) & 7]
        if special & 0x200:
            lock = _SKULL_IS_CARD[lock]
        return make(Category.LOCKED_DOOR, f"{lock.value} locked door {kind}", lock)
    if special >= 0x3400:
        return make(Category.LIFT, "lift", monsters=bool(special & 0x20))
    if special >= 0x3000:
        return make(Category.STAIRS, "build stairs", monsters=bool(special & 0x20))
    if special >= 0x2F80:
        return make(Category.CRUSHER, "crusher", monsters=bool(special & 0x20))
    return None


def classify_line(special: int) -> LineSpecial | None:
    """Return the classification of a linedef special, or None for special 0."""
    if special == 0:
        return None
    if special >= 0x2F80 and (gen := _generalized(special)):
        return gen
    entry = _VANILLA.get(special) or _BOOM.get(special)
    if entry:
        trigger, category, desc, *rest = entry
        return LineSpecial(special, trigger, category, desc, *rest)
    return LineSpecial(special, Trigger.NONE, Category.OTHER, f"unclassified special {special}")


@dataclass(frozen=True)
class SectorInfo:
    damage: int        # health per damage tic (0, 5, 10, 20)
    secret: bool
    ends_level: bool   # E1M8-style: damage, and exit when health drops below 11


_VANILLA_DAMAGE = {4: 20, 5: 10, 7: 5, 11: 20, 16: 20}
_BOOM_DAMAGE = [0, 5, 10, 20]


def classify_sector(special: int) -> SectorInfo:
    """Vanilla sector specials live in bits 0-4; Boom adds damage, secret and friction bits."""
    base = special & 0x1F
    damage = max(_VANILLA_DAMAGE.get(base, 0), _BOOM_DAMAGE[(special >> 5) & 3])
    return SectorInfo(damage=damage, secret=base == 9 or bool(special & 0x80),
                      ends_level=base == 11)

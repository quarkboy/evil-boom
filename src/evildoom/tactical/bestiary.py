"""What's what among the things in a level, by ZDoom class name: monsters (how big, how
tough, how they hurt you), missiles in flight (how fast, how wide), the player's weapons and
the items lying about."""
import math
from dataclasses import dataclass


@dataclass(frozen=True)
class Monster:
    radius: float
    height: float
    health: int
    attack: str          # "hitscan", "missile", "melee", "charge" (lost souls), "fire" (arch-vile), "none"
    melee: bool = False  # also claws/bites up close
    threat: float = 1.0  # how much it matters to deal with it first (damage per second, roughly)
    boss: bool = False   # worth the big guns


MONSTERS: dict[str, Monster] = {
    "Zombieman": Monster(20, 56, 20, "hitscan", threat=2),
    "ShotgunGuy": Monster(20, 56, 30, "hitscan", threat=4),
    "ChaingunGuy": Monster(20, 56, 70, "hitscan", threat=6),
    "WolfensteinSS": Monster(20, 56, 50, "hitscan", threat=4),
    "DoomImp": Monster(20, 56, 60, "missile", melee=True, threat=2),
    "Demon": Monster(30, 56, 150, "melee", threat=3),
    "Spectre": Monster(30, 56, 150, "melee", threat=3),
    "LostSoul": Monster(16, 56, 100, "charge", threat=2),
    "Cacodemon": Monster(31, 56, 400, "missile", melee=True, threat=3),
    "HellKnight": Monster(24, 64, 500, "missile", melee=True, threat=4),
    "BaronOfHell": Monster(24, 64, 1000, "missile", melee=True, threat=4),
    "Arachnotron": Monster(64, 64, 500, "missile", threat=6),
    "PainElemental": Monster(31, 56, 400, "missile", threat=4),
    "Revenant": Monster(20, 56, 300, "missile", melee=True, threat=6),
    "Fatso": Monster(48, 64, 600, "missile", threat=7),
    "Archvile": Monster(20, 56, 700, "fire", threat=12),
    "Cyberdemon": Monster(40, 110, 4000, "missile", threat=15, boss=True),
    "SpiderMastermind": Monster(128, 100, 3000, "hitscan", threat=15, boss=True),
    "CommanderKeen": Monster(16, 72, 100, "none", threat=0),
}


@dataclass(frozen=True)
class Missile:
    radius: float
    speed: float
    damage: float  # roughly, on a direct hit
    splash: bool = False


MISSILES: dict[str, Missile] = {
    "DoomImpBall": Missile(6, 10, 12),
    "CacodemonBall": Missile(6, 10, 20),
    "BaronBall": Missile(6, 15, 32),
    "ArachnotronPlasma": Missile(13, 25, 20),
    "FatShot": Missile(6, 20, 32),
    "RevenantTracer": Missile(11, 10, 40),
    "Rocket": Missile(11, 20, 80, splash=True),
    "PlasmaBall": Missile(13, 25, 20),
    "BFGBall": Missile(13, 25, 400),
    "SpawnShot": Missile(6, 10, 0),   # the Icon of Sin's cubes
    "LostSoul": Missile(16, 20, 12),  # (when charging; it's a monster too)
}


@dataclass(frozen=True)
class Weapon:
    slot: int
    ammo_per_shot: int
    damage: float                    # a shot's, on average, every pellet hitting (a rocket's
    #                                  with its blast)
    cycle: int                       # tics from one shot to the next, holding the trigger
    spread: float = 0.0              # hitscan: pellets/bullets land up to this far off (degrees,
    #                                  most near the middle: P_Random() - P_Random())
    hitscan: bool = True
    speed: float = 0.0               # (its missile's, if not hitscan)
    splash: float = 0.0              # keep at least this far from what it hits
    reach: float = 2048.0            # no shooting at anything further (the spread wastes it)

    @property
    def dps(self) -> float:
        """Damage a second, most of it hitting (what she can do with it, roughly)."""
        return 0.8 * self.damage * TICS_PER_SECOND / self.cycle

    def hits(self, radius: float, d: float) -> float:
        """The share of a shot's damage that lands on something this wide this far off."""
        if not self.hitscan:
            return 0.85 if d < 768 else 0.85 * 768 / d  # (it may step out of the way)
        if self.spread == 0:
            return 1.0
        a = math.degrees(math.atan2(radius, max(d, 1.0)))
        return 1.0 if a >= self.spread else 1.0 - (1.0 - a / self.spread) ** 2


TICS_PER_SECOND = 35
# by slot (slot 1's fist and chainsaw share, as do slot 3's shotguns: SUPER_SHOTGUN)
WEAPONS: dict[int, Weapon] = {
    1: Weapon(1, 0, 11, 22, reach=80),
    2: Weapon(2, 1, 10, 14, spread=5.6, reach=800),
    3: Weapon(3, 1, 70, 37, spread=5.6, reach=640),
    4: Weapon(4, 1, 10, 4, spread=5.6, reach=1100),
    5: Weapon(5, 1, 180, 20, hitscan=False, speed=20, splash=160),
    6: Weapon(6, 1, 22, 3, hitscan=False, speed=25),
    7: Weapon(7, 40, 700, 60, hitscan=False, speed=25),
}
SUPER_SHOTGUN = Weapon(3, 2, 200, 57, spread=11.2, reach=480)


def weapon_in(slot: int, super_shotgun: bool | None) -> Weapon:
    """What a slot brings up (slot 3: the super shotgun, once she has it)."""
    return SUPER_SHOTGUN if slot == 3 and super_shotgun else WEAPONS[slot]


ITEMS: dict[str, str] = {
    "Shotgun": "Weapon", "SuperShotgun": "Weapon", "Chaingun": "Weapon",
    "RocketLauncher": "Weapon", "PlasmaRifle": "Weapon", "Chainsaw": "Weapon", "BFG9000": "Weapon",
    "Clip": "Ammo", "ClipBox": "Ammo", "Shell": "Ammo", "ShellBox": "Ammo", "RocketAmmo": "Ammo",
    "RocketBox": "Ammo", "Cell": "Ammo", "CellPack": "Ammo", "Backpack": "Ammo",
    "Stimpack": "Health", "Medikit": "Health", "HealthBonus": "Health", "Soulsphere": "Health",
    "Megasphere": "Health", "Berserk": "Health",
    "GreenArmor": "Armor", "BlueArmor": "Armor", "ArmorBonus": "Armor",
    "InvulnerabilitySphere": "Powerup", "BlurSphere": "Powerup", "RadSuit": "Powerup",
    "Allmap": "Powerup", "Infrared": "Powerup",
    "BlueCard": "Key", "YellowCard": "Key", "RedCard": "Key", "BlueSkull": "Key",
    "YellowSkull": "Key", "RedSkull": "Key",
}

# weapon pickups -> the slot they fill
WEAPON_ITEMS = {"Chainsaw": 1, "Shotgun": 3, "SuperShotgun": 3, "Chaingun": 4,
                "RocketLauncher": 5, "PlasmaRifle": 6, "BFG9000": 7}
# ammo pickups -> (slots it's for, how much)
AMMO_ITEMS = {"Clip": ((2, 4), 10), "ClipBox": ((2, 4), 50), "Shell": ((3,), 4),
              "ShellBox": ((3,), 20), "RocketAmmo": ((5,), 1), "RocketBox": ((5,), 5),
              "Cell": ((6, 7), 20), "CellPack": ((6, 7), 100), "Backpack": ((2, 3, 4, 5, 6, 7), 10)}
# health pickups -> how much (and up to what)
HEALTH_ITEMS = {"Stimpack": (10, 100), "Medikit": (25, 100), "HealthBonus": (1, 200),
                "Soulsphere": (100, 200), "Megasphere": (200, 200), "Berserk": (100, 100)}
ARMOR_ITEMS = {"GreenArmor": (100, 100), "BlueArmor": (200, 200), "ArmorBonus": (1, 200),
               "Megasphere": (200, 200)}


def category(name: str) -> str:
    if name in MONSTERS:
        return "Monster"
    if name in MISSILES:
        return "Projectile"
    return ITEMS.get(name, "Other")

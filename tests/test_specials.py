from evildoom.wad.specials import (Category, Key, Lock, Trigger, classify_line,
                                   classify_sector)


def test_zero_is_not_special():
    assert classify_line(0) is None


def test_vanilla_doors():
    door = classify_line(1)
    assert (door.trigger, door.category, door.lock, door.monsters) == \
        (Trigger.DR, Category.DOOR, Lock.NONE, True)
    blue = classify_line(26)
    assert (blue.category, blue.lock) == (Category.LOCKED_DOOR, Lock.BLUE)
    assert classify_line(137).lock is Lock.YELLOW


def test_monster_only_teleport():
    tele = classify_line(125)
    assert tele.category is Category.TELEPORT and tele.monsters and not tele.players


def test_unknown_special_is_other():
    info = classify_line(9999)
    assert info.category is Category.OTHER and info.description.startswith("unclassified")


def test_generalized_locked_door():
    # Locked door base | blue card (2) in bits 6-8 | DR trigger (7)
    special = 0x3800 | (2 << 6) | 7
    info = classify_line(special)
    assert info.generalized and info.category is Category.LOCKED_DOOR
    assert info.trigger is Trigger.DR and info.lock is Lock.BLUE_CARD
    # Bit 9 makes skull and card of the same color equivalent
    assert classify_line(special | 0x200).lock is Lock.BLUE
    # Key value 7 = all keys; with bit 9 = one of each color
    assert classify_line(0x3800 | (7 << 6)).lock is Lock.ALL_SIX
    assert classify_line(0x3800 | (7 << 6) | 0x200).lock is Lock.ALL_COLORS


def test_generalized_ranges():
    assert classify_line(0x6000 | 3).category is Category.FLOOR
    assert classify_line(0x6000 | 3).trigger is Trigger.SR
    assert classify_line(0x4000).category is Category.CEILING
    door = classify_line(0x3C00 | 0x80 | 1)
    assert door.category is Category.DOOR and door.monsters and door.trigger is Trigger.WR
    assert classify_line(0x3400 | 0x20).category is Category.LIFT
    assert classify_line(0x3000).category is Category.STAIRS
    assert classify_line(0x2F80).category is Category.CRUSHER


def test_lock_satisfaction():
    assert Lock.NONE.satisfied(set())
    assert Lock.RED.satisfied({Key.RED_SKULL})
    assert Lock.RED.satisfied({Key.RED_CARD})
    assert not Lock.RED_CARD.satisfied({Key.RED_SKULL})
    assert not Lock.ANY.satisfied(set())
    assert Lock.ANY.satisfied({Key.YELLOW_SKULL})
    assert Lock.ALL_COLORS.satisfied({Key.RED_SKULL, Key.BLUE_CARD, Key.YELLOW_CARD})
    assert not Lock.ALL_SIX.satisfied({Key.RED_SKULL, Key.BLUE_CARD, Key.YELLOW_CARD})
    assert Lock.ALL_SIX.satisfied(set(Key))


def test_sector_specials():
    assert classify_sector(9).secret
    assert classify_sector(5).damage == 10
    assert classify_sector(7).damage == 5
    e1m8 = classify_sector(11)
    assert e1m8.damage == 20 and e1m8.ends_level
    # Boom generalized: 20% damage (3 << 5) + secret bit
    boom = classify_sector((3 << 5) | 0x80)
    assert boom.damage == 20 and boom.secret

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Quarkboy

//! Linedef and sector specials in the terms navigation cares about.
//!
//! Sources: the vanilla linedef type table (1-141), the Boom reference for generalized linedef
//! bitfields (0x2F80-0x7FFF) and generalized sector specials. Boom extended types (142+) are
//! only partly classified: exits and teleporters, which matter for navigation; everything
//! else in that range is `Category::Other`.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Trigger {
    /// walk over, once
    W1,
    WR,
    /// switch (use), once
    S1,
    SR,
    /// shoot, once
    G1,
    GR,
    /// door-style use on the line itself, once
    D1,
    DR,
    /// passive, map setup (scrollers, transfers)
    None,
}

impl Trigger {
    pub fn repeatable(self) -> bool {
        matches!(self, Trigger::WR | Trigger::SR | Trigger::GR | Trigger::DR)
    }

    /// once only: gone once set off
    pub fn once(self) -> bool {
        matches!(self, Trigger::W1 | Trigger::S1 | Trigger::G1 | Trigger::D1)
    }

    pub fn walked(self) -> bool {
        matches!(self, Trigger::W1 | Trigger::WR)
    }

    pub fn used(self) -> bool {
        matches!(self, Trigger::S1 | Trigger::SR | Trigger::D1 | Trigger::DR)
    }

    pub fn shot(self) -> bool {
        matches!(self, Trigger::G1 | Trigger::GR)
    }

    pub fn name(self) -> &'static str {
        match self {
            Trigger::W1 => "W1",
            Trigger::WR => "WR",
            Trigger::S1 => "S1",
            Trigger::SR => "SR",
            Trigger::G1 => "G1",
            Trigger::GR => "GR",
            Trigger::D1 => "D1",
            Trigger::DR => "DR",
            Trigger::None => "--",
        }
    }
}

const GEN_TRIGGERS: [Trigger; 8] = [
    Trigger::W1,
    Trigger::WR,
    Trigger::S1,
    Trigger::SR,
    Trigger::G1,
    Trigger::GR,
    Trigger::D1,
    Trigger::DR,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Category {
    Door,
    LockedDoor,
    Lift,
    Floor,
    Ceiling,
    Crusher,
    Stairs,
    Exit,
    SecretExit,
    Teleport,
    Light,
    Other,
}

impl Category {
    pub fn is_exit(self) -> bool {
        matches!(self, Category::Exit | Category::SecretExit)
    }

    pub fn is_door(self) -> bool {
        matches!(self, Category::Door | Category::LockedDoor)
    }
}

/// Key items, by their thing type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Key {
    BlueCard = 5,
    YellowCard = 6,
    RedCard = 13,
    RedSkull = 38,
    YellowSkull = 39,
    BlueSkull = 40,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    Red,
    Blue,
    Yellow,
}

impl Key {
    pub const ALL: [Key; 6] = [
        Key::BlueCard,
        Key::YellowCard,
        Key::RedCard,
        Key::RedSkull,
        Key::YellowSkull,
        Key::BlueSkull,
    ];

    pub fn from_thing(kind: u16) -> Option<Key> {
        Key::ALL.into_iter().find(|&k| k as u16 == kind)
    }

    pub fn color(self) -> Color {
        match self {
            Key::BlueCard | Key::BlueSkull => Color::Blue,
            Key::YellowCard | Key::YellowSkull => Color::Yellow,
            Key::RedCard | Key::RedSkull => Color::Red,
        }
    }

    /// As Python's enum names it ("BLUE_SKULL"): the logs say so.
    pub fn name(self) -> &'static str {
        match self {
            Key::BlueCard => "BLUE_CARD",
            Key::YellowCard => "YELLOW_CARD",
            Key::RedCard => "RED_CARD",
            Key::RedSkull => "RED_SKULL",
            Key::YellowSkull => "YELLOW_SKULL",
            Key::BlueSkull => "BLUE_SKULL",
        }
    }

    /// The order the engine reports keys held in (blue card, yellow card, red card, blue
    /// skull, yellow skull, red skull).
    pub const ENGINE_ORDER: [Key; 6] = [
        Key::BlueCard,
        Key::YellowCard,
        Key::RedCard,
        Key::BlueSkull,
        Key::YellowSkull,
        Key::RedSkull,
    ];
}

/// A set of keys held, as bits by `Key::ALL`'s order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Keys(pub u8);

impl Keys {
    fn bit(k: Key) -> u8 {
        1 << Key::ALL.iter().position(|&x| x == k).unwrap_or(0)
    }

    pub fn has(self, k: Key) -> bool {
        self.0 & Self::bit(k) != 0
    }

    pub fn with(self, k: Key) -> Keys {
        Keys(self.0 | Self::bit(k))
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Every key in `other` is held.
    pub fn covers(self, other: Keys) -> bool {
        other.0 & !self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Key> {
        Key::ALL.into_iter().filter(move |&k| self.has(k))
    }

    pub fn has_color(self, c: Color) -> bool {
        self.iter().any(|k| k.color() == c)
    }
}

impl FromIterator<Key> for Keys {
    fn from_iter<I: IntoIterator<Item = Key>>(it: I) -> Self {
        it.into_iter().fold(Keys::default(), Keys::with)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lock {
    None,
    /// either red key (vanilla)
    Red,
    Blue,
    Yellow,
    RedCard,
    BlueCard,
    YellowCard,
    RedSkull,
    BlueSkull,
    YellowSkull,
    /// any one key
    Any,
    /// one key of each color
    AllColors,
    AllSix,
}

impl Lock {
    pub fn satisfied(self, keys: Keys) -> bool {
        match self {
            Lock::None => true,
            Lock::Red => keys.has_color(Color::Red),
            Lock::Blue => keys.has_color(Color::Blue),
            Lock::Yellow => keys.has_color(Color::Yellow),
            Lock::Any => !keys.is_empty(),
            Lock::AllColors => [Color::Red, Color::Blue, Color::Yellow]
                .into_iter()
                .all(|c| keys.has_color(c)),
            Lock::AllSix => keys.covers(Key::ALL.into_iter().collect()),
            Lock::RedCard => keys.has(Key::RedCard),
            Lock::BlueCard => keys.has(Key::BlueCard),
            Lock::YellowCard => keys.has(Key::YellowCard),
            Lock::RedSkull => keys.has(Key::RedSkull),
            Lock::BlueSkull => keys.has(Key::BlueSkull),
            Lock::YellowSkull => keys.has(Key::YellowSkull),
        }
    }

    /// As Python's enum values have it ("red", "all_six"): generalized locked doors'
    /// descriptions say so.
    pub fn value(self) -> &'static str {
        match self {
            Lock::None => "none",
            Lock::Red => "red",
            Lock::Blue => "blue",
            Lock::Yellow => "yellow",
            Lock::RedCard => "red_card",
            Lock::BlueCard => "blue_card",
            Lock::YellowCard => "yellow_card",
            Lock::RedSkull => "red_skull",
            Lock::BlueSkull => "blue_skull",
            Lock::YellowSkull => "yellow_skull",
            Lock::Any => "any",
            Lock::AllColors => "all_colors",
            Lock::AllSix => "all_six",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LineSpecial {
    pub special: u16,
    pub trigger: Trigger,
    pub category: Category,
    pub description: String,
    pub lock: Lock,
    /// can monsters set it off
    pub monsters: bool,
    /// can players set it off
    pub players: bool,
    pub generalized: bool,
}

impl fmt::Display for LineSpecial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.description)
    }
}

use Category as C;
use Trigger as T;

/// special: trigger, category, description, lock, monsters, players
type Entry = (u16, Trigger, Category, &'static str, Lock, bool, bool);

const fn e(special: u16, t: Trigger, c: Category, d: &'static str) -> Entry {
    (special, t, c, d, Lock::None, false, true)
}

const fn m(special: u16, t: Trigger, c: Category, d: &'static str) -> Entry {
    (special, t, c, d, Lock::None, true, true)
}

const fn k(special: u16, t: Trigger, c: Category, d: &'static str, lock: Lock) -> Entry {
    (special, t, c, d, lock, false, true)
}

#[rustfmt::skip]
const TABLE: &[Entry] = &[
    m(1, T::DR, C::Door, "door open-wait-close"),
    e(2, T::W1, C::Door, "door open stay"),
    e(3, T::W1, C::Door, "door close"),
    m(4, T::W1, C::Door, "door open-wait-close"),
    e(5, T::W1, C::Floor, "floor raise to lowest ceiling"),
    e(6, T::W1, C::Crusher, "fast crusher"),
    e(7, T::S1, C::Stairs, "build stairs 8"),
    e(8, T::W1, C::Stairs, "build stairs 8"),
    e(9, T::S1, C::Floor, "donut"),
    m(10, T::W1, C::Lift, "lift"),
    e(11, T::S1, C::Exit, "exit level"),
    e(12, T::W1, C::Light, "light to brightest neighbor"),
    e(13, T::W1, C::Light, "light to 255"),
    e(14, T::S1, C::Floor, "floor raise 32, change texture"),
    e(15, T::S1, C::Floor, "floor raise 24, change texture"),
    e(16, T::W1, C::Door, "door close-wait-open"),
    e(17, T::W1, C::Light, "light start blinking"),
    e(18, T::S1, C::Floor, "floor raise to next higher"),
    e(19, T::W1, C::Floor, "floor lower to highest neighbor"),
    e(20, T::S1, C::Floor, "floor raise to next higher, change texture"),
    e(21, T::S1, C::Lift, "lift"),
    e(22, T::W1, C::Floor, "floor raise to next higher, change texture"),
    e(23, T::S1, C::Floor, "floor lower to lowest"),
    e(24, T::G1, C::Floor, "floor raise to lowest ceiling"),
    e(25, T::W1, C::Crusher, "slow crusher"),
    k(26, T::DR, C::LockedDoor, "blue door open-wait-close", Lock::Blue),
    k(27, T::DR, C::LockedDoor, "yellow door open-wait-close", Lock::Yellow),
    k(28, T::DR, C::LockedDoor, "red door open-wait-close", Lock::Red),
    e(29, T::S1, C::Door, "door open-wait-close"),
    e(30, T::W1, C::Floor, "floor raise by shortest lower texture"),
    e(31, T::D1, C::Door, "door open stay"),
    k(32, T::D1, C::LockedDoor, "blue door open stay", Lock::Blue),
    k(33, T::D1, C::LockedDoor, "red door open stay", Lock::Red),
    k(34, T::D1, C::LockedDoor, "yellow door open stay", Lock::Yellow),
    e(35, T::W1, C::Light, "light to 35"),
    e(36, T::W1, C::Floor, "floor turbo lower to highest neighbor + 8"),
    e(37, T::W1, C::Floor, "floor lower to lowest, change texture"),
    e(38, T::W1, C::Floor, "floor lower to lowest"),
    m(39, T::W1, C::Teleport, "teleport"),
    e(40, T::W1, C::Ceiling, "ceiling raise to highest"),
    e(41, T::S1, C::Ceiling, "ceiling lower to floor"),
    e(42, T::SR, C::Door, "door close"),
    e(43, T::SR, C::Ceiling, "ceiling lower to floor"),
    e(44, T::W1, C::Ceiling, "ceiling lower to 8 above floor"),
    e(45, T::SR, C::Floor, "floor lower to highest neighbor"),
    e(46, T::GR, C::Door, "door open stay"),
    e(47, T::G1, C::Floor, "floor raise to next higher, change texture"),
    e(48, T::None, C::Other, "scroll wall left"),
    e(49, T::S1, C::Crusher, "slow crusher"),
    e(50, T::S1, C::Door, "door close"),
    e(51, T::S1, C::SecretExit, "secret exit"),
    e(52, T::W1, C::Exit, "exit level"),
    e(53, T::W1, C::Lift, "perpetual lift start"),
    e(54, T::W1, C::Lift, "perpetual lift stop"),
    e(55, T::S1, C::Floor, "floor raise to lowest ceiling - 8, crush"),
    e(56, T::W1, C::Floor, "floor raise to lowest ceiling - 8, crush"),
    e(57, T::W1, C::Crusher, "crusher stop"),
    e(58, T::W1, C::Floor, "floor raise 24"),
    e(59, T::W1, C::Floor, "floor raise 24, change texture"),
    e(60, T::SR, C::Floor, "floor lower to lowest"),
    e(61, T::SR, C::Door, "door open stay"),
    e(62, T::SR, C::Lift, "lift"),
    e(63, T::SR, C::Door, "door open-wait-close"),
    e(64, T::SR, C::Floor, "floor raise to lowest ceiling"),
    e(65, T::SR, C::Floor, "floor raise to lowest ceiling - 8, crush"),
    e(66, T::SR, C::Floor, "floor raise 24, change texture"),
    e(67, T::SR, C::Floor, "floor raise 32, change texture"),
    e(68, T::SR, C::Floor, "floor raise to next higher, change texture"),
    e(69, T::SR, C::Floor, "floor raise to next higher"),
    e(70, T::SR, C::Floor, "floor turbo lower to highest neighbor + 8"),
    e(71, T::S1, C::Floor, "floor turbo lower to highest neighbor + 8"),
    e(72, T::WR, C::Ceiling, "ceiling lower to 8 above floor"),
    e(73, T::WR, C::Crusher, "slow crusher"),
    e(74, T::WR, C::Crusher, "crusher stop"),
    e(75, T::WR, C::Door, "door close"),
    e(76, T::WR, C::Door, "door close-wait-open"),
    e(77, T::WR, C::Crusher, "fast crusher"),
    e(79, T::WR, C::Light, "light to 35"),
    e(80, T::WR, C::Light, "light to brightest neighbor"),
    e(81, T::WR, C::Light, "light to 255"),
    e(82, T::WR, C::Floor, "floor lower to lowest"),
    e(83, T::WR, C::Floor, "floor lower to highest neighbor"),
    e(84, T::WR, C::Floor, "floor lower to lowest, change texture"),
    e(86, T::WR, C::Door, "door open stay"),
    e(87, T::WR, C::Lift, "perpetual lift start"),
    m(88, T::WR, C::Lift, "lift"),
    e(89, T::WR, C::Lift, "perpetual lift stop"),
    e(90, T::WR, C::Door, "door open-wait-close"),
    e(91, T::WR, C::Floor, "floor raise to lowest ceiling"),
    e(92, T::WR, C::Floor, "floor raise 24"),
    e(93, T::WR, C::Floor, "floor raise 24, change texture"),
    e(94, T::WR, C::Floor, "floor raise to lowest ceiling - 8, crush"),
    e(95, T::WR, C::Floor, "floor raise to next higher, change texture"),
    e(96, T::WR, C::Floor, "floor raise by shortest lower texture"),
    m(97, T::WR, C::Teleport, "teleport"),
    e(98, T::WR, C::Floor, "floor turbo lower to highest neighbor + 8"),
    k(99, T::SR, C::LockedDoor, "blue blazing door open stay", Lock::Blue),
    e(100, T::W1, C::Stairs, "turbo build stairs 16"),
    e(101, T::S1, C::Floor, "floor raise to lowest ceiling"),
    e(102, T::S1, C::Floor, "floor lower to highest neighbor"),
    e(103, T::S1, C::Door, "door open stay"),
    e(104, T::W1, C::Light, "light to darkest neighbor"),
    e(105, T::WR, C::Door, "blazing door open-wait-close"),
    e(106, T::WR, C::Door, "blazing door open stay"),
    e(107, T::WR, C::Door, "blazing door close"),
    e(108, T::W1, C::Door, "blazing door open-wait-close"),
    e(109, T::W1, C::Door, "blazing door open stay"),
    e(110, T::W1, C::Door, "blazing door close"),
    e(111, T::S1, C::Door, "blazing door open-wait-close"),
    e(112, T::S1, C::Door, "blazing door open stay"),
    e(113, T::S1, C::Door, "blazing door close"),
    e(114, T::SR, C::Door, "blazing door open-wait-close"),
    e(115, T::SR, C::Door, "blazing door open stay"),
    e(116, T::SR, C::Door, "blazing door close"),
    e(117, T::DR, C::Door, "blazing door open-wait-close"),
    e(118, T::D1, C::Door, "blazing door open stay"),
    e(119, T::W1, C::Floor, "floor raise to next higher"),
    e(120, T::WR, C::Lift, "blazing lift"),
    e(121, T::W1, C::Lift, "blazing lift"),
    e(122, T::S1, C::Lift, "blazing lift"),
    e(123, T::SR, C::Lift, "blazing lift"),
    e(124, T::W1, C::SecretExit, "secret exit"),
    (125, T::W1, C::Teleport, "teleport (monsters only)", Lock::None, true, false),
    (126, T::WR, C::Teleport, "teleport (monsters only)", Lock::None, true, false),
    e(127, T::S1, C::Stairs, "turbo build stairs 16"),
    e(128, T::WR, C::Floor, "floor raise to next higher"),
    e(129, T::WR, C::Floor, "floor turbo raise to next higher"),
    e(130, T::W1, C::Floor, "floor turbo raise to next higher"),
    e(131, T::S1, C::Floor, "floor turbo raise to next higher"),
    e(132, T::SR, C::Floor, "floor turbo raise to next higher"),
    k(133, T::S1, C::LockedDoor, "blue blazing door open stay", Lock::Blue),
    k(134, T::SR, C::LockedDoor, "red blazing door open stay", Lock::Red),
    k(135, T::S1, C::LockedDoor, "red blazing door open stay", Lock::Red),
    k(136, T::SR, C::LockedDoor, "yellow blazing door open stay", Lock::Yellow),
    k(137, T::S1, C::LockedDoor, "yellow blazing door open stay", Lock::Yellow),
    e(138, T::SR, C::Light, "light to 255"),
    e(139, T::SR, C::Light, "light to 35"),
    e(140, T::S1, C::Floor, "floor raise 512"),
    e(141, T::W1, C::Crusher, "silent crusher"),
    // Boom extended types that matter for getting about
    m(174, T::S1, C::Teleport, "teleport"),
    m(195, T::SR, C::Teleport, "teleport"),
    e(197, T::G1, C::Exit, "exit level"),
    e(198, T::G1, C::SecretExit, "secret exit"),
    m(207, T::W1, C::Teleport, "silent teleport"),
    m(208, T::WR, C::Teleport, "silent teleport"),
    m(209, T::S1, C::Teleport, "silent teleport"),
    m(210, T::SR, C::Teleport, "silent teleport"),
    m(243, T::W1, C::Teleport, "silent line-to-line teleport"),
    m(244, T::WR, C::Teleport, "silent line-to-line teleport"),
];

const GEN_LOCKS: [Lock; 8] = [
    Lock::Any,
    Lock::RedCard,
    Lock::BlueCard,
    Lock::YellowCard,
    Lock::RedSkull,
    Lock::BlueSkull,
    Lock::YellowSkull,
    Lock::AllSix,
];
const SPEEDS: [&str; 4] = ["slow", "normal", "fast", "turbo"];

/// A lock that a card or a skull of its color opens alike.
fn skull_is_card(lock: Lock) -> Lock {
    match lock {
        Lock::RedCard | Lock::RedSkull => Lock::Red,
        Lock::BlueCard | Lock::BlueSkull => Lock::Blue,
        Lock::YellowCard | Lock::YellowSkull => Lock::Yellow,
        Lock::AllSix => Lock::AllColors,
        other => other,
    }
}

fn generalized(special: u16) -> Option<LineSpecial> {
    let trigger = GEN_TRIGGERS[usize::from(special & 7)];
    let speed = SPEEDS[usize::from((special >> 3) & 3)];
    let make = |category, desc: String, lock, monsters| LineSpecial {
        special,
        trigger,
        category,
        description: format!("{speed} {desc}"),
        lock,
        monsters,
        players: true,
        generalized: true,
    };
    if special > 0x7FFF {
        return None;
    }
    if special >= 0x4000 {
        let is_floor = special >= 0x6000;
        let change = (special >> 10) & 3;
        let monsters = change == 0 && special & 0x20 != 0;
        let direction = if special & 0x40 != 0 { "up" } else { "down" };
        let crush = if special & 0x1000 != 0 { ", crush" } else { "" };
        return Some(if is_floor {
            make(
                Category::Floor,
                format!("floor move {direction}{crush}"),
                Lock::None,
                monsters,
            )
        } else {
            make(
                Category::Ceiling,
                format!("ceiling move {direction}{crush}"),
                Lock::None,
                monsters,
            )
        });
    }
    if special >= 0x3C00 {
        let kind = ["open-wait-close", "open stay", "close-wait-open", "close"]
            [usize::from((special >> 5) & 3)];
        return Some(make(
            Category::Door,
            format!("door {kind}"),
            Lock::None,
            special & 0x80 != 0,
        ));
    }
    if special >= 0x3800 {
        let kind = if special & 0x20 != 0 {
            "open stay"
        } else {
            "open-wait-close"
        };
        let mut lock = GEN_LOCKS[usize::from((special >> 6) & 7)];
        if special & 0x200 != 0 {
            lock = skull_is_card(lock);
        }
        return Some(make(
            Category::LockedDoor,
            format!("{} locked door {kind}", lock.value()),
            lock,
            false,
        ));
    }
    if special >= 0x3400 {
        return Some(make(
            Category::Lift,
            "lift".into(),
            Lock::None,
            special & 0x20 != 0,
        ));
    }
    if special >= 0x3000 {
        return Some(make(
            Category::Stairs,
            "build stairs".into(),
            Lock::None,
            special & 0x20 != 0,
        ));
    }
    if special >= 0x2F80 {
        return Some(make(
            Category::Crusher,
            "crusher".into(),
            Lock::None,
            special & 0x20 != 0,
        ));
    }
    None
}

/// What a linedef special is; None for special 0.
pub fn classify_line(special: u16) -> Option<LineSpecial> {
    if special == 0 {
        return None;
    }
    if special >= 0x2F80 {
        if let Some(gen) = generalized(special) {
            return Some(gen);
        }
    }
    Some(match TABLE.iter().find(|row| row.0 == special) {
        Some(&(_, trigger, category, description, lock, monsters, players)) => LineSpecial {
            special,
            trigger,
            category,
            description: description.to_string(),
            lock,
            monsters,
            players,
            generalized: false,
        },
        None => LineSpecial {
            special,
            trigger: Trigger::None,
            category: Category::Other,
            description: format!("unclassified special {special}"),
            lock: Lock::None,
            monsters: false,
            players: true,
            generalized: false,
        },
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectorInfo {
    /// health a damage tic (0, 5, 10, 20)
    pub damage: i32,
    pub secret: bool,
    /// an exit floor: hurts, and the level ends when health drops below 11
    pub ends_level: bool,
}

/// Vanilla sector specials are bits 0-4; Boom adds damage, secret and friction bits.
pub fn classify_sector(special: u16) -> SectorInfo {
    let base = special & 0x1F;
    let vanilla = match base {
        4 | 11 | 16 => 20,
        5 => 10,
        7 => 5,
        _ => 0,
    };
    let boom = [0, 5, 10, 20][usize::from((special >> 5) & 3)];
    SectorInfo {
        damage: vanilla.max(boom),
        secret: base == 9 || special & 0x80 != 0,
        ends_level: base == 11,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vanilla_and_generalized() {
        let d = classify_line(26).unwrap();
        assert_eq!(
            (d.trigger, d.category, d.lock),
            (Trigger::DR, Category::LockedDoor, Lock::Blue)
        );
        assert!(classify_line(1).unwrap().monsters);
        assert!(!classify_line(125).unwrap().players);
        assert_eq!(classify_line(9999).unwrap().category, Category::Other);
        // a generalized door: fast, open stay, SR
        let g = classify_line(0x3C00 | (2 << 3) | (1 << 5) | 3).unwrap();
        assert_eq!(g.description, "fast door open stay");
        assert_eq!(g.trigger, Trigger::SR);
    }

    #[test]
    fn locks() {
        let blue_skull: Keys = [Key::BlueSkull].into_iter().collect();
        assert!(Lock::Blue.satisfied(blue_skull));
        assert!(!Lock::BlueCard.satisfied(blue_skull));
        assert!(!Lock::AllColors.satisfied(blue_skull));
        assert!(Lock::Any.satisfied(blue_skull));
        assert!(Lock::AllSix.satisfied(Key::ALL.into_iter().collect()));
    }

    #[test]
    fn sectors() {
        assert!(classify_sector(9).secret);
        assert_eq!(classify_sector(7).damage, 5);
        assert!(classify_sector(11).ends_level);
        assert_eq!(classify_sector(0x80 | (3 << 5)).damage, 20);
    }
}

/// A crusher's kind, as its line's special has it (DSDA's p_ceilng.c): how fast it comes down
/// (units a tic), and whether it slows to an eighth of that on something under it (vanilla's
/// slow ones, and Boom's generalized ones under 3x their speed: then it holds her there till
/// she's dead) or keeps on through (the fast ones: one stroke through her).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CrusherKind {
    pub speed: f64,
    pub holds: bool,
}

/// The kind of crusher a line's special starts; None for anything else (a crusher-stop line too).
pub fn crusher_kind(special: u16) -> Option<CrusherKind> {
    match special {
        6 | 77 => Some(CrusherKind {
            speed: 2.0,
            holds: false,
        }),
        25 | 49 | 73 | 141 => Some(CrusherKind {
            speed: 1.0,
            holds: true,
        }),
        0x2F80..0x3000 => {
            let code = (special & 0x18) >> 3;
            Some(CrusherKind {
                speed: f64::from(1u16 << code),
                holds: code <= 1,
            })
        }
        _ => None,
    }
}

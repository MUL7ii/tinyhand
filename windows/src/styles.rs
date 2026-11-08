use crate::arm::Arm;
use crate::hand::HandAsset;

pub enum Style {
    Laser,
    Hand(Arm),
}

pub const LASER_INK: u32 = 0xFF291F;
pub const LASER: &str = "laser";

const PURPLE: u32 = 0x8754F2;
const RED: u32 = LASER_INK;

/// id, name shown in settings, ink color
const HANDS: [(&str, &str, u32); 36] = [
    ("lavender", "Lavender toy", PURPLE),
    ("glove", "Cursor glove", PURPLE),
    ("coral", "Coral vinyl", RED),
    ("plush", "Plush", 0x2F86F0),
    ("robot", "Robot", 0x16A87A),
    ("pink", "Bubblegum", 0xEE4F98),
    ("blue", "Sky", 0x2F86F0),
    ("mint", "Mint", 0x16A87A),
    ("lemon", "Lemon", 0xF2B705),
    ("black", "Midnight", 0x1D1D21),
    ("chrome", "Chrome", 0x6B7380),
    ("holo", "Holo", PURPLE),
    ("fuzzy", "Fuzzy", PURPLE),
    ("wood", "Wood", 0x9A5E2C),
    ("marble", "Marble", 0x6B7380),
    ("terrazzo", "Terrazzo", 0xF0561D),
    ("glass", "Glass", 0x1597E5),
    ("jelly", "Jelly", 0xA33FE6),
    ("pastel", "Pastel", 0xEE4F98),
    ("pixel", "Pixel", 0x1D1D21),
    ("lowpoly", "Low poly", PURPLE),
    ("knit", "Knit", PURPLE),
    ("skeleton", "Skeleton", 0x9A5E2C),
    ("astro", "Astronaut", 0x2F86F0),
    ("space", "Space", RED),
    ("monster", "Monster", 0x3E9E2E),
    ("water", "Water", 0x1597E5),
    ("lava", "Lava", 0xF0561D),
    ("grass", "Grass", 0x3E9E2E),
    ("cactus", "Cactus", 0x3E9E2E),
    ("choco", "Chocolate", 0x9A5E2C),
    ("cloud", "Cloud", 0x2F86F0),
    ("mech", "Mech", 0xF0561D),
    ("neon", "Neon", 0xE0309A),
    ("leaf", "Leaf", 0x3E9E2E),
    ("cookie", "Gingerbread", 0x9A5E2C),
];

/// Every pointer a user can pick, the laser last.
pub fn all() -> impl Iterator<Item = (&'static str, &'static str)> {
    HANDS.iter().map(|(id, name, _)| (*id, *name)).chain(std::iter::once((LASER, "Laser dot")))
}

pub fn is_known(id: &str) -> bool {
    id == LASER || HANDS.iter().any(|(h, _, _)| *h == id)
}

/// Unknown or unloadable hands fall back to the laser, like the Mac app.
pub fn load(id: &str) -> (Style, u32) {
    let Some(&(_, _, ink)) = HANDS.iter().find(|(name, _, _)| *name == id) else { return (Style::Laser, LASER_INK) };
    match HandAsset::load(id) {
        Some(asset) => (Style::Hand(Arm::new(asset)), ink),
        None => (Style::Laser, LASER_INK),
    }
}

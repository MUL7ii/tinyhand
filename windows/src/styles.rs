use crate::arm::Arm;
use crate::hand::HandAsset;

pub enum Style {
    Laser,
    Hand(Arm),
}

pub const LASER_INK: u32 = 0xFF291F;

const PURPLE: u32 = 0x8754F2;
const RED: u32 = LASER_INK;

const HANDS: [(&str, u32); 36] = [
    ("lavender", PURPLE),
    ("glove", PURPLE),
    ("coral", RED),
    ("plush", 0x2F86F0),
    ("robot", 0x16A87A),
    ("pink", 0xEE4F98),
    ("blue", 0x2F86F0),
    ("mint", 0x16A87A),
    ("lemon", 0xF2B705),
    ("black", 0x1D1D21),
    ("chrome", 0x6B7380),
    ("holo", PURPLE),
    ("fuzzy", PURPLE),
    ("wood", 0x9A5E2C),
    ("marble", 0x6B7380),
    ("terrazzo", 0xF0561D),
    ("glass", 0x1597E5),
    ("jelly", 0xA33FE6),
    ("pastel", 0xEE4F98),
    ("pixel", 0x1D1D21),
    ("lowpoly", PURPLE),
    ("knit", PURPLE),
    ("skeleton", 0x9A5E2C),
    ("astro", 0x2F86F0),
    ("space", RED),
    ("monster", 0x3E9E2E),
    ("water", 0x1597E5),
    ("lava", 0xF0561D),
    ("grass", 0x3E9E2E),
    ("cactus", 0x3E9E2E),
    ("choco", 0x9A5E2C),
    ("cloud", 0x2F86F0),
    ("mech", 0xF0561D),
    ("neon", 0xE0309A),
    ("leaf", 0x3E9E2E),
    ("cookie", 0x9A5E2C),
];

/// Unknown or unloadable hands fall back to the laser, like the Mac app.
pub fn load(id: &str) -> (Style, u32) {
    let Some(&(_, ink)) = HANDS.iter().find(|(name, _)| *name == id) else { return (Style::Laser, LASER_INK) };
    match HandAsset::load(id) {
        Some(asset) => (Style::Hand(Arm::new(asset)), ink),
        None => (Style::Laser, LASER_INK),
    }
}

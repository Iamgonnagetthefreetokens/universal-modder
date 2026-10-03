//! Name generation for villages, kingdoms and their rulers.
//!
//! Syllable tables per cultural flavour. Deterministic: everything is drawn from
//! the world RNG, so the same seed always produces the same map of names.

use crate::races::Race;
use crate::rng::Rng;

const NEUTRAL_A: [&str; 16] = [
    "Ald", "Bri", "Cal", "Dun", "Elor", "Fen", "Grim", "Hal", "Kor", "Mor", "Nar", "Orin", "Rav",
    "Thal", "Ves", "Wes",
];
const NEUTRAL_B: [&str; 16] = [
    "holm", "mark", "gard", "fell", "stead", "reach", "moor", "wick", "hold", "burg", "vale",
    "cliff", "haven", "ridge", "ford", "gate",
];
const ELF_A: [&str; 12] = [
    "Ael", "Syl", "Lor", "Ithi", "Nae", "Thal", "Yen", "Cael", "Elen", "Mir", "Vael", "Orin",
];
const ELF_B: [&str; 12] = [
    "loth", "doriel", "wen", "thalas", "ion", "mere", "sari", "anor", "linn", "riel", "viel",
    "duin",
];
const DWARF_A: [&str; 12] = [
    "Dur", "Khaz", "Grim", "Borin", "Thrum", "Krag", "Bal", "Dain", "Ulf", "Gorm", "Har", "Sten",
];
const DWARF_B: [&str; 12] = [
    "forge", "hold", "deep", "delving", "gate", "hall", "anvil", "mine", "beard", "stone",
    "crag", "barrow",
];
const ORC_A: [&str; 12] = [
    "Grok", "Urz", "Mor", "Skar", "Krug", "Zag", "Nash", "Drak", "Gul", "Rot", "Krak", "Brug",
];
const ORC_B: [&str; 12] = [
    "mash", "gul", "tusk", "claw", "fang", "skull", "gore", "bile", "grind", "snarl", "howl",
    "crush",
];
const BANDIT_A: [&str; 10] = [
    "Red", "Black", "Iron", "Grey", "Bone", "Ash", "Copper", "Salt", "Rust", "Crow",
];
const BANDIT_B: [&str; 10] = [
    "fang", "claw", "blade", "mark", "hand", "crew", "pack", "den", "roost", "camp",
];
const TITLE: [&str; 12] = [
    "King", "Queen", "Chief", "Warlord", "Lord", "Lady", "Elder", "Thane", "Duke", "Matriarch",
    "Patriarch", "Captain",
];
const KINGDOM_FORM: [&str; 8] = [
    "Kingdom of {n}",
    "Realm of {n}",
    "Dominion of {n}",
    "Clan of {n}",
    "League of {n}",
    "Hold of {n}",
    "Dominion of {n}",
    "Free Cities of {n}",
];
const PERSONAL_A: [&str; 16] = [
    "Aar", "Bel", "Cor", "Dag", "Ea", "Fal", "Gor", "Hed", "Ing", "Jor", "Kel", "Lom", "Mir",
    "Nor", "Oth", "Rag",
];
const PERSONAL_B: [&str; 16] = [
    "in", "ek", "as", "un", "or", "el", "ath", "ur", "iel", "and", "oth", "wyn", "grim", "vald",
    "ric", "mar",
];

fn table_prefix(race: Race) -> &'static [&'static str] {
    match race {
        Race::Elf => &ELF_A,
        Race::Dwarf => &DWARF_A,
        Race::Orc => &ORC_A,
        Race::Bandit => &BANDIT_A,
        _ => &NEUTRAL_A,
    }
}

fn table_suffix(race: Race) -> &'static [&'static str] {
    match race {
        Race::Elf => &ELF_B,
        Race::Dwarf => &DWARF_B,
        Race::Orc => &ORC_B,
        Race::Bandit => &BANDIT_B,
        _ => &NEUTRAL_B,
    }
}

/// A village name in the race's flavour, e.g. `Khazforge`, `Aelwen`, `Aldstead`.
pub fn village_name(rng: &mut Rng, race: Race) -> String {
    let a = rng.pick_copy(table_prefix(race));
    let b = rng.pick_copy(table_suffix(race));
    let mut s = String::with_capacity(a.len() + b.len());
    s.push_str(a);
    s.push_str(b);
    s
}

/// A kingdom name, e.g. `Kingdom of Aelwen`.
pub fn kingdom_name(rng: &mut Rng, race: Race, capital: &str) -> String {
    // Half of all kingdoms are named after their capital, half after their people.
    if rng.chance(0.5) {
        let form = rng.pick_copy(&KINGDOM_FORM);
        form.replace("{n}", capital)
    } else {
        let root = village_name(rng, race);
        format!("{root} Empire")
    }
}

/// A personal name for a leader or king.
pub fn person_name(rng: &mut Rng) -> String {
    let a = rng.pick_copy(&PERSONAL_A);
    let b = rng.pick_copy(&PERSONAL_B);
    format!("{a}{b}")
}

/// A ruler's full title, e.g. `Queen Aelin of Aelwen`.
pub fn ruler_name(rng: &mut Rng, village: &str) -> String {
    let title = rng.pick_copy(&TITLE);
    let person = person_name(rng);
    format!("{title} {person} of {village}")
}

/// A culture string for a kingdom, used in war/peace flavour text.
pub fn motto(rng: &mut Rng) -> &'static str {
    rng.pick_copy(&[
        "Steel before words",
        "We remember",
        "Root and stone",
        "From the ashes",
        "By sea and star",
        "Nothing is given",
        "The land provides",
        "Fire answers fire",
        "Blood and iron",
        "First to rise",
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_deterministic_per_seed() {
        let mut a = Rng::new(5);
        let mut b = Rng::new(5);
        assert_eq!(
            village_name(&mut a, Race::Dwarf),
            village_name(&mut b, Race::Dwarf)
        );
        assert_eq!(person_name(&mut a), person_name(&mut b));
    }

    #[test]
    fn names_look_like_names() {
        let mut rng = Rng::new(77);
        for race in [Race::Human, Race::Elf, Race::Dwarf, Race::Orc, Race::Bandit] {
            for _ in 0..50 {
                let n = village_name(&mut rng, race);
                assert!(n.len() >= 4, "{n} is too short");
                assert!(n.chars().all(|c| c.is_ascii_alphabetic()), "{n}");
                assert!(n.chars().next().unwrap().is_ascii_uppercase(), "{n}");
            }
        }
        let k = kingdom_name(&mut rng, Race::Orc, "Grokmash");
        assert!(!k.is_empty());
        let r = ruler_name(&mut rng, "Grokmash");
        assert!(r.contains(" of Grokmash"));
    }

    #[test]
    fn village_names_vary() {
        let mut rng = Rng::new(9);
        let mut seen = std::collections::BTreeSet::new();
        for _ in 0..100 {
            seen.insert(village_name(&mut rng, Race::Human));
        }
        assert!(seen.len() > 20, "only {} distinct names", seen.len());
    }
}

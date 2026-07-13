use crate::rng::Pcg32;

const ADJECTIVES: [&str; 16] = [
    "Amber", "Iron", "Crimson", "Ashen", "Golden", "Shadow", "Verdant", "Pale",
    "Obsidian", "Copper", "Silent", "Restless", "Hollow", "Bitter", "Wandering", "Gilded",
];
const NOUNS: [&str; 16] = [
    "Host", "Legion", "Marsh", "Warren", "Vale", "Reach", "Hollow", "Expanse",
    "Court", "Swarm", "Colony", "Dominion", "Nest", "Coil", "Drift", "Span",
];
const GIVEN: [&str; 32] = [
    "Ada", "Bramble", "Cyrus", "Dot", "Ember", "Fen", "Gale", "Hazel",
    "Ivo", "Juno", "Kestrel", "Lark", "Moss", "Nim", "Orin", "Pike",
    "Quill", "Rune", "Sable", "Thorn", "Umber", "Vex", "Wren", "Xan",
    "Yarrow", "Zephyr", "Bryn", "Cinder", "Dusk", "Flint", "Grove", "Hollis",
];

/// Deterministic from `(seed, colony)` so a seed always tells the same story and
/// save/load preserves it. Two-part "Adjective Noun".
pub fn colony_name(seed: u64, colony: u8) -> String {
    let mut r = Pcg32::new(seed ^ 0x9E37_79B9_7F4A_7C15, colony as u64 + 1);
    let a = ADJECTIVES[r.next_below(ADJECTIVES.len() as u32) as usize];
    let n = NOUNS[r.next_below(NOUNS.len() as u32) as usize];
    format!("the {a} {n}")
}

/// Deterministic from the ant's id. A given name plus the id's own number keeps
/// two ants with the same given name distinguishable.
pub fn ant_name(id: u64) -> String {
    let mut r = Pcg32::new(id ^ 0xD1B5_4A32_D192_ED03, 1);
    let g = GIVEN[r.next_below(GIVEN.len() as u32) as usize];
    format!("{g}-{}", id % 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colony_name_is_deterministic_for_seed_and_id() {
        assert_eq!(colony_name(1, 0), colony_name(1, 0));
        assert_eq!(colony_name(42, 3), colony_name(42, 3));
    }

    #[test]
    fn different_colonies_get_different_names() {
        let names: Vec<String> = (0..8).map(|c| colony_name(1, c)).collect();
        let mut uniq = names.clone();
        uniq.sort();
        uniq.dedup();
        assert_eq!(uniq.len(), names.len(), "colony names collided: {names:?}");
    }

    #[test]
    fn ant_name_is_deterministic_for_id() {
        assert_eq!(ant_name(1234), ant_name(1234));
        assert_ne!(ant_name(1), ant_name(2));
    }

    #[test]
    fn names_are_non_empty_and_ascii() {
        let n = colony_name(7, 5);
        assert!(!n.is_empty() && n.is_ascii());
        assert!(!ant_name(99).is_empty());
    }
}

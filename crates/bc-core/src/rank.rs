//! Chad's progress ranks. Pure fun, purely derived from finished books.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Rank {
    pub name: &'static str,
    /// Books finished needed to reach this rank.
    pub threshold: u32,
    /// Books needed for the next rank, if any.
    pub next_threshold: Option<u32>,
}

const RANKS: &[(&str, u32)] = &[
    ("Bookworm", 0),
    ("Page Turner", 3),
    ("Tome Raider", 10),
    ("Lord Footnote", 25),
    ("Gigachad of Letters", 50),
];

pub fn rank_for(finished: u32) -> Rank {
    let idx = RANKS.iter().rposition(|(_, t)| finished >= *t).unwrap_or(0);
    Rank {
        name: RANKS[idx].0,
        threshold: RANKS[idx].1,
        next_threshold: RANKS.get(idx + 1).map(|r| r.1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks() {
        assert_eq!(rank_for(0).name, "Bookworm");
        assert_eq!(rank_for(3).name, "Page Turner");
        assert_eq!(rank_for(49).name, "Lord Footnote");
        let top = rank_for(500);
        assert_eq!(top.name, "Gigachad of Letters");
        assert_eq!(top.next_threshold, None);
    }
}

//! `forge-coverage --ranked`: how much of what Commander players actually play compiles,
//! and what blocks the rest. Pure data in, a struct out; printing is separate.

use std::collections::{BTreeMap, BTreeSet};

/// The tiers of the table: the top 100, 300, … cards by EDHREC rank.
pub const TIERS: [u32; 6] = [100, 300, 1000, 2000, 5000, 10000];
/// How many blockers each list shows, and how many blocked cards.
pub const LIST_BLOCKERS: usize = 40;
pub const LIST_CARDS: usize = 60;

#[derive(Debug, PartialEq, Eq)]
pub struct Tier {
    pub top: u32,
    /// Cards in the tier that Forge has a script for.
    pub cards: usize,
    pub compiled: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Ranked {
    pub tiers: Vec<Tier>,
    /// The tier the rest describes: rank <= `top`.
    pub top: u32,
    pub blocked: usize,
    /// Blocked cards with exactly one distinct blocker.
    pub one_blocker: usize,
    /// (blocker, cards it alone blocks), most first, ties by name.
    pub alone: Vec<(String, usize)>,
    /// (blocker, cards whose list has it), most first, ties by name.
    pub any: Vec<(String, usize)>,
    /// (rank, card, its blockers) of the most played blocked cards.
    pub worst: Vec<(u32, String, Vec<String>)>,
}

/// `name` as the scripts know it: the full name first, then the front face
/// (Scryfall names two-faced cards "Front // Back").
fn script_name<'a>(name: &'a str, known: &dyn Fn(&str) -> bool) -> Option<&'a str> {
    if known(name) {
        return Some(name);
    }
    let front = name.split(" // ").next().unwrap_or(name);
    (front != name && known(front)).then_some(front)
}

fn sorted(counts: BTreeMap<&str, usize>, n: usize) -> Vec<(String, usize)> {
    let mut rows: Vec<(&str, usize)> = counts.into_iter().collect();
    rows.sort_by_key(|&(name, c)| (std::cmp::Reverse(c), name));
    rows.into_iter()
        .take(n)
        .map(|(b, c)| (b.to_string(), c))
        .collect()
}

/// `ranks`: (Scryfall name, EDHREC rank) of Commander-legal cards. `compiled`: whether a
/// script name compiles. `blocked`: script name -> what blocks it. Cards with neither a
/// compiled nor a blocked script are left out. `n`: the tier to break down.
pub fn ranked(
    ranks: impl IntoIterator<Item = (String, u32)>,
    compiled: &dyn Fn(&str) -> bool,
    blocked: &BTreeMap<String, BTreeSet<String>>,
    n: u32,
) -> Ranked {
    let mut tiers: Vec<Tier> = TIERS
        .iter()
        .map(|&top| Tier {
            top,
            cards: 0,
            compiled: 0,
        })
        .collect();
    let mut r = Ranked {
        top: n,
        ..Default::default()
    };
    let known = |s: &str| compiled(s) || blocked.contains_key(s);
    let mut ranks: Vec<(u32, String)> = ranks.into_iter().map(|(n, r)| (r, n)).collect();
    ranks.sort();
    let (mut alone, mut any): (BTreeMap<&str, usize>, BTreeMap<&str, usize>) = Default::default();
    for (rank, name) in &ranks {
        let Some(script) = script_name(name, &known) else {
            continue;
        };
        let ok = compiled(script);
        for t in tiers.iter_mut().filter(|t| *rank <= t.top) {
            t.cards += 1;
            t.compiled += usize::from(ok);
        }
        if ok || *rank > n {
            continue;
        }
        let Some(blockers) = blocked.get(script) else {
            continue;
        };
        r.blocked += 1;
        if blockers.len() == 1 {
            r.one_blocker += 1;
        }
        for b in blockers {
            *any.entry(b.as_str()).or_default() += 1;
            if blockers.len() == 1 {
                *alone.entry(b.as_str()).or_default() += 1;
            }
        }
        if r.worst.len() < LIST_CARDS {
            r.worst
                .push((*rank, name.clone(), blockers.iter().cloned().collect()));
        }
    }
    r.tiers = tiers;
    r.alone = sorted(alone, LIST_BLOCKERS);
    r.any = sorted(any, LIST_BLOCKERS);
    r
}

impl std::fmt::Display for Ranked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "\nCommander-legal cards by EDHREC rank (with a Forge script): cards / compile"
        )?;
        for t in &self.tiers {
            writeln!(
                f,
                "  top {:6}: {:6} cards, {:6} compile ({:.1}%)",
                t.top,
                t.cards,
                t.compiled,
                100.0 * t.compiled as f64 / t.cards.max(1) as f64
            )?;
        }
        writeln!(
            f,
            "\nin the top {}: {} blocked, {} of them by exactly one blocker",
            self.top, self.blocked, self.one_blocker
        )?;
        writeln!(f, "\nblockers that alone block the most cards")?;
        for (b, c) in &self.alone {
            writeln!(f, "  {c:6}  {b}")?;
        }
        writeln!(f, "\nblockers in the most cards' lists")?;
        for (b, c) in &self.any {
            writeln!(f, "  {c:6}  {b}")?;
        }
        writeln!(f, "\nthe most played blocked cards")?;
        for (rank, name, blockers) in &self.worst {
            writeln!(f, "#{rank} {name}: {}", blockers.join(" | "))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn sample() -> Ranked {
        let ranks = vec![
            ("Test Alpha".to_string(), 1),
            ("Test Beta".to_string(), 2),
            ("Test Gamma // Test Delta".to_string(), 100),
            ("Test Epsilon".to_string(), 101),
            ("Test Zeta".to_string(), 3),
            ("Test Eta".to_string(), 4),
            ("Test Theta // Test Iota".to_string(), 5),
            ("Test Unscripted".to_string(), 6),
        ];
        let ok = ["Test Alpha", "Test Gamma"];
        let mut blocked = BTreeMap::new();
        blocked.insert("Test Beta".to_string(), set(&["b:x"]));
        blocked.insert("Test Epsilon".to_string(), set(&["b:x"]));
        blocked.insert("Test Zeta".to_string(), set(&["b:y", "b:x"]));
        blocked.insert("Test Eta".to_string(), set(&["b:y"]));
        // A split card is known by its full name.
        blocked.insert("Test Theta // Test Iota".to_string(), set(&["b:z"]));
        ranked(ranks, &|s| ok.contains(&s), &blocked, 100)
    }

    #[test]
    fn tiers_count_scripted_cards_and_compiled() {
        let r = sample();
        // Unscripted card is left out; rank 101 is outside the top 100.
        assert_eq!(
            r.tiers[0],
            Tier {
                top: 100,
                cards: 6,
                compiled: 2
            }
        );
        assert_eq!(r.tiers[1].cards, 7);
        assert_eq!(r.tiers[1].compiled, 2);
    }

    #[test]
    fn blocked_and_exactly_one_blocker() {
        let r = sample();
        // Beta, Zeta, Eta, Theta (Epsilon is rank 101).
        assert_eq!(r.blocked, 4);
        assert_eq!(r.one_blocker, 3);
    }

    #[test]
    fn lists_break_ties_by_name_and_matching_uses_full_or_front() {
        let r = sample();
        assert_eq!(
            r.alone,
            vec![
                ("b:x".to_string(), 1),
                ("b:y".to_string(), 1),
                ("b:z".to_string(), 1)
            ]
        );
        assert_eq!(
            r.any,
            vec![
                ("b:x".to_string(), 2),
                ("b:y".to_string(), 2),
                ("b:z".to_string(), 1)
            ]
        );
        let names: Vec<&str> = r.worst.iter().map(|w| w.1.as_str()).collect();
        assert_eq!(
            names,
            [
                "Test Beta",
                "Test Zeta",
                "Test Eta",
                "Test Theta // Test Iota"
            ]
        );
        assert_eq!(r.worst[1].2, vec!["b:x".to_string(), "b:y".to_string()]);
    }
}

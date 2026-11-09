//! M3 acceptance (spec §13): "Heuristic AI never takes lethal commander damage when a legal
//! non-losing block exists (property test over generated boards)" — CR 903.10a / 704.6c.
//!
//! Generated boards: the attacker swings with their commander (plus random extra
//! attackers) at a defender who has already taken enough damage from that commander for this
//! hit to be lethal. The defender is the heuristic AI with random untapped blockers.

use ai::HeuristicAgent;
use engine_core::testing::Scenario;
use engine_core::turn::Step;
use engine_core::zones::Zone;
use engine_core::{Agent, ChoiceKind, GameConfig, OptionDesc, PlayerId, Primitive, Response};
use harness::script::Script;
use proptest::prelude::*;

const P0: PlayerId = PlayerId(0);
const P1: PlayerId = PlayerId(1);

const BLOCKERS: &[&str] = &[
    "Test Sprout",
    "Test Bruiser",
    "Test Golem",
    "Test Grazer",
    "Test Spider",
    "Test Raider",
];
const EXTRA_ATTACKERS: &[&str] = &["Test Bruiser", "Test Grazer", "Test Trampler"];

/// Play combat: P0 attacks P1 with everything; P1 is the heuristic AI. Returns the script
/// after combat (or after the game ended).
fn run(commander: &str, taken: u32, blockers: &[usize], extra: &[usize], seed: u64) -> Script {
    let mut sc = Scenario::new(GameConfig::commander(2), harness::catalog(), seed);
    for p in 0..2 {
        sc.library(PlayerId(p), "Forest", 10);
    }
    sc.turn(P0, Step::BeginningOfCombat);
    let cmd = sc.commander(P0, Zone::Battlefield, commander);
    sc.commander_damage(P1, cmd, taken);
    for &e in extra {
        sc.card(P0, Zone::Battlefield, EXTRA_ATTACKERS[e]);
    }
    for &b in blockers {
        sc.card(P1, Zone::Battlefield, BLOCKERS[b]);
    }
    let mut s = Script::new(sc.build());
    let mut ai = HeuristicAgent::new(seed, harness::catalog());
    for _ in 0..2_000 {
        if s.next.is_none() || s.st.turn.step == Step::EndOfCombat {
            break;
        }
        let d = s.decision().clone();
        let r = if d.player == P1 {
            ai.decide(&engine_core::player_view(&s.st, P1), &d)
        } else if d.kind == ChoiceKind::DeclareAttackers {
            let i = d
                .options
                .iter()
                .position(|o| matches!(o, OptionDesc::Attack { defender, .. } if *defender == P1));
            Response::Pick(i.into_iter().collect())
        } else {
            match &d.prim {
                Primitive::PickN { min, .. } => Response::Pick((0..*min).collect()),
                Primitive::PickNumber { max, .. } => Response::Number(*max),
                Primitive::Allocate {
                    total,
                    per_min,
                    per_max,
                } => {
                    let mut v = per_min.clone();
                    let mut left = total - per_min.iter().sum::<u32>();
                    for (i, m) in per_max.iter().enumerate() {
                        let add = left.min(m - v[i]);
                        v[i] += add;
                        left -= add;
                    }
                    Response::Allocate(v)
                }
                Primitive::Order => Response::Order((0..d.options.len()).collect()),
            }
        };
        s.respond(r);
    }
    s
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]

    #[test]
    fn ai_never_takes_avoidable_lethal_commander_damage(
        tyrant in any::<bool>(),
        taken in 15u32..=20,
        blockers in proptest::collection::vec(0usize..BLOCKERS.len(), 0..4),
        extra in proptest::collection::vec(0usize..EXTRA_ATTACKERS.len(), 0..3),
        seed in any::<u64>(),
    ) {
        // Test Matriarch: 3/5 vigilance. Test Tyrant: 5/3 menace (needs two blockers).
        let (commander, power, need) = if tyrant { ("Test Tyrant", 5, 2) } else { ("Test Matriarch", 3, 1) };
        let taken = taken.max(21 - power);
        let s = run(commander, taken, &blockers, &extra, seed);
        let legal_block_exists = blockers.len() >= need;
        let ci = s.st.commanders[0].1;
        let total = s.st.player(P1).commander_damage.get(&ci).copied().unwrap_or(0);
        if legal_block_exists {
            prop_assert!(!s.st.player(P1).has_lost, "lost with a legal block available: {commander} taken {taken} blockers {blockers:?} extra {extra:?}");
            prop_assert!(total < 21, "took {total} commander damage");
        }
    }
}

/// Sanity: the heuristic agent beats random agents (it isn't secretly broken).
#[test]
fn heuristic_beats_random_agents() {
    let base = harness::PlayOptions {
        setup: harness::Setup::Commander { players: 4 },
        agents: harness::Agents::HeuristicVsRandom,
        ..Default::default()
    };
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let results = harness::fuzz_with(base, 500, 40, threads);
    let wins = results
        .iter()
        .filter(
            |(_, r)| matches!(r, Ok(rec) if rec.result.as_ref().and_then(|r| r.winner) == Some(P0)),
        )
        .count();
    assert!(wins >= 30, "heuristic seat won only {wins}/40");
}

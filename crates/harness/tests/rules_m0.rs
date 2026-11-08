//! Atomic rules checks for the M0 subset, asserted over the event streams of random games.
//! Each test cites the CR text it checks (2026-11-06).

use engine_core::events::{Event, LossReason};
use engine_core::zones::Zone;
use engine_core::{ChoiceKind, OptionDesc, PlayerId, player_view};
use harness::{GameRecord, PlayOptions, feed, m0_game, play_random};
use std::collections::BTreeMap;

fn games() -> Vec<GameRecord> {
    (100..130)
        .map(|s| play_random(s, PlayOptions::default()).unwrap())
        .collect()
}

/// Events of each turn, keyed by turn number, with the active player.
fn turns(rec: &GameRecord) -> Vec<(u32, PlayerId, Vec<&Event>)> {
    let mut out: Vec<(u32, PlayerId, Vec<&Event>)> = Vec::new();
    for e in &rec.events {
        if let Event::TurnBegan { turn, active } = e {
            out.push((*turn, *active, Vec::new()));
        } else if let Some(t) = out.last_mut() {
            t.2.push(e);
        }
    }
    out
}

/// CR 103.8a: "In a two-player game, the player who plays first skips the draw step of
/// their first turn." CR 504.1: otherwise the active player draws.
#[test]
fn cr_103_8a_first_player_skips_first_draw() {
    for rec in games() {
        let t = turns(&rec);
        let draws = |i: usize| {
            t[i].2
                .iter()
                .filter(|e| matches!(e, Event::Drew { player, .. } | Event::DrewFromEmptyLibrary { player } if *player == t[i].1))
                .count()
        };
        assert_eq!(
            draws(0),
            0,
            "seed {}: starting player drew on turn 1",
            rec.seed
        );
        if t.len() > 1 {
            assert_eq!(draws(1), 1, "seed {}: turn 2 draw", rec.seed);
        }
    }
}

/// CR 103.4: "Each player begins the game with a starting life total of 20."
#[test]
fn cr_103_4_starting_life() {
    let st = m0_game(5);
    assert!(st.players.iter().all(|p| p.life == 20));
}

/// CR 704.5f: "If a creature has toughness 0 or less, it's put into its owner's graveyard."
/// Test Wisp (1/0) must die at the first SBA check after it resolves.
#[test]
fn cr_704_5f_zero_toughness_dies() {
    let mut checked = 0;
    for rec in games() {
        let st0 = m0_game(rec.seed);
        let wisp = st0.catalog.find("Test Wisp").unwrap();
        let instance_card: BTreeMap<_, _> = st0
            .objects
            .values()
            .map(|o| (o.card_instance, o.card))
            .collect();
        for (i, e) in rec.events.iter().enumerate() {
            if let Event::ZoneChange {
                card_instance,
                to: Zone::Battlefield,
                new,
                ..
            } = e
                && instance_card[card_instance] == wisp
            {
                let died = rec.events[i + 1..].iter().find_map(|e| match e {
                    Event::ZoneChange { old, to, .. } if old == new => Some(*to),
                    _ => None,
                });
                assert_eq!(
                    died,
                    Some(Zone::Graveyard),
                    "seed {}: Wisp survived",
                    rec.seed
                );
                checked += 1;
            }
        }
    }
    assert!(checked > 0, "no Test Wisp ever resolved in the sample");
}

/// CR 514.1: discard down to maximum hand size (seven).
#[test]
fn cr_514_1_cleanup_discard_to_seven() {
    let mut seen = 0;
    for rec in games() {
        for (k, d) in rec.decisions.iter().enumerate() {
            if d.kind == ChoiceKind::Discard {
                let mut st = m0_game(rec.seed);
                feed(&mut st, &rec.responses[..k]).unwrap();
                let hand = st.zones.hands[d.player.index()].len();
                let engine_core::Primitive::PickN { min, max } = d.prim else {
                    panic!()
                };
                assert_eq!((min, max), (hand - 7, hand - 7));
                seen += 1;
            }
        }
    }
    assert!(seen > 0);
}

/// CR 704.5a / 704.5b and CR 104.2a: the game ends when a player loses; the other wins.
#[test]
fn cr_704_5ab_game_ends_with_a_winner() {
    let mut reasons = std::collections::BTreeSet::new();
    for rec in games() {
        let lost: Vec<_> = rec
            .events
            .iter()
            .filter_map(|e| match e {
                Event::PlayerLost { player, reason } => Some((*player, *reason)),
                _ => None,
            })
            .collect();
        assert!(!lost.is_empty());
        for (_, r) in &lost {
            reasons.insert(format!("{r:?}"));
        }
        let winner = rec.result.as_ref().unwrap().winner;
        if lost.len() == 1 {
            assert_eq!(winner, Some(PlayerId(1 - lost[0].0.0)));
        } else {
            assert_eq!(winner, None, "CR 104.4a: simultaneous loss is a draw");
        }
        assert!(matches!(rec.events.last(), Some(Event::GameOver { .. })));
    }
    assert!(reasons.contains(&format!("{:?}", LossReason::ZeroLife)));
}

/// CR 117.4 / 500.2: a step ends only after all players pass in succession with an empty stack.
#[test]
fn cr_117_4_steps_end_after_all_pass() {
    for rec in games().into_iter().take(5) {
        let mut passes = 0;
        for e in &rec.events {
            match e {
                Event::PriorityPassed { .. } => passes += 1,
                Event::StepBegan { .. } | Event::GameOver { .. } => passes = 0,
                Event::LandPlayed { .. } | Event::SpellCast { .. } | Event::Resolved { .. } => {
                    passes = 0
                }
                _ => {}
            }
            assert!(passes <= 2, "seed {}: more passes than players", rec.seed);
        }
    }
}

/// Spec §5 / CLAUDE.md invariant 6: a PlayerView never exposes an opponent's hand or any
/// library contents.
#[test]
fn player_view_redacts_hidden_zones() {
    let rec = play_random(7, PlayOptions::default()).unwrap();
    let mut st = m0_game(7);
    feed(&mut st, &rec.responses[..rec.responses.len() / 2]).unwrap();
    for p in [PlayerId(0), PlayerId(1)] {
        let v = player_view(&st, p);
        let opp = PlayerId(1 - p.0);
        assert_eq!(
            v.players[opp.index()].hand_size,
            st.zones.hands[opp.index()].len()
        );
        // Every object the view exposes, from every place it can appear.
        let mut visible: Vec<engine_core::ObjectId> = v
            .hand
            .iter()
            .chain(&v.battlefield)
            .chain(&v.stack)
            .chain(&v.exile)
            .chain(v.players.iter().flat_map(|pl| &pl.graveyard))
            .map(|o| o.id)
            .collect();
        if let Some(d) = &v.decision {
            for o in &d.options {
                if let OptionDesc::Card { card }
                | OptionDesc::PlayLand { card }
                | OptionDesc::CastSpell { card } = o
                {
                    visible.push(*card);
                }
            }
        }
        let hidden = st.zones.hands[opp.index()]
            .iter()
            .chain(st.zones.libraries.iter().flatten());
        for id in hidden {
            assert!(!visible.contains(id), "{id:?} leaked to {p:?}");
        }
        assert_eq!(v.hand.len(), st.zones.hands[p.index()].len());
    }
}

//! Proof by wire (D-092): a client needs nothing but the catalog and the JSON text of its
//! view, its decision and its events. Whole games are played with every seat's agent (and a
//! narrator per seat) on the far side of a JSON boundary; each game ends in the same state as
//! the same seed played directly. What goes over the wire to a seat is checked against the
//! shared knowledge model (`tests/knowledge`): no id of a hidden card it may not know, and no
//! name that only such a card has (CR 400.2, 402.3, 701.20).

mod knowledge;

use engine_core::client::{EventView, Seat};
use engine_core::events::Event;
use engine_core::{Agent, GameState, PendingDecision, PlayerId, PlayerView, Response};
use harness::text::Narrator;
use harness::{Agents, PlayOptions, Setup};
use knowledge::Model;
use std::collections::BTreeSet;

/// One seat's client: it has the catalog (shared, read-only) and reads only JSON.
struct Client {
    agent: Box<dyn Agent>,
    narrator: Narrator,
    lines: usize,
}

impl Client {
    fn events(&mut self, json: &str) {
        let events: Vec<Event> = serde_json::from_str(json).expect("events parse");
        self.lines += self.narrator.lines(&events).len();
    }

    fn decide(&mut self, view: &str, decision: &str) -> String {
        let view: PlayerView = serde_json::from_str(view).expect("view parses");
        let decision: PendingDecision = serde_json::from_str(decision).expect("decision parses");
        assert_eq!(view.decision.as_ref(), Some(&decision), "its own decision");
        let r = self.agent.decide(&view, &decision);
        serde_json::to_string(&r).unwrap()
    }
}

/// Every JSON string in `text`.
fn strings(text: &str) -> BTreeSet<String> {
    fn walk(v: &serde_json::Value, out: &mut BTreeSet<String>) {
        match v {
            serde_json::Value::String(s) => {
                out.insert(s.clone());
            }
            serde_json::Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            serde_json::Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    let mut out = BTreeSet::new();
    walk(&serde_json::from_str(text).expect("json"), &mut out);
    out
}

/// The names (every face's) of the cards `model`'s seat may not know that no object it may
/// know shares: a JSON string equal to one of them would leak that card.
fn secret_names(st: &GameState, model: &Model) -> BTreeSet<String> {
    let names = |id: engine_core::ObjectId| {
        let card = st.object(id).card;
        let def = st.catalog.get(card);
        let mut n = vec![def.name.clone()];
        n.extend(
            (1..=def.faces.len())
                .map(|f| st.catalog.get(st.catalog.face(card, f as u8)).name.clone()),
        );
        n
    };
    let unknown: BTreeSet<_> = model
        .unknown()
        .filter(|id| st.objects.contains_key(id))
        .collect();
    let mut known = BTreeSet::new();
    for &id in st.objects.keys().filter(|id| !unknown.contains(id)) {
        known.extend(names(id));
    }
    // Commanders are public wherever they are (CR 903.3).
    for c in st.commander_cards.values() {
        known.insert(st.catalog.get(*c).name.clone());
    }
    unknown
        .into_iter()
        .flat_map(names)
        .filter(|n| !known.contains(n))
        .collect()
}

/// Play `setup`/`seed` with each seat's agent behind the wire. Returns the final state hash,
/// the number of decisions and how many log lines the narrators wrote.
fn over_the_wire(setup: Setup, seed: u64, agents: Agents) -> (u64, usize, usize) {
    let mut st = setup.game(seed);
    let catalog = st.catalog.clone();
    let n = st.num_players() as u8;
    let seats: Vec<Seat> = (0..n).map(|p| Seat::Player(PlayerId(p))).collect();
    // Host side: the seat's event redaction, and (for the test) what the seat may know.
    let mut redact: Vec<EventView> = seats.iter().map(|s| EventView::new(&st, *s)).collect();
    let mut models: Vec<Model> = seats.iter().map(|s| Model::new(&st, *s)).collect();
    // Client side: built from the catalog and the JSON of the seat's first view.
    let mut clients: Vec<Client> = (0..n)
        .map(|p| {
            let json = serde_json::to_string(&engine_core::player_view(&st, PlayerId(p))).unwrap();
            let view: PlayerView = serde_json::from_str(&json).unwrap();
            Client {
                agent: agents.make(seed, p, &catalog),
                narrator: Narrator::new(catalog.clone(), &view),
                lines: 0,
            }
        })
        .collect();
    let mut out = engine_core::start(&mut st);
    let mut decisions = 0;
    loop {
        for (i, client) in clients.iter_mut().enumerate() {
            let mut told = Vec::new();
            for e in &out.truth {
                let t = redact[i].redact(std::slice::from_ref(e));
                models[i].check(e, &t);
                told.extend(t);
            }
            let json = serde_json::to_string(&told).unwrap();
            let back: Vec<Event> = serde_json::from_str(&json).unwrap();
            assert_eq!(back, told, "events round-trip");
            client.events(&json);
        }
        let Some(d) = out.next.clone() else { break };
        let p = d.player.index();
        let view = engine_core::player_view(&st, d.player);
        let view_json = serde_json::to_string(&view).unwrap();
        let decision_json = serde_json::to_string(&d).unwrap();
        // What crossed the wire: as the seat's model allows (ids), and no secret name.
        let wired: PlayerView = serde_json::from_str(&view_json).unwrap();
        assert_eq!(wired, view, "the view round-trips");
        models[p].check_view(&st, &wired);
        // (Its decision may show it hidden cards — a scry, a search — before any event has:
        // that is what the decision is about, CR 701.20e, 701.23a.)
        let mut rest = wired.clone();
        rest.decision = None;
        rest.revealed.clear();
        // (Nor a top card a rule lets the seat see, CR 401.5 — `check_view` checks the rule.)
        for pl in &mut rest.players {
            use card_model::PlayerRule::{LookAtTopCard, TopCardRevealed};
            if pl.rules.contains(&TopCardRevealed)
                || (pl.id == d.player && pl.rules.contains(&LookAtTopCard))
            {
                pl.library_top = None;
            }
        }
        let secret = secret_names(&st, &models[p]);
        for s in strings(&serde_json::to_string(&rest).unwrap()) {
            assert!(
                !secret.contains(&s),
                "seat {p} is sent the name {s:?} of a card it may not know: {view_json}"
            );
        }
        let answer = clients[p].decide(&view_json, &decision_json);
        let r: Response = serde_json::from_str(&answer).expect("response parses");
        out = engine_core::step(&mut st, r).expect("a legal response");
        decisions += 1;
        assert!(decisions < harness::MAX_DECISIONS, "stuck");
    }
    let lines = clients.iter().map(|c| c.lines).sum();
    (st.state_hash(), decisions, lines)
}

fn same_as_direct(setup: &str, seeds: std::ops::RangeInclusive<u64>, agents: Agents) {
    let setup = Setup::parse(setup).expect("setup");
    for seed in seeds {
        let direct = harness::play_random(
            seed,
            PlayOptions {
                setup,
                agents,
                ..Default::default()
            },
        )
        .expect("a direct game");
        let (hash, decisions, lines) = over_the_wire(setup, seed, agents);
        assert_eq!(
            decisions,
            direct.responses.len(),
            "{} seed {seed}",
            setup.label()
        );
        assert_eq!(hash, direct.state_hash, "{} seed {seed}", setup.label());
        assert!(direct.result.is_some() && lines > 0);
    }
}

// D-092: the test decks of M2, played by random agents.
#[test]
fn m2_games_over_the_wire() {
    same_as_direct("m2-4p", 1..=2, Agents::Random);
}

// D-092: Commander, the heuristic agent at seat 0 (it reads more of the view).
#[test]
fn commander_games_over_the_wire() {
    same_as_direct("cmd-4p", 1..=2, Agents::HeuristicVsRandom);
}

// D-092: random decks of every compiled card (searches, reveals, digging, copies, emblems).
#[test]
fn forge_games_over_the_wire() {
    same_as_direct("forge-4p", 1..=3, Agents::Random);
}

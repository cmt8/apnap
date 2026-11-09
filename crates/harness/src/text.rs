//! Plain-text rendering of events, views and decisions for `watch` and `play`.
//! This is UI code: it may see the full `GameState` to name things in the event log, but
//! a human player's prompt is built from their `PlayerView` only.

use engine_core::events::{DamageTarget, Event};
use engine_core::stack::Target;
use engine_core::view::{ObjectView, StackView};
use engine_core::{GameState, ObjectId, OptionDesc, PendingDecision, PlayerId, PlayerView};

fn p(p: PlayerId) -> String {
    format!("P{}", p.0)
}

/// Tracks object names through zone changes (CR 400.7 gives every move a new id), so log
/// lines can name objects that have since moved.
#[derive(Debug, Default)]
pub struct Narrator {
    names: std::collections::BTreeMap<ObjectId, String>,
}

impl Narrator {
    pub fn new(st: &GameState) -> Self {
        let names = st
            .objects
            .keys()
            .map(|&id| {
                (
                    id,
                    format!("{} ({})", st.card_def(id).name, p(st.object(id).owner)),
                )
            })
            .collect();
        Narrator { names }
    }

    pub fn name(&self, id: ObjectId) -> String {
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("#{}", id.0))
    }

    /// Narrate a batch of events, merging consecutive draws by the same player.
    pub fn lines(&mut self, events: &[Event], viewer: Option<PlayerId>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut last_drawer: Option<PlayerId> = None;
        for e in events {
            let drawer = match e {
                Event::Drew { player, .. } => Some(*player),
                _ => None,
            };
            let Some(line) = self.line(e, viewer) else {
                continue;
            };
            match (drawer, last_drawer, out.last_mut()) {
                (Some(d), Some(l), Some(prev)) if d == l => {
                    let what = line.split_once(" draws ").map_or("", |x| x.1);
                    if what == "a card" {
                        *prev = merge_count(prev);
                    } else {
                        prev.push_str(", ");
                        prev.push_str(what);
                    }
                }
                _ => out.push(line),
            }
            last_drawer = drawer;
        }
        out
    }

    /// Feed every event in order; returns its log line, if any.
    pub fn line(&mut self, e: &Event, viewer: Option<PlayerId>) -> Option<String> {
        match e {
            Event::ZoneChange { old, new, .. } => {
                let n = self.name(*old);
                self.names.insert(*new, n);
            }
            Event::AbilityActivated {
                ability, source, ..
            } => {
                let n = format!("ability of {}", self.name(*source));
                self.names.insert(*ability, n);
            }
            _ => {}
        }
        event_line(self, e, viewer)
    }
}

/// One line per interesting event; `None` for noise (priority passes, step markers, etc).
/// `viewer`: if set, everyone else's draws render as "draws a card" (a player's own log).
/// "P1 draws a card" → "P1 draws 2 cards" → "P1 draws 3 cards" ...
fn merge_count(prev: &str) -> String {
    let (who, what) = prev.split_once(" draws ").unwrap_or((prev, ""));
    let n = match what {
        "a card" => 1,
        w => w
            .split_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or(1),
    };
    format!("{who} draws {} cards", n + 1)
}

fn event_line(st: &Narrator, e: &Event, viewer: Option<PlayerId>) -> Option<String> {
    Some(match e {
        Event::GameStarted { seed, num_players } => {
            format!("== game start: seed {seed}, {num_players} players ==")
        }
        Event::StartingPlayerChosen { chooser, starting } => {
            format!("{} chooses {} to go first", p(*chooser), p(*starting))
        }
        Event::MulliganDeclared { player, mulligan } => format!(
            "{} {}",
            p(*player),
            if *mulligan { "mulligans" } else { "keeps" }
        ),
        Event::TurnBegan { turn, active } => format!("\n--- turn {turn}: {} ---", p(*active)),
        Event::Drew { player, card } => {
            if viewer.is_some_and(|v| v != *player) {
                format!("{} draws a card", p(*player))
            } else {
                format!("{} draws {}", p(*player), st.name(*card))
            }
        }
        Event::DrewFromEmptyLibrary { player } => {
            format!("{} tries to draw from an empty library!", p(*player))
        }
        Event::LandPlayed { player, land } => format!("{} plays {}", p(*player), st.name(*land)),
        Event::SpellCast { player, spell } => format!("{} casts {}", p(*player), st.name(*spell)),
        Event::Resolved { spell } => format!("  {} resolves", st.name(*spell)),
        Event::AttackerDeclared { attacker, defender } => {
            format!("  {} attacks {}", st.name(*attacker), p(*defender))
        }
        Event::BlockerDeclared { blocker, attacker } => {
            format!("  {} blocks {}", st.name(*blocker), st.name(*attacker))
        }
        Event::DamageDealt {
            source,
            target,
            amount,
            ..
        } => {
            let t = match target {
                DamageTarget::Player(q) => p(*q),
                DamageTarget::Object(o) => st.name(*o),
            };
            format!("  {} deals {amount} to {t}", st.name(*source))
        }
        Event::LifeChanged { player, life, .. } => format!("  {} is at {life} life", p(*player)),
        Event::Discarded { player, card } => format!("{} discards {}", p(*player), st.name(*card)),
        Event::ZoneChange {
            new,
            from: engine_core::zones::Zone::Battlefield,
            to: engine_core::zones::Zone::Graveyard,
            ..
        } => format!("  {} dies", st.name(*new)),
        Event::AbilityActivated {
            player, ability, ..
        } => {
            format!("{} activates {}", p(*player), st.name(*ability))
        }
        Event::ModesChosen { modes, .. } => format!("  choosing mode {modes:?}"),
        Event::XChosen { x, .. } => format!("  with X = {x}"),
        Event::TargetsChosen { targets, .. } => {
            let ts: Vec<String> = targets
                .iter()
                .map(|t| match t {
                    Target::Player(q) => p(*q),
                    Target::Object(o) => st.name(*o),
                })
                .collect();
            format!("  targeting {}", ts.join(", "))
        }
        Event::Fizzled { object } => {
            format!(
                "  {} fizzles: all its targets are illegal",
                st.name(*object)
            )
        }
        Event::Countered { object } => format!("  {} is countered", st.name(*object)),
        Event::Sacrificed { player, object } => {
            format!("{} sacrifices {}", p(*player), st.name(*object))
        }
        Event::PlayerLeftGame { player } => format!("{} leaves the game", p(*player)),
        Event::PlayerLost { player, reason } => format!("{} LOSES ({reason:?})", p(*player)),
        Event::GameOver { winner } => match winner {
            Some(w) => format!("== game over: {} wins ==", p(*w)),
            None => "== game over: draw ==".into(),
        },
        _ => return None,
    })
}

fn obj(o: &ObjectView) -> String {
    let mut s = o.name.clone();
    if let (Some(pw), Some(t)) = (o.power, o.toughness) {
        s.push_str(&format!(" {pw}/{t}"));
    }
    if o.damage_marked > 0 {
        s.push_str(&format!(" ({} dmg)", o.damage_marked));
    }
    if o.tapped {
        s.push_str(" [T]");
    }
    if o.summoning_sick && o.card_types.contains(&card_model::CardType::Creature) {
        s.push_str(" (sick)");
    }
    s
}

/// A compact board summary from the viewer's perspective.
pub fn board(v: &PlayerView) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "turn {} — {} active — {:?}\n",
        v.turn_number,
        p(v.active),
        v.step
    ));
    for pl in &v.players {
        let me = if pl.id == v.viewer { " (you)" } else { "" };
        out.push_str(&format!(
            "  {}{me}: {} life, hand {}, library {}, graveyard {}\n",
            p(pl.id),
            pl.life,
            pl.hand_size,
            pl.library_size,
            pl.graveyard.len()
        ));
        let perms: Vec<String> = v
            .battlefield
            .iter()
            .filter(|o| o.controller == pl.id)
            .map(obj)
            .collect();
        if !perms.is_empty() {
            out.push_str(&format!("    battlefield: {}\n", perms.join(", ")));
        }
    }
    if !v.stack.is_empty() {
        let stack: Vec<String> = v
            .stack
            .iter()
            .rev()
            .map(|s| {
                let mut line = stack_name(s);
                if !s.targets.is_empty() {
                    let ts: Vec<String> = s.targets.iter().map(|t| target_name(v, t)).collect();
                    line.push_str(&format!(" → {}", ts.join(", ")));
                }
                line
            })
            .collect();
        out.push_str(&format!("  stack (top first): {}\n", stack.join(" | ")));
    }
    let hand: Vec<String> = v.hand.iter().map(obj).collect();
    out.push_str(&format!("  your hand: {}\n", hand.join(", ")));
    out
}

fn view_name(v: &PlayerView, id: ObjectId) -> String {
    if let Some(o) = v.hand.iter().chain(&v.battlefield).find(|o| o.id == id) {
        return obj(o);
    }
    if let Some(s) = v.stack.iter().find(|s| s.id == id) {
        return stack_name(s);
    }
    format!("#{}", id.0)
}

fn stack_name(s: &StackView) -> String {
    match s.ability_of {
        Some(_) => format!("ability of {} ({})", s.name, p(s.controller)),
        None => format!("{} ({})", s.name, p(s.controller)),
    }
}

fn target_name(v: &PlayerView, t: &Target) -> String {
    match t {
        Target::Player(q) => p(*q),
        Target::Object(id) => view_name(v, *id),
    }
}

/// Describe one option using only what the viewer can see.
pub fn option_line(v: &PlayerView, o: &OptionDesc) -> String {
    match o {
        OptionDesc::Pass => "pass".into(),
        OptionDesc::PlayLand { card } => format!("play {}", view_name(v, *card)),
        OptionDesc::CastSpell { card } => format!("cast {}", view_name(v, *card)),
        OptionDesc::TapForMana { source, mana } => {
            format!("tap {} for {mana:?}", view_name(v, *source))
        }
        OptionDesc::Player { player } => p(*player),
        OptionDesc::Keep => "keep".into(),
        OptionDesc::Mulligan => "mulligan".into(),
        OptionDesc::Card { card } => view_name(v, *card),
        OptionDesc::Attack { attacker, defender } => {
            format!("attack {} with {}", p(*defender), view_name(v, *attacker))
        }
        OptionDesc::Block { blocker, attacker } => {
            format!(
                "block {} with {}",
                view_name(v, *attacker),
                view_name(v, *blocker)
            )
        }
        OptionDesc::DamageRecipient { object } => view_name(v, *object),
        OptionDesc::ActivateAbility { source, index } => {
            format!("activate {} (ability {index})", view_name(v, *source))
        }
        OptionDesc::Mode { index } => format!("mode {index}"),
        OptionDesc::Target { target } => format!("target {}", target_name(v, target)),
        OptionDesc::Mana { mana } => mana.symbol().into(),
    }
}

pub fn decision_prompt(v: &PlayerView, d: &PendingDecision) -> String {
    use engine_core::Primitive;
    let mut out = format!("{:?}", d.kind);
    match &d.prim {
        Primitive::PickN { min, max } => {
            out.push_str(&format!(
                " — pick {min}..={max} (indices separated by spaces"
            ));
            if *min == 0 {
                out.push_str("; empty line = none");
            }
            out.push(')');
        }
        Primitive::PickNumber { min, max } => out.push_str(&format!(" — number {min}..={max}")),
        Primitive::Allocate { total, .. } => {
            out.push_str(&format!(" — split {total} among these (one number each)"))
        }
        Primitive::Order => out.push_str(" — order all (indices, first..last)"),
    }
    out.push('\n');
    for (i, o) in d.options.iter().enumerate() {
        out.push_str(&format!("  [{i}] {}\n", option_line(v, o)));
    }
    out
}

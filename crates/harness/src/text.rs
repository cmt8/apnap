//! Plain-text rendering of events, views and decisions for `watch` and `play`.
//! This is UI code: it may see the full `GameState` to name things in the event log, but
//! a human player's prompt is built from their `PlayerView` only.
//!
//! The engine's event order is not the order a reader wants (a spell's targets come before
//! its `SpellCast`, a non-permanent spell's `Resolved` comes after its effects, a draw is a
//! `ZoneChange` followed by `Drew`). The [`Narrator`] therefore keeps a little state: it
//! counts priority passes to know when the top of the stack starts to resolve, buffers the
//! details of a cast until the cast is announced, and holds back one event when the next
//! one explains it.

use card_model::describe;
use engine_core::events::{DamageTarget, Event};
use engine_core::mana::ManaType;
use engine_core::stack::Target;
use engine_core::turn::Step;
use engine_core::view::{ObjectView, StackView};
use engine_core::zones::Zone;
use engine_core::{GameState, ObjectId, OptionDesc, PendingDecision, PlayerId, PlayerView};

fn p(p: PlayerId) -> String {
    format!("P{}", p.0)
}

/// What the narrator remembers about an object id (CR 400.7 gives every move a new id).
#[derive(Debug, Clone)]
struct Info {
    name: String,
    /// The catalog entry of the card (its front face), for turning it over.
    card: Option<card_model::CardDefId>,
    /// Which face is up (CR 712.6); 0 unless a `Transformed` event said otherwise.
    face: u8,
    owner: Option<PlayerId>,
    creature: bool,
}

/// An event held back until the next one shows whether it needs a line of its own.
#[derive(Debug, Clone)]
enum Pend {
    /// A permanent became tapped: for mana (next event `ManaAdded`), by attacking (next
    /// event `AttackerDeclared`), or by something else.
    Tap(ObjectId),
    /// A zone change that a following event may already say (`Drew`, `Discarded`,
    /// `Sacrificed`, `LandPlayed`).
    Move { new: ObjectId, from: Zone, to: Zone },
    /// Damage to a player: a `LifeChanged` right after is the same news.
    Damage {
        source: ObjectId,
        target: PlayerId,
        amount: u32,
        combat: bool,
    },
    /// One card revealed (activating an ability from hand reveals it).
    Reveal { player: PlayerId, card: ObjectId },
    /// A discard that may turn out to be the cost of cycling.
    Discard { player: PlayerId, card: ObjectId },
}

/// Tracks object names through zone changes (CR 400.7 gives every move a new id), so log
/// lines can name objects that have since moved, and the state needed to put the engine's
/// events into reading order.
#[derive(Debug)]
pub struct Narrator {
    info: std::collections::BTreeMap<ObjectId, Info>,
    catalog: std::sync::Arc<card_model::CardCatalog>,
    pend: Option<Pend>,
    /// Consecutive `PriorityPassed` events since anything else happened.
    passes: usize,
    /// Players still in the game (a full round of passes is this many).
    alive: usize,
    /// Spells and abilities on the stack, bottom first, as announced.
    stack: Vec<ObjectId>,
    /// The stack object whose resolution is being narrated (its effects are indented).
    resolving: Option<ObjectId>,
    /// A spell moved to the stack whose `SpellCast` has not come yet, and where from.
    casting: Option<Zone>,
    /// Lines of that cast (targets, modes, payment), shown after the `casts` line.
    cast_buf: Vec<String>,
    /// Triggered abilities that have triggered but are not on the stack yet.
    triggers: Vec<(ObjectId, PlayerId, card_model::AbilityId)>,
    /// The ability just put on the stack, while nothing else has happened.
    just_announced: Option<ObjectId>,
    /// Permanents tapped for mana and mana added since the last payment.
    adds: Vec<ManaType>,
    adds_src: Option<ObjectId>,
    adds_player: Option<PlayerId>,
    /// The last event was a `ManaAdded` (more mana from the same source follows).
    adds_open: bool,
    /// The permanent whose tap the next `ManaAdded` belongs to.
    tap_src: Option<ObjectId>,
    /// What each player has floating: the mana whose type is known, and a count of the rest.
    pool: std::collections::BTreeMap<PlayerId, (Vec<ManaType>, u32)>,
    /// A search that has not (yet) taken a card out of the library.
    searching: Option<(PlayerId, bool)>,
    /// A step heading shown before the next line that happens in that step.
    pending_step: Option<&'static str>,
    /// The heading last shown this turn.
    last_heading: Option<&'static str>,
    /// Permanents untapped in the untap step (one summary line).
    untapped: Vec<ObjectId>,
    step: Option<Step>,
    /// Permanents tapped in the declare attackers step, until `AttackerDeclared` says
    /// they attack (the engine taps them all before declaring).
    attack_taps: Vec<ObjectId>,
    active: Option<PlayerId>,
    /// The damage just dealt, for "(X's damage)" on lifelink.
    last_damage: Option<(ObjectId, u32)>,
    /// Set by the settled `Move` before a `Sacrificed` / by `Reveal` before an activation.
    sacrificed_died: bool,
    from_hand: bool,
}

impl Narrator {
    pub fn new(st: &GameState) -> Self {
        let creature = |id: ObjectId| {
            st.card_def(id)
                .card_types
                .contains(&card_model::CardType::Creature)
        };
        let info = st
            .objects
            .keys()
            .map(|&id| {
                (
                    id,
                    Info {
                        name: format!("{} ({})", st.card_def(id).name, p(st.object(id).owner)),
                        card: Some(st.object(id).card),
                        face: 0,
                        owner: Some(st.object(id).owner),
                        creature: creature(id),
                    },
                )
            })
            .collect();
        Narrator {
            info,
            catalog: st.catalog.clone(),
            pend: None,
            passes: 0,
            alive: st.players_in_game(),
            stack: Vec::new(),
            resolving: None,
            casting: None,
            cast_buf: Vec::new(),
            triggers: Vec::new(),
            just_announced: None,
            adds: Vec::new(),
            adds_src: None,
            adds_player: None,
            adds_open: false,
            tap_src: None,
            pool: std::collections::BTreeMap::new(),
            searching: None,
            pending_step: None,
            last_heading: None,
            untapped: Vec::new(),
            step: None,
            attack_taps: Vec::new(),
            active: None,
            last_damage: None,
            sacrificed_died: false,
            from_hand: false,
        }
    }

    pub fn name(&self, id: ObjectId) -> String {
        self.info
            .get(&id)
            .map(|i| i.name.clone())
            .unwrap_or_else(|| format!("#{}", id.0))
    }

    /// The name without the owner ("Forest", not "Forest (P0)").
    fn short(&self, id: ObjectId) -> String {
        let n = self.name(id);
        match n.rsplit_once(" (P") {
            Some((a, _)) => a.to_string(),
            None => n,
        }
    }

    fn owner(&self, id: ObjectId) -> Option<PlayerId> {
        self.info.get(&id).and_then(|i| i.owner)
    }

    /// Narrate a batch of events, merging consecutive draws by the same player. State
    /// carries over to the next batch (a cast or a resolution can span a decision).
    pub fn lines(&mut self, events: &[Event], viewer: Option<PlayerId>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut last_drawer: Option<PlayerId> = None;
        for e in events {
            let before = out.len();
            self.feed(e, viewer, &mut out);
            let drawer = match e {
                Event::Drew { player, .. } => Some(*player),
                _ => None,
            };
            // Only draws with nothing between them but their own zone changes are one line,
            // and only at the same nesting: a draw step is not a spell's draw.
            if !matches!(e, Event::Drew { .. } | Event::ZoneChange { .. }) {
                last_drawer = None;
            }
            if out.len() > before {
                if let (Some(d), Some(l)) = (drawer, last_drawer)
                    && d == l
                    && out.len() == before + 1
                    && before > 0
                    && indent(&out[before]) == indent(&out[before - 1])
                {
                    let line = out.pop().unwrap_or_default();
                    let what = line.split_once(" draws ").map_or("", |x| x.1);
                    let prev = out.last_mut().expect("previous draw");
                    if what == "a card" {
                        *prev = merge_count(prev);
                    } else {
                        prev.push_str(", ");
                        prev.push_str(what);
                    }
                }
                last_drawer = drawer;
            }
        }
        self.end_batch(viewer, &mut out);
        out
    }

    /// Feed one event; returns its log line(s), joined by newlines, if any. (Some events
    /// are held back until the next one, so a line may appear one call late.)
    pub fn line(&mut self, e: &Event, viewer: Option<PlayerId>) -> Option<String> {
        let mut out = Vec::new();
        self.feed(e, viewer, &mut out);
        if out.is_empty() {
            None
        } else {
            Some(out.join("\n"))
        }
    }

    /// Nothing more will be said about the held-back events of this batch.
    fn end_batch(&mut self, viewer: Option<PlayerId>, out: &mut Vec<String>) {
        self.flush_adds(out);
        self.adds_open = false;
        // A tap may still turn into a mana tap in the next batch (a choice of color).
        if !matches!(self.pend, Some(Pend::Tap(_))) {
            self.flush_pend(viewer, out);
        }
        self.flush_untaps(out);
        self.drain_cast(out);
    }

    fn flush_attack_taps(&mut self, out: &mut Vec<String>) {
        for o in std::mem::take(&mut self.attack_taps) {
            let l = format!("  {} taps", self.name(o));
            self.emit(l, out);
        }
    }

    fn drain_cast(&mut self, out: &mut Vec<String>) {
        self.casting = None;
        out.append(&mut self.cast_buf);
    }

    fn flush_untaps(&mut self, out: &mut Vec<String>) {
        if self.untapped.is_empty() {
            return;
        }
        let ids = std::mem::take(&mut self.untapped);
        let who = self.active.map_or(String::new(), |a| format!("{} ", p(a)));
        out.push(format!(
            "  {who}untaps {} permanent(s): {}",
            ids.len(),
            self.counted(&ids, 60)
        ));
    }

    /// "4x Forest, Elf": names with counts, in order of first appearance, cut at `max`.
    fn counted(&self, ids: &[ObjectId], max: usize) -> String {
        let mut seen: Vec<(String, usize)> = Vec::new();
        for &id in ids {
            let n = self.short(id);
            match seen.iter_mut().find(|(s, _)| *s == n) {
                Some((_, c)) => *c += 1,
                None => seen.push((n, 1)),
            }
        }
        let mut s = String::new();
        for (i, (n, c)) in seen.iter().enumerate() {
            let part = if *c > 1 {
                format!("{c}x {n}")
            } else {
                n.clone()
            };
            if i > 0 {
                s.push_str(", ");
            }
            if s.len() + part.len() > max {
                s.push_str("...");
                break;
            }
            s.push_str(&part);
        }
        s
    }

    /// A line of the game log: indented under the resolving object, or kept back until the
    /// cast it belongs to is announced.
    fn emit(&mut self, line: String, out: &mut Vec<String>) {
        if let Some(h) = self.pending_step.take() {
            self.last_heading = Some(h);
            out.push(h.to_string());
        }
        let line = if self.resolving.is_some() && !line.starts_with('\n') {
            format!("    {}", line.trim_start())
        } else {
            line
        };
        if self.casting.is_some() {
            self.cast_buf.push(line);
        } else {
            out.push(line);
        }
    }

    fn flush_pend(&mut self, viewer: Option<PlayerId>, out: &mut Vec<String>) {
        let Some(pend) = self.pend.take() else {
            return;
        };
        let line = match pend {
            Pend::Tap(o) => Some(format!("  {} taps", self.name(o))),
            Pend::Move { new, from, to } => self.zone_line(new, from, to, viewer),
            Pend::Damage {
                source,
                target,
                amount,
                combat,
            } => Some(self.damage_line(source, &p(target), amount, combat, "")),
            Pend::Reveal { player, card } => {
                Some(format!("  {} reveals {}", p(player), self.name(card)))
            }
            Pend::Discard { player, card } => {
                Some(format!("{} discards {}", p(player), self.name(card)))
            }
        };
        if let Some(l) = line {
            self.emit(l, out);
        }
    }

    fn damage_line(
        &self,
        source: ObjectId,
        to: &str,
        amount: u32,
        combat: bool,
        tail: &str,
    ) -> String {
        let kind = if combat { " combat damage" } else { " damage" };
        format!("  {} deals {amount}{kind} to {to}{tail}", self.name(source))
    }

    /// Let the held-back event be explained by `e`. Returns true if `e` is used up.
    fn settle(&mut self, e: &Event, viewer: Option<PlayerId>, out: &mut Vec<String>) -> bool {
        let Some(pend) = self.pend.clone() else {
            return false;
        };
        match (&pend, e) {
            (Pend::Tap(o), Event::ManaAdded { .. }) => {
                self.tap_src = Some(*o);
                self.pend = None;
            }
            (Pend::Tap(o), Event::AttackerDeclared { attacker, .. }) if o == attacker => {
                self.pend = None;
            }
            (Pend::Move { new, from, to }, ev) => {
                let explained = match (from, to, ev) {
                    (Zone::Library, Zone::Hand, Event::Drew { card, .. }) => card == new,
                    (Zone::Hand, Zone::Graveyard, Event::Discarded { card, .. }) => card == new,
                    (Zone::Hand, Zone::Battlefield, Event::LandPlayed { land, .. }) => land == new,
                    (Zone::Battlefield, Zone::Graveyard, Event::Sacrificed { object, .. }) => {
                        if object == new {
                            self.sacrificed_died = true;
                        }
                        object == new
                    }
                    _ => false,
                };
                if explained {
                    self.pend = None;
                } else {
                    self.flush_pend(viewer, out);
                }
            }
            (
                Pend::Damage {
                    source,
                    target,
                    amount,
                    combat,
                },
                Event::LifeChanged {
                    player,
                    delta,
                    life,
                },
            ) if player == target && *delta == -(*amount as i32) => {
                self.pend = None;
                let tail = format!(" ({} at {life})", p(*player));
                let l = self.damage_line(*source, &p(*target), *amount, *combat, &tail);
                self.emit(l, out);
                return true;
            }
            (Pend::Reveal { card, .. }, Event::AbilityActivated { source, .. })
                if card == source =>
            {
                self.from_hand = true;
                self.pend = None;
            }
            (Pend::Discard { player, card }, Event::Cycled { card: c, .. }) if card == c => {
                self.pend = None;
                let l = format!("{} cycles {}", p(*player), self.name(*card));
                self.emit(l, out);
                return true;
            }
            _ => self.flush_pend(viewer, out),
        }
        false
    }

    /// CR 117.4: when every player passes in a row with something on the stack, its top
    /// object starts to resolve, and everything until its `Resolved` belongs to it.
    fn begin_resolution(&mut self, e: &Event, out: &mut Vec<String>) {
        if self.passes >= self.alive
            && self.resolving.is_none()
            && let Some(&top) = self.stack.last()
            && !matches!(e, Event::Fizzled { object } if *object == top)
        {
            self.resolving = Some(top);
            if let Some(h) = self.pending_step.take() {
                self.last_heading = Some(h);
                out.push(h.to_string());
            }
            out.push(format!("  {} resolves", self.name(top)));
        }
        self.passes = 0;
    }

    fn track_names(&mut self, e: &Event) {
        match e {
            Event::ZoneChange { old, new, .. } => {
                if let Some(mut i) = self.info.get(old).cloned() {
                    // CR 712.6a: off the stack and battlefield a card has its front face.
                    if i.face != 0
                        && let Some(c) = i.card
                    {
                        i.face = 0;
                        let owner = i.owner.map_or(String::new(), |o| format!(" ({})", p(o)));
                        i.name = format!("{}{owner}", self.catalog.get(c).name);
                        i.creature = self
                            .catalog
                            .get(c)
                            .card_types
                            .contains(&card_model::CardType::Creature);
                    }
                    self.info.insert(*new, i);
                }
            }
            Event::Transformed { object, face } => {
                if let Some(i) = self.info.get_mut(object)
                    && let Some(c) = i.card
                {
                    let def = self.catalog.get(self.catalog.face(c, *face));
                    let owner = i.owner.map_or(String::new(), |o| format!(" ({})", p(o)));
                    i.face = *face;
                    i.name = format!("{}{owner}", def.name);
                    i.creature = def.card_types.contains(&card_model::CardType::Creature);
                }
            }
            Event::AttackerDeclared { attacker: o, .. }
            | Event::BlockerDeclared { blocker: o, .. }
            | Event::DamageDealt {
                source: o,
                combat: true,
                ..
            } => {
                // CR 700.4: a creature that dies was a creature on the battlefield, however
                // it became one; attacking, blocking or dealing combat damage proves it.
                if let Some(i) = self.info.get_mut(o) {
                    i.creature = true;
                }
            }
            Event::TokenCreated {
                object,
                card,
                controller,
            } => {
                let def = self.catalog.get(*card);
                self.info.insert(
                    *object,
                    Info {
                        name: format!("{} token ({})", def.name, p(*controller)),
                        card: Some(*card),
                        face: 0,
                        owner: Some(*controller),
                        creature: def.card_types.contains(&card_model::CardType::Creature),
                    },
                );
            }
            _ => {}
        }
    }

    /// First ~40 characters of an ability's rules text.
    fn tag(&self, id: card_model::AbilityId) -> String {
        let def = self.catalog.get(id.card);
        let text = describe::ability_text(def, self.catalog.ability(id));
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.chars().count() > 40 {
            let cut: String = text.chars().take(40).collect();
            format!("{}...", cut.trim_end())
        } else {
            text
        }
    }

    fn flush_adds(&mut self, out: &mut Vec<String>) {
        if self.adds.is_empty() {
            return;
        }
        let syms: String = std::mem::take(&mut self.adds)
            .iter()
            .map(|m| m.symbol())
            .collect();
        let who = self.adds_player.take().map_or(String::new(), p);
        let l = match self.adds_src.take() {
            Some(o) => format!("  {who} taps {} for {syms}", self.short(o)),
            None => format!("  {who} adds {syms}"),
        };
        self.emit(l, out);
    }

    /// A search that took nothing from the library says so when it is over.
    fn finish_search(&mut self, out: &mut Vec<String>) {
        if let Some((who, false)) = self.searching.take() {
            let l = format!("  {} finds nothing", p(who));
            self.emit(l, out);
        }
        self.searching = None;
    }

    fn end_resolution(&mut self) {
        self.resolving = None;
    }

    fn feed(&mut self, e: &Event, viewer: Option<PlayerId>, out: &mut Vec<String>) {
        self.track_names(e);
        if !matches!(e, Event::ManaAdded { .. }) {
            self.flush_adds(out);
            self.adds_open = false;
        }
        if matches!(
            e,
            Event::Shuffled { .. }
                | Event::Resolved { .. }
                | Event::PriorityPassed { .. }
                | Event::StepBegan { .. }
                | Event::TurnBegan { .. }
                | Event::SpellCast { .. }
                | Event::AbilityActivated { .. }
        ) {
            self.finish_search(out);
        }
        if let Event::ZoneChange {
            from: Zone::Library,
            ..
        } = e
            && let Some((_, found)) = &mut self.searching
        {
            *found = true;
        }
        if let Event::PriorityPassed { .. } = e {
            // Nothing resolves while players pass (CR 117.3d).
            self.end_resolution();
            self.flush_pend(viewer, out);
            self.flush_untaps(out);
            self.flush_attack_taps(out);
            self.drain_cast(out);
            self.passes += 1;
            self.triggers.clear();
            self.just_announced = None;
            self.last_damage = None;
            return;
        }
        if !matches!(e, Event::Untapped { .. }) {
            self.flush_untaps(out);
        }
        if !matches!(e, Event::Tapped { .. } | Event::AttackerDeclared { .. }) {
            self.flush_attack_taps(out);
        }
        self.begin_resolution(e, out);
        if self.settle(e, viewer, out) {
            self.last_damage = None;
            self.just_announced = None;
            return;
        }
        let announced = self.just_announced.take();
        let last_damage = self.last_damage.take();
        match e {
            Event::GameStarted { seed, num_players } => out.push(format!(
                "== game start: seed {seed}, {num_players} players =="
            )),
            Event::StartingPlayerChosen { chooser, starting } => {
                let l = format!("{} chooses {} to go first", p(*chooser), p(*starting));
                self.emit(l, out);
            }
            Event::Shuffled { player } => {
                let l = format!("{} shuffles their library", p(*player));
                self.emit(l, out);
            }
            Event::MulliganDeclared { player, mulligan } => {
                let l = format!(
                    "{} {}",
                    p(*player),
                    if *mulligan { "mulligans" } else { "keeps" }
                );
                self.emit(l, out);
            }
            Event::TurnBegan { turn, active } => {
                self.drain_cast(out);
                self.active = Some(*active);
                self.triggers.clear();
                self.end_resolution();
                self.pending_step = None;
                self.last_heading = None;
                out.push(format!("\n--- turn {turn}: {} ---", p(*active)));
            }
            Event::StepBegan { step } => {
                self.step = Some(*step);
                self.drain_cast(out);
                self.end_resolution();
                // Headings appear only if something happens in the step (combat: any of
                // its steps), so a reader can check timing without noise.
                let in_combat = self.pending_step == Some("  -- combat --");
                self.pending_step = match step {
                    Step::Upkeep => Some("  -- upkeep --"),
                    // The draw after an upkeep with actions in it needs its own heading.
                    Step::Draw if self.last_heading == Some("  -- upkeep --") => {
                        Some("  -- draw --")
                    }
                    Step::BeginningOfCombat => Some("  -- combat --"),
                    Step::PostcombatMain => Some("  -- second main --"),
                    Step::End => Some("  -- end step --"),
                    Step::DeclareAttackers
                    | Step::DeclareBlockers
                    | Step::CombatDamage
                    | Step::EndOfCombat
                        if in_combat =>
                    {
                        Some("  -- combat --")
                    }
                    _ => None,
                };
            }
            Event::ZoneChange {
                new, from, to, old, ..
            } => {
                let _ = old;
                if *to == Zone::Stack && *from != Zone::Stack {
                    // Announcing a spell (CR 601.2a): its details follow, `SpellCast` last.
                    self.casting = Some(*from);
                } else if matches!(
                    (from, to),
                    (Zone::Library, Zone::Hand)
                        | (Zone::Hand, Zone::Graveyard)
                        | (Zone::Battlefield, Zone::Graveyard)
                        | (Zone::Hand, Zone::Battlefield)
                ) {
                    self.pend = Some(Pend::Move {
                        new: *new,
                        from: *from,
                        to: *to,
                    });
                } else if let Some(l) = self.zone_line(*new, *from, *to, viewer) {
                    self.emit(l, out);
                }
                if *from == Zone::Stack {
                    self.stack.retain(|s| s != old);
                }
            }
            Event::Drew { player, card } => {
                let l = if viewer.is_some_and(|v| v != *player) {
                    format!("{} draws a card", p(*player))
                } else {
                    format!("{} draws {}", p(*player), self.name(*card))
                };
                self.emit(l, out);
            }
            Event::DrewFromEmptyLibrary { player } => {
                let l = format!("{} tries to draw from an empty library!", p(*player));
                self.emit(l, out);
            }
            Event::LandPlayed { player, land } => {
                let l = format!("{} plays {}", p(*player), self.name(*land));
                self.emit(l, out);
            }
            Event::SpellCast { player, spell } => {
                let from = self.casting.take();
                let tail = match from {
                    Some(Zone::Command) => " from the command zone",
                    Some(Zone::Graveyard) => " from the graveyard",
                    Some(Zone::Exile) => " from exile",
                    Some(Zone::Library) => " from the library",
                    _ => "",
                };
                let l = format!("{} casts {}{tail}", p(*player), self.name(*spell));
                self.emit(l, out);
                out.append(&mut self.cast_buf);
                self.stack.push(*spell);
            }
            Event::Resolved { spell } => {
                self.stack.retain(|s| s != spell);
                if self.resolving != Some(*spell) {
                    let l = format!("  {} resolves", self.name(*spell));
                    self.emit(l, out);
                }
                // Whatever was being narrated is over (the stack may also be empty).
                self.end_resolution();
            }
            Event::AttackerDeclared { attacker, defender } => {
                self.attack_taps.retain(|o| o != attacker);
                let l = format!("  {} attacks {}", self.name(*attacker), p(*defender));
                self.emit(l, out);
            }
            Event::BlockerDeclared { blocker, attacker } => {
                let l = format!("  {} blocks {}", self.name(*blocker), self.name(*attacker));
                self.emit(l, out);
            }
            Event::DamageDealt {
                source,
                target,
                amount,
                combat,
            } => {
                self.last_damage = Some((*source, *amount));
                match target {
                    // Held back: a `LifeChanged` right after says the rest.
                    DamageTarget::Player(q) => {
                        self.pend = Some(Pend::Damage {
                            source: *source,
                            target: *q,
                            amount: *amount,
                            combat: *combat,
                        })
                    }
                    DamageTarget::Object(o) => {
                        let l = self.damage_line(*source, &self.name(*o), *amount, *combat, "");
                        self.emit(l, out);
                    }
                }
            }
            Event::LifeChanged {
                player,
                delta,
                life,
            } => {
                let l = if *delta >= 0 {
                    let why = match last_damage {
                        Some((s, a)) if a as i32 == *delta => {
                            format!(" from {}'s damage", self.name(s))
                        }
                        _ => String::new(),
                    };
                    format!("  {} gains {delta} ({life}){why}", p(*player))
                } else {
                    format!("  {} loses {} ({life})", p(*player), -delta)
                };
                self.emit(l, out);
            }
            Event::Discarded { player, card } => {
                self.pend = Some(Pend::Discard {
                    player: *player,
                    card: *card,
                });
            }
            Event::Cycled { player, card } => {
                let l = format!("{} cycles {}", p(*player), self.name(*card));
                self.emit(l, out);
            }
            Event::AbilityActivated {
                player,
                ability,
                source,
            } => {
                let trig = self
                    .triggers
                    .iter()
                    .position(|(s, c, _)| s == source && c == player)
                    .map(|i| self.triggers.remove(i).2);
                let from_hand = std::mem::take(&mut self.from_hand);
                let src = self.name(*source);
                let l = match trig {
                    Some(id) => {
                        let n = format!("trigger of {src} [{}]", self.tag(id));
                        self.info.insert(
                            *ability,
                            Info {
                                name: n.clone(),
                                owner: None,
                                card: None,
                                face: 0,
                                creature: false,
                            },
                        );
                        format!("{} puts {n} on the stack", p(*player))
                    }
                    None => {
                        let n = format!("ability of {src}");
                        self.info.insert(
                            *ability,
                            Info {
                                name: n.clone(),
                                owner: None,
                                card: None,
                                face: 0,
                                creature: false,
                            },
                        );
                        let from = if from_hand { " from hand" } else { "" };
                        format!("{} activates {n}{from}", p(*player))
                    }
                };
                self.stack.push(*ability);
                self.just_announced = Some(*ability);
                self.emit(l, out);
            }
            Event::ModesChosen { modes, .. } => {
                self.keep_announced(announced);
                let l = format!("  choosing mode {modes:?}");
                self.emit(l, out);
            }
            Event::XChosen { x, .. } => {
                self.keep_announced(announced);
                let l = format!("  with X = {x}");
                self.emit(l, out);
            }
            Event::TargetsChosen { targets, .. } => {
                self.keep_announced(announced);
                let ts: Vec<String> = targets
                    .iter()
                    .map(|t| match t {
                        Target::Player(q) => p(*q),
                        Target::Object(o) => self.name(*o),
                    })
                    .collect();
                let l = if ts.is_empty() {
                    "  with no targets".to_string()
                } else {
                    format!("  targeting {}", ts.join(", "))
                };
                self.emit(l, out);
            }
            Event::Fizzled { object } => {
                self.stack.retain(|s| s != object);
                if self.resolving == Some(*object) {
                    self.resolving = None;
                }
                let l = format!(
                    "  {} fizzles: all its targets are illegal",
                    self.name(*object)
                );
                self.emit(l, out);
            }
            Event::Countered { object } => {
                self.stack.retain(|s| s != object);
                let l = format!("  {} is countered", self.name(*object));
                self.emit(l, out);
            }
            Event::AbilityRemoved { ability } => {
                if self.resolving == Some(*ability) {
                    self.end_resolution();
                }
                if self.stack.contains(ability) {
                    self.stack.retain(|s| s != ability);
                    let n = self.name(*ability);
                    let l = if announced == Some(*ability) {
                        // CR 603.3c-d: a trigger with no legal mode or target.
                        format!("  {n} is removed from the stack: no legal target (CR 603.3d)")
                    } else {
                        format!("  {n} ceases to exist (its controller left the game)")
                    };
                    self.emit(l, out);
                }
            }
            Event::Sacrificed { player, object } => {
                let died = std::mem::take(&mut self.sacrificed_died);
                let note = match (died, self.info.get(object).is_some_and(|i| i.creature)) {
                    (true, true) => " (it dies)",
                    (true, false) => " (it is put into the graveyard)",
                    _ => "",
                };
                let l = format!("{} sacrifices {}{note}", p(*player), self.name(*object));
                self.emit(l, out);
            }
            Event::PlayerLeftGame { player } => {
                self.alive = self.alive.saturating_sub(1);
                let l = format!("{} leaves the game", p(*player));
                self.emit(l, out);
            }
            Event::TokenCreated {
                object, controller, ..
            } => {
                let l = format!("{} creates {}", p(*controller), self.name(*object));
                self.emit(l, out);
            }
            Event::CountersPut { object, kind, n } => {
                let l = format!(
                    "  {} gets {n} {} counter(s)",
                    self.name(*object),
                    counter(*kind)
                );
                self.emit(l, out);
            }
            Event::CountersRemoved { object, kind, n } => {
                let l = format!(
                    "  {n} {} counter(s) removed from {}",
                    counter(*kind),
                    self.name(*object)
                );
                self.emit(l, out);
            }
            Event::Triggered {
                source,
                controller,
                ability,
            } => {
                self.triggers.push((*source, *controller, *ability));
                let l = format!("  {} triggers", self.name(*source));
                self.emit(l, out);
            }
            Event::DamagePrevented { target, amount } => {
                let t = match target {
                    DamageTarget::Player(q) => p(*q),
                    DamageTarget::Object(o) => self.name(*o),
                };
                let l = format!("  {amount} damage to {t} prevented");
                self.emit(l, out);
            }
            Event::Replaced { source } => {
                let l = format!("  ({}'s replacement effect applies)", self.name(*source));
                self.emit(l, out);
            }
            Event::Attached { aura, to } => {
                let l = format!(
                    "  {} is now attached to {}",
                    self.name(*aura),
                    self.name(*to)
                );
                self.emit(l, out);
            }
            Event::Unattached { object } => {
                let l = format!("  {} becomes unattached", self.name(*object));
                self.emit(l, out);
            }
            Event::ControlChanged { object, controller } => {
                let l = format!(
                    "  {} gains control of {}",
                    p(*controller),
                    self.name(*object)
                );
                self.emit(l, out);
            }
            Event::BlockDeclarationIllegal { player } => {
                let l = format!(
                    "  {}'s block declaration was illegal and is undone",
                    p(*player)
                );
                self.emit(l, out);
            }
            Event::RemovedFromCombat { object } => {
                let l = format!("  {} is removed from combat", self.name(*object));
                self.emit(l, out);
            }
            // CR 707.4–707.5: what a permanent is now a copy of.
            Event::BecameCopy { object, card } => {
                let l = format!(
                    "  {} becomes a copy of {}",
                    self.name(*object),
                    self.catalog.get(*card).name
                );
                self.emit(l, out);
            }
            Event::Transformed { object, face } => {
                let side = if *face == 0 { "front" } else { "back" };
                let l = format!("  {} transforms ({side} face up)", self.name(*object));
                self.emit(l, out);
            }
            // CR 614.12a: a choice made as a permanent enters is public.
            Event::Chose {
                player,
                object,
                chosen,
            } => {
                let value = match chosen {
                    card_model::Chosen::CreatureType(t) => t.name().to_string(),
                    card_model::Chosen::Color(c) => format!("{c:?}").to_lowercase(),
                };
                let l = format!(
                    "  {} chooses {value} for {}",
                    p(*player),
                    self.name(*object)
                );
                self.emit(l, out);
            }
            Event::Tapped { object } => {
                if self.step == Some(Step::DeclareAttackers) {
                    self.attack_taps.push(*object);
                } else {
                    self.pend = Some(Pend::Tap(*object));
                }
            }
            Event::Untapped { object } => {
                if self.step == Some(Step::Untap) {
                    self.untapped.push(*object);
                } else {
                    let l = format!("  {} untaps", self.name(*object));
                    self.emit(l, out);
                }
            }
            Event::ManaAdded { player, mana } => {
                let src = self.tap_src.take();
                // A second mana straight after the first is the same source's.
                if src.is_some() || !self.adds_open {
                    self.flush_adds(out);
                    self.adds_src = src;
                    self.adds_player = Some(*player);
                }
                self.adds.push(*mana);
                self.adds_open = true;
                self.pool.entry(*player).or_default().0.push(*mana);
            }
            Event::ManaPaid { player, amount } => {
                let (known, unknown) = self.pool.remove(player).unwrap_or_default();
                let total = known.len() as u32 + unknown;
                let l = if total == *amount && unknown == 0 && !known.is_empty() {
                    let syms: String = known.iter().map(|m| m.symbol()).collect();
                    format!("  (paid {amount} from the pool: {syms})")
                } else if total > *amount {
                    self.pool.insert(*player, (Vec::new(), total - amount));
                    format!("  (paid {amount}; {} left floating)", total - amount)
                } else {
                    format!("  (paid {amount})")
                };
                self.emit(l, out);
            }
            Event::ManaEmptied { player, .. } => {
                self.pool.remove(player);
            }
            Event::SearchedLibrary { player } => {
                self.searching = Some((*player, false));
                let l = format!("  {} searches their library", p(*player));
                self.emit(l, out);
            }
            Event::Revealed { player, cards } => {
                if let [card] = cards.as_slice() {
                    self.pend = Some(Pend::Reveal {
                        player: *player,
                        card: *card,
                    });
                } else {
                    let names: Vec<String> = cards.iter().map(|c| self.name(*c)).collect();
                    let l = format!("  {} reveals {}", p(*player), names.join(", "));
                    self.emit(l, out);
                }
            }
            Event::LookedAtLibrary { player, count } => {
                let l = format!(
                    "  {} looks at the top {count} card(s) of their library",
                    p(*player)
                );
                self.emit(l, out);
            }
            Event::Scried {
                player,
                count,
                bottom,
            } => {
                let l = format!("  {} scries {count} ({bottom} to the bottom)", p(*player));
                self.emit(l, out);
            }
            Event::Surveilled {
                player,
                count,
                graveyard,
            } => {
                let l = format!(
                    "  {} surveils {count} ({graveyard} to the graveyard)",
                    p(*player)
                );
                self.emit(l, out);
            }
            Event::LibraryArranged {
                player,
                top,
                bottom,
            } => {
                let l = format!(
                    "  {} puts {top} card(s) on top and {bottom} on the bottom of their library",
                    p(*player)
                );
                self.emit(l, out);
            }
            // Hidden from everyone but the player who looked.
            Event::LookedAt { player, cards } => {
                if viewer.is_some_and(|v| v != *player) {
                    return;
                }
                let names: Vec<String> = cards.iter().map(|c| self.name(*c)).collect();
                let l = format!("  {} looks at {}", p(*player), names.join(", "));
                self.emit(l, out);
            }
            Event::PlayerLost { player, reason } => {
                let l = format!("{} LOSES ({reason:?})", p(*player));
                self.emit(l, out);
            }
            Event::GameOver { winner } => {
                self.flush_pend(viewer, out);
                out.push(match winner {
                    Some(w) => format!("== game over: {} wins ==", p(*w)),
                    None => "== game over: draw ==".into(),
                });
            }
            Event::ObjectLeftGame { object } => {
                self.stack.retain(|s| s != object);
            }
            Event::ContinuousEffectBegan { source, .. } => {
                // The event carries only an id; what the effect does is in the state.
                let l = format!("  {} starts a continuous effect", self.name(*source));
                self.emit(l, out);
            }
            // Deliberately silent: bookkeeping with no news for a reader.
            Event::PriorityPassed { .. }
            | Event::CeasedToExist { .. }
            | Event::DamageRemoved { .. } => {}
        }
    }

    /// Targets and modes chosen for an ability just put on the stack belong to it.
    fn keep_announced(&mut self, announced: Option<ObjectId>) {
        self.just_announced = announced;
    }

    /// The line for an object changing zones; `None` if the other events say it.
    /// Hidden-zone moves (library to hand, hand to library) are named only to players who
    /// may know the card (its owner), CR 400.2.
    fn zone_line(
        &self,
        new: ObjectId,
        from: Zone,
        to: Zone,
        viewer: Option<PlayerId>,
    ) -> Option<String> {
        let n = self.name(new);
        let zone = |z: Zone| match z {
            Zone::Library => "the library",
            Zone::Hand => "hand",
            Zone::Battlefield => "the battlefield",
            Zone::Graveyard => "the graveyard",
            Zone::Stack => "the stack",
            Zone::Exile => "exile",
            Zone::Command => "the command zone",
        };
        let owner = self.owner(new);
        let hidden = |owner: Option<PlayerId>| viewer.is_some() && viewer != owner;
        let who = owner.map_or("a player".to_string(), p);
        Some(match (from, to) {
            (_, Zone::Stack) | (Zone::Stack, Zone::Graveyard) => return None,
            (Zone::Library, Zone::Hand) | (Zone::Hand, Zone::Library) => {
                let what = if hidden(owner) {
                    "a card".to_string()
                } else {
                    n
                };
                format!("  {who} puts {what} from {} into {}", zone(from), zone(to))
            }
            (Zone::Battlefield, Zone::Graveyard) => {
                if self.info.get(&new).is_some_and(|i| i.creature) {
                    format!("  {n} dies")
                } else {
                    format!("  {n} is put into the graveyard")
                }
            }
            (Zone::Stack, Zone::Battlefield) => format!("  {n} enters the battlefield"),
            (Zone::Battlefield, Zone::Exile) => format!("  {n} is exiled"),
            (Zone::Battlefield, Zone::Hand) => format!("  {n} is returned to hand"),
            (Zone::Battlefield, Zone::Library) => format!("  {n} is put into the library"),
            (_, Zone::Command) => format!("  {n} is put into the command zone"),
            (f, Zone::Battlefield) => format!("  {n} enters the battlefield from {}", zone(f)),
            (f, Zone::Graveyard) => format!("  {n} is put into the graveyard from {}", zone(f)),
            (f, Zone::Exile) => format!("  {n} is exiled from {}", zone(f)),
            (f, Zone::Hand) => format!("  {n} is returned to hand from {}", zone(f)),
            (f, Zone::Library) => format!("  {n} is put into the library from {}", zone(f)),
        })
    }
}

fn indent(l: &str) -> usize {
    l.len() - l.trim_start().len()
}

fn counter(k: card_model::CounterKind) -> &'static str {
    match k {
        card_model::CounterKind::PlusOne => "+1/+1",
        card_model::CounterKind::MinusOne => "-1/-1",
    }
}

/// "P1 draws a card" -> "P1 draws 2 cards" -> "P1 draws 3 cards" ...
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
                    line.push_str(&format!(" -> {}", ts.join(", ")));
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
    // Cards that aren't on the table: say where they are.
    for pl in &v.players {
        if let Some(o) = pl.graveyard.iter().find(|o| o.id == id) {
            return format!("{} (in {}'s graveyard)", o.name, p(pl.id));
        }
    }
    if let Some(o) = v.exile.iter().find(|o| o.id == id) {
        return format!("{} (exiled)", o.name);
    }
    if let Some(o) = v.command.iter().find(|o| o.id == id) {
        return format!("{} (command zone)", o.name);
    }
    if let Some(o) = v.revealed.iter().find(|o| o.id == id) {
        return o.name.clone();
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
        // The view names the face that is up; the option is about another one.
        OptionDesc::PlayLandFace { card, face } => {
            format!("play {} with face {face} up", view_name(v, *card))
        }
        OptionDesc::CastFace { card, face } => {
            format!("cast {} with face {face} up", view_name(v, *card))
        }
        OptionDesc::CastWith {
            card,
            face,
            ability,
        } => format!(
            "cast {} (face {face}) for its alternative cost (ability {})",
            view_name(v, *card),
            ability.index
        ),
        OptionDesc::TapForMana {
            source,
            mana,
            ability,
        } => match ability {
            None => format!("tap {} for {mana:?}", view_name(v, *source)),
            Some(a) => format!(
                "tap {} for {mana:?} ({}ability {})",
                view_name(v, *source),
                if a.granted { "granted " } else { "" },
                a.index
            ),
        },
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
        OptionDesc::ActivateAbility { source, ability } => {
            format!(
                "activate {} ({}ability {})",
                view_name(v, *source),
                if ability.granted { "granted " } else { "" },
                ability.index
            )
        }
        OptionDesc::Mode { index } => format!("mode {index}"),
        OptionDesc::Target { target } => format!("target {}", target_name(v, target)),
        OptionDesc::Mana { mana } => mana.symbol().into(),
        OptionDesc::Replacement {
            source, ability, ..
        } => {
            format!(
                "apply {}'s replacement effect ({})",
                view_name(v, *source),
                ability.index
            )
        }
        OptionDesc::Shield { id } => format!("prevention shield #{id}"),
        OptionDesc::Lasting { id, index } => format!("lasting effect #{id} (part {index})"),
        OptionDesc::Accept => "yes".into(),
        OptionDesc::Decline => "no".into(),
        OptionDesc::Trigger { source, ability } => {
            format!(
                "{}'s triggered ability ({})",
                view_name(v, *source),
                ability.index
            )
        }
        OptionDesc::CreatureType { name } => name.name().into(),
        OptionDesc::Color { color } => format!("{color:?}").to_lowercase(),
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

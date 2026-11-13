//! `PlayerView`: a redacted, per-seat projection of the game (spec §5, D-092). Agents (AI and
//! UI) only ever see this, never `GameState` (CLAUDE.md invariant 6). For a player,
//! opponents' hands and all libraries are counts only; a spectator sees public information
//! only; `Seat::Everyone` (replays) sees everything.

use crate::client::Seat;
use crate::decision::{OptionDesc, PendingDecision};
use crate::frames::Frame;
use crate::ids::CardInstanceId;
use crate::ids::{ObjectId, PlayerId};
use crate::layers::chars;
use crate::mana::ManaPool;
use crate::stack::{StackKind, Target};
use crate::state::{CombatState, GameResult, GameState};
use crate::turn::Step;
use crate::zones::Zone;
use alloc::string::String;
use alloc::vec::Vec;
use card_model::{CardType, CounterKind, Keyword};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectView {
    pub id: ObjectId,
    /// The card's definition (public card text; the AI reads it from the catalog).
    pub card: card_model::CardDefId,
    pub name: String,
    pub owner: PlayerId,
    pub controller: PlayerId,
    pub card_types: Vec<CardType>,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    pub tapped: bool,
    pub damage_marked: u32,
    pub summoning_sick: bool,
    pub keywords: Vec<Keyword>,
    pub mana_value: u32,
    pub counters: Vec<(CounterKind, u32)>,
    /// CR 903.3: a commander (public information).
    pub is_commander: bool,
    pub card_instance: CardInstanceId,
    pub attached_to: Option<ObjectId>,
    /// Abilities it has that aren't printed on its card: given to it by other objects or
    /// effects (public: the text is the giving card's).
    #[serde(default)]
    pub granted: Vec<card_model::AbilityId>,
    /// Which face of `card` is up (CR 712.6; 0 = the front face): its text and picture are
    /// those of `catalog.face(card, face)`. Players may look at both faces (CR 712.4).
    #[serde(default)]
    pub face: u8,
    /// What was chosen for it as it entered ("chosen type: Elf", CR 607.2d; public).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chosen: Vec<card_model::Chosen>,
    /// A copy (CR 707): `card` is then the card it copies (its picture and text), and
    /// this is the card or token it is itself ("Test Clone"). Public (CR 707.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub printed: Option<card_model::CardDefId>,
    /// CR 114: an emblem (in the command zone): its abilities are `granted` (written on
    /// `card`, the card whose effect made it); it has no other characteristics. Public.
    #[serde(default, skip_serializing_if = "core::ops::Not::not")]
    pub emblem: bool,
}

/// A commander designation (CR 903.3): public for the whole game, wherever the card is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommanderInfo {
    pub owner: PlayerId,
    pub card_instance: CardInstanceId,
    pub card: card_model::CardDefId,
    pub name: String,
}

/// A spell or ability on the stack.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StackView {
    pub id: ObjectId,
    pub controller: PlayerId,
    /// `Some(source)` for an activated ability; `None` for a spell.
    pub ability_of: Option<ObjectId>,
    /// The spell's name, or the name of the ability's source card.
    pub name: String,
    /// The spell's card, or the ability's source card.
    pub card: card_model::CardDefId,
    /// For abilities: the ability's text (of `card`, or of the card that gave it).
    pub ability: Option<card_model::AbilityId>,
    pub modes: Vec<u8>,
    pub x: u32,
    pub targets: Vec<Target>,
    /// The face of the spell's card that is up (CR 712.8): `catalog.face(card, face)`.
    #[serde(default)]
    pub face: u8,
    /// For a triggered ability: the public object whose event triggered it — the spell or
    /// ability a ward trigger would counter (D-089).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<ObjectId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerPublic {
    pub id: PlayerId,
    pub life: i32,
    pub has_lost: bool,
    pub hand_size: usize,
    pub library_size: usize,
    /// All unspent mana, restricted or not.
    pub mana_pool: ManaPool,
    /// How much of `mana_pool` can only be spent in certain ways (CR 106.6).
    #[serde(default)]
    pub restricted_mana: [u32; 6],
    pub graveyard: Vec<ObjectView>,
    /// CR 903.10a: combat damage taken from each commander card.
    pub commander_damage: Vec<(CardInstanceId, u32)>,
    /// CR 903.8: casts from the command zone, per commander card.
    pub commander_casts: Vec<(CardInstanceId, u32)>,
    /// The top card of the player's library, if the viewer may see it (CR 401.5): the
    /// player plays with it revealed, or it is the viewer's and they may look at it.
    #[serde(default)]
    pub library_top: Option<ObjectView>,
    /// Rules about the player in force (CR 613.10), for showing: "no maximum hand size",
    /// "may play an additional land", "hexproof".
    #[serde(default)]
    pub rules: Vec<card_model::PlayerRule>,
    /// The player's emblems (CR 114, public), in the order they got them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emblems: Vec<EmblemView>,
    /// The cards in the player's hand — only in the view of [`Seat::Everyone`] (a replay, a
    /// referee; D-092). A player's own hand is [`PlayerView::hand`]; nobody else sees it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hand: Option<Vec<ObjectView>>,
    /// The player's library, bottom to top — only in the view of [`Seat::Everyone`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<Vec<ObjectView>>,
}

/// An emblem (CR 114): an object in its owner's command zone with abilities and nothing
/// else. Public, to every seat.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmblemView {
    pub id: ObjectId,
    /// The card whose effect made it (its abilities are written on that card).
    pub card: card_model::CardDefId,
    /// That card's name ("an emblem from Elspeth, Sun's Champion"; the emblem has none).
    pub from: String,
    pub abilities: Vec<card_model::AbilityId>,
    /// What each ability says, in the order of `abilities`.
    pub text: Vec<String>,
}

/// An effect that holds for a while without being on any object (CR 611.2; public):
/// "target creature can't be blocked this turn".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastingView {
    pub id: u32,
    pub controller: PlayerId,
    /// What it does: abilities written on the card that made it.
    pub abilities: Vec<card_model::AbilityId>,
    /// The object it is about, if it is about one.
    pub about: Option<ObjectId>,
    pub until_end_of_turn: bool,
}

/// What one seat may see of the game now (D-090, D-092): with the catalog, the seat's
/// [`crate::client::EventView`] stream and the [`PendingDecision`] (in `decision`), all a
/// client gets. Built for every kind of seat by [`seat_view`], so a player's, a spectator's
/// and a replay's views can't drift apart.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerView {
    /// The player whose view this is. For [`Seat::Spectator`] and [`Seat::Everyone`], who
    /// are no player, it is player 0 (the seat a table is drawn from): read `seat`.
    pub viewer: PlayerId,
    /// Whose view this is: a player, a spectator, or everything (D-092).
    pub seat: Seat,
    pub turn_number: u32,
    pub active: PlayerId,
    pub step: Step,
    pub players: Vec<PlayerPublic>,
    /// The viewer's own hand.
    pub hand: Vec<ObjectView>,
    pub battlefield: Vec<ObjectView>,
    /// Bottom to top. Everything on the stack is public.
    pub stack: Vec<StackView>,
    pub exile: Vec<ObjectView>,
    /// The command zone (public).
    pub command: Vec<ObjectView>,
    /// CR 903.3: every commander in the game (public): owner, card, name.
    pub commanders: Vec<CommanderInfo>,
    pub combat: Option<CombatState>,
    /// The pending decision, if it is the viewer's.
    pub decision: Option<PendingDecision>,
    /// Cards in hidden zones that the viewer is looking at for the pending decision (CR
    /// 701.20e): the cards it offers that aren't otherwise in this view, and the rest of
    /// what the effect lets the player see — the library being searched (CR 701.23a), the
    /// cards a scry looks at. Empty unless the decision is the viewer's.
    #[serde(default)]
    pub revealed: Vec<ObjectView>,
    /// The effects in force that are on no object (CR 611.2).
    #[serde(default)]
    pub lasting: Vec<LastingView>,
    /// Who the game is waiting for, and for what kind of decision — public, to every seat
    /// (D-090). Its options are only in `decision`, for the player deciding: they may name
    /// cards in that player's hidden zones.
    #[serde(default)]
    pub waiting: Option<(PlayerId, crate::decision::ChoiceKind)>,
    /// How many decisions have been answered so far: it changes with every decision, so a
    /// client can tell two decisions apart (and that its own came back unchanged after an
    /// illegal response). Public: every seat is told who decides each time (`waiting`).
    #[serde(default)]
    pub decision_count: u64,
    /// How the game ended, once it has (CR 104): `winner` is `None` for a draw.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<GameResult>,
}

fn object_view(st: &GameState, id: ObjectId) -> ObjectView {
    let o = st.object(id);
    let c = chars(st, id);
    let mana_value = c.mana_value();
    ObjectView {
        id,
        // A copy shows the card it copies (CR 707.2).
        // (A double-faced token copy shows its own two faces, CR 707.8a.)
        card: o
            .copy
            .as_ref()
            .filter(|v| !v.faces)
            .map_or(o.card, |v| v.card),
        name: c.name,
        owner: o.owner,
        controller: crate::layers::controller_of(st, id),
        card_types: c.card_types.to_vec(),
        power: c.power,
        toughness: c.toughness,
        tapped: o.tapped,
        damage_marked: o.damage_marked,
        summoning_sick: o.summoning_sick,
        mana_value,
        keywords: c.keywords,
        counters: o.counters.iter().map(|(k, n)| (*k, *n)).collect(),
        is_commander: st.is_commander(id),
        card_instance: o.card_instance,
        attached_to: o.attached_to,
        granted: c.abilities.iter().copied().filter(|a| a.granted).collect(),
        face: if o.copy.as_ref().is_some_and(|v| !v.faces) {
            0
        } else {
            o.face
        },
        chosen: o.chosen.clone(),
        printed: o.copy.as_ref().map(|_| o.card),
        emblem: o.emblem.is_some(),
    }
}

/// An emblem as every seat sees it, with what its abilities say ("this" in them is the
/// emblem, CR 114.4).
pub fn emblem_view(st: &GameState, id: ObjectId) -> EmblemView {
    let o = st.object(id);
    let def = st.catalog.get(o.card);
    let abilities = chars(st, id).abilities;
    let text = abilities
        .iter()
        .map(|a| {
            card_model::describe::ability_text(def, st.catalog.ability(*a))
                .replace("this permanent", "this emblem")
                .replace("This permanent", "This emblem")
        })
        .collect();
    EmblemView {
        id,
        card: o.card,
        from: def.name.clone(),
        abilities,
        text,
    }
}

fn views(st: &GameState, ids: &[ObjectId]) -> Vec<ObjectView> {
    ids.iter().map(|&id| object_view(st, id)).collect()
}

/// What `viewer` may see of the game now (D-090): every public zone, their own hand, the
/// counts of hidden ones, the cards their own pending decision lets them look at, their own
/// decision with its options, and — for everyone — who the game waits for and for what kind
/// of decision (`waiting`). Never another player's hand, a library's cards (but a top card
/// the viewer may see, CR 401.5), or another player's options. With the viewer's redacted
/// events (`client::EventView`) it is all a client needs; agents get nothing else.
pub fn player_view(st: &GameState, viewer: PlayerId) -> PlayerView {
    seat_view(st, Seat::Player(viewer))
}

/// What `seat` may see of the game now (D-092), by one piece of code for every seat:
/// - `Seat::Player(p)`: [`player_view`] — public zones, `p`'s own hand, the top card of a
///   library `p` may see, `p`'s own decision and the hidden cards it shows them.
/// - `Seat::Spectator`: public information only — no hand, no decision (`waiting` still
///   says who decides and what kind of decision), a library's top card only if it is
///   revealed to everyone (CR 401.5).
/// - `Seat::Everyone` (a replay, a referee): everything — every hand and library
///   (`PlayerPublic::hand`/`library`), and the pending decision whoever's it is.
pub fn seat_view(st: &GameState, seat: Seat) -> PlayerView {
    let rules = crate::players::in_force(st);
    let (viewer, me) = match seat {
        Seat::Player(p) => (p, Some(p)),
        Seat::Spectator | Seat::Everyone => (PlayerId(0), None),
    };
    let all = seat == Seat::Everyone;
    let decider = st.pending.as_ref().map(|d| d.player);
    // Whose decision (and the hidden cards it shows) this seat gets.
    let decides = match seat {
        Seat::Player(p) => decider.filter(|d| *d == p),
        Seat::Spectator => None,
        Seat::Everyone => decider,
    };
    PlayerView {
        viewer,
        seat,
        decision_count: st.decision_count,
        result: st.result.clone(),
        turn_number: st.turn.turn_number,
        active: st.turn.active,
        step: st.turn.step,
        players: st
            .players
            .iter()
            .map(|p| PlayerPublic {
                id: p.id,
                life: p.life,
                has_lost: p.has_lost,
                hand_size: st.zones.hands[p.id.index()].len(),
                library_size: st.zones.libraries[p.id.index()].len(),
                mana_pool: {
                    let mut pool = p.mana_pool;
                    for r in &p.restricted_mana {
                        pool.add(r.mana, r.amount);
                    }
                    pool
                },
                restricted_mana: {
                    let mut a = [0u32; 6];
                    for r in &p.restricted_mana {
                        a[r.mana.index()] += r.amount;
                    }
                    a
                },
                graveyard: views(st, &st.zones.graveyards[p.id.index()]),
                commander_damage: p.commander_damage.iter().map(|(c, n)| (*c, *n)).collect(),
                commander_casts: p.commander_casts.iter().map(|(c, n)| (*c, *n)).collect(),
                library_top: crate::players::library_top(st, p.id)
                    .filter(|_| all || crate::players::library_top_seen_by(st, me, p.id))
                    .map(|id| object_view(st, id)),
                rules: rules
                    .iter()
                    .filter(|r| r.about(p.id))
                    .map(|r| r.rule.clone())
                    .collect(),
                emblems: crate::effects::emblems(st)
                    .into_iter()
                    .filter(|&id| st.object(id).owner == p.id)
                    .map(|id| emblem_view(st, id))
                    .collect(),
                hand: all.then(|| views(st, &st.zones.hands[p.id.index()])),
                library: all.then(|| views(st, &st.zones.libraries[p.id.index()])),
            })
            .collect(),
        hand: match me {
            Some(p) => views(st, &st.zones.hands[p.index()]),
            None => Vec::new(),
        },
        battlefield: views(st, &st.zones.battlefield),
        stack: st
            .stack
            .iter()
            .map(|so| StackView {
                id: so.id,
                controller: so.controller,
                ability_of: match so.kind {
                    StackKind::Spell => None,
                    StackKind::Ability { source, .. } => Some(source),
                },
                card: match &so.kind {
                    StackKind::Spell => st.object(so.id).card,
                    StackKind::Ability { source_card, .. } => *source_card,
                },
                ability: match &so.kind {
                    StackKind::Spell => None,
                    StackKind::Ability { ability, .. } => Some(*ability),
                },
                name: match &so.kind {
                    StackKind::Spell => st.card_def(so.id).name.clone(),
                    StackKind::Ability { source_card, .. } => {
                        st.catalog.get(*source_card).name.clone()
                    }
                },
                modes: so.modes.clone(),
                x: so.x,
                targets: so.all_targets().copied().collect(),
                face: match so.kind {
                    StackKind::Spell => st.object(so.id).face,
                    StackKind::Ability { .. } => 0,
                },
                // What a triggered ability is about, where everyone can see it: the spell
                // or ability a ward trigger would counter, a permanent that entered.
                about: so.subject.filter(|id| {
                    st.stack_object(*id).is_some()
                        || st.objects.get(id).is_some_and(|o| {
                            matches!(
                                o.zone,
                                Zone::Battlefield | Zone::Graveyard | Zone::Exile | Zone::Stack
                            )
                        })
                }),
            })
            .collect(),
        exile: views(st, &st.zones.exile),
        command: views(st, &st.zones.command),
        commanders: st
            .commanders
            .iter()
            .map(|&(owner, card_instance)| {
                let card = st
                    .commander_cards
                    .get(&card_instance)
                    .copied()
                    .unwrap_or(card_model::CardDefId(0));
                CommanderInfo {
                    owner,
                    card_instance,
                    card,
                    name: st.catalog.get(card).name.clone(),
                }
            })
            .collect(),
        combat: st.combat.clone(),
        decision: st.pending.clone().filter(|d| Some(d.player) == decides),
        waiting: st.pending.as_ref().map(|d| (d.player, d.kind)),
        revealed: match decides {
            Some(p) => views(st, &shown_to(st, p)),
            None => Vec::new(),
        },
        lasting: st
            .lasting
            .iter()
            .filter(|e| e.live(st))
            .map(|e| LastingView {
                id: e.id,
                controller: e.controller,
                abilities: e.abilities.clone(),
                about: e.about,
                until_end_of_turn: e.until_end_of_turn,
            })
            .collect(),
    }
}

/// The hidden cards the pending decision shows to `viewer` (none if it isn't theirs).
fn shown_to(st: &GameState, viewer: PlayerId) -> Vec<ObjectId> {
    let Some(d) = st.pending.as_ref().filter(|d| d.player == viewer) else {
        return Vec::new();
    };
    let hidden = |id: &ObjectId| {
        st.objects.get(id).is_some_and(|o| match o.zone {
            Zone::Library => true,
            Zone::Hand => o.owner != viewer,
            _ => false,
        })
    };
    let mut out: Vec<ObjectId> = d
        .options
        .iter()
        .filter_map(|o| match o {
            OptionDesc::Card { card } => Some(*card),
            // A card a rule lets them play from a hidden zone (the top of their library):
            // they know what they are offered.
            OptionDesc::PlayLand { card }
            | OptionDesc::CastSpell { card }
            | OptionDesc::CastWith { card, .. } => Some(*card),
            _ => None,
        })
        .filter(hidden)
        .collect();
    if let Some(Frame::Resolve(f)) = st.frames.last() {
        let mut more = f.shown(st);
        more.retain(|id| hidden(id) && !out.contains(id));
        // By name: what the player learns is which cards these are, not their order.
        more.sort_by(|a, b| (&st.card_def(*a).name, *a).cmp(&(&st.card_def(*b).name, *b)));
        out.extend(more);
    }
    out
}

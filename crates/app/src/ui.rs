//! The egui view (spec §11): a pure function of the session's `PlayerView`, the pending
//! decision and a little widget state. Every decision can be answered with the mouse alone:
//! each option is a button in the decision panel, and options that belong to a card or
//! player can also be chosen by clicking that card or player on the board.

use crate::art::Art;
use crate::cardview::{self, Badge, CardLook, Corner, Icon, Plate};
use crate::combat::Plan;
use crate::session::Session;
use card_model::describe;
use card_model::{
    AbilityDef, AsEnters, CardCatalog, CardDefId, CardType, CounterKind, ReplaceWith,
};
use eframe::egui::{self, Color32, RichText, Stroke, vec2};
use engine_core::decision::{ChoiceKind, OptionDesc, PendingDecision, Primitive, Response};
use engine_core::stack::Target;
use engine_core::turn::Step;
use engine_core::view::{ObjectView, PlayerView};
use engine_core::{ObjectId, PlayerId};
use harness::{Agents, Replay, Setup};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const HUMAN: PlayerId = PlayerId(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Game,
}

/// The setups offered in the menu.
pub const SETUPS: &[(&str, Setup)] = &[
    (
        "Commander — the four precons, 4 players (the cards that work so far)",
        Setup::Precon {
            players: 4,
            decks: harness::precon_seats(),
        },
    ),
    (
        "Commander — precons, 1 v 1",
        Setup::Precon {
            players: 2,
            decks: harness::precon_seats(),
        },
    ),
    (
        "60-card duel — real cards (Stompy vs Angels)",
        Setup::Real { players: 2 },
    ),
    (
        "60-card free-for-all — 4 players, real cards",
        Setup::Real { players: 4 },
    ),
    (
        "Commander — 4 players (test decks)",
        Setup::Commander { players: 4 },
    ),
    (
        "Commander — 1 v 1 (test decks)",
        Setup::Commander { players: 2 },
    ),
    ("60-card duel (M2 test deck)", Setup::M2 { players: 2 }),
    (
        "60-card free-for-all — 4 players (M2 test deck)",
        Setup::M2 { players: 4 },
    ),
    ("Vanilla duel (creatures and lands only)", Setup::M0Vanilla),
];

/// What a decision option is attached to on the board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Obj(ObjectId),
    Player(PlayerId),
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub session: Option<Session>,
    pub setup_index: usize,
    /// The precon each seat plays (seat 0 is the human), for the precon setups.
    pub decks: [u8; 4],
    pub seed: u64,
    pub opponents: Agents,
    pub status: String,
    pub saves: Vec<PathBuf>,
    /// Where saved games are written and listed.
    pub saves_dir: PathBuf,
    pub replay_playing: bool,
    // Widget state for the decision currently shown.
    decision_id: u64,
    pub picked: Vec<usize>,
    pub number: u32,
    pub alloc: Vec<u32>,
    pub order: Vec<usize>,
    pub focus: Option<Anchor>,
    /// Card images and mana symbols.
    pub art: Art,
    /// Pause after each thing an AI seat visibly does, in milliseconds (0 = no pause).
    pub pace_ms: u64,
    resume_at: Option<Instant>,
    /// The card under the pointer, shown enlarged next to it.
    pub zoomed: Option<Preview>,
    /// The attack or block being planned on the board (see `combat`).
    pub plan: Option<Plan>,
    /// The graveyard or exile pile being looked through.
    pub zone_view: Option<(PlayerId, Pile)>,
    /// Where to put the pile window when it next opens: beside the counter that was
    /// clicked, not over it.
    zone_place: Option<(egui::Pos2, egui::Align2)>,
    /// Also show a button for every option that can be chosen by clicking the board.
    pub list_options: bool,
    /// Animate cards turning and stepping forward.
    pub animate: bool,
    /// Where things were drawn last frame (for tests that use the pointer).
    pub card_rects: BTreeMap<ObjectId, egui::Rect>,
    pub player_rects: BTreeMap<PlayerId, egui::Rect>,
    /// The room the table had last frame (between the bars and the side panel).
    pub table_rect: egui::Rect,
}

/// A pile of cards that can be looked through.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pile {
    Graveyard,
    Exile,
}

/// The card shown enlarged next to the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preview {
    pub card: CardDefId,
    pub object: Option<ObjectId>,
}

/// How long the app waits after each visible AI action, by name.
pub const SPEEDS: &[(&str, u64)] = &[
    ("Instant", 0),
    ("Fast", 150),
    ("Normal", 450),
    ("Slow", 1000),
    ("Slower", 2000),
    ("Slowest", 4000),
];

impl Default for App {
    /// The app as a person runs it: card images on, AI seats act at a watchable speed.
    fn default() -> Self {
        App::new(saves_dir(), Art::new(true), 450)
    }
}

/// Where saved games live: `$APNAP_DATA_DIR/saves` or the platform data directory.
pub fn saves_dir() -> PathBuf {
    let base = std::env::var_os("APNAP_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("apnap")
        });
    base.join("saves")
}

enum Action {
    Respond(Response),
    PassTurn,
    ToMenu,
    Save,
    ToggleStop {
        mine: bool,
        step: Step,
    },
    AutoPay(bool),
    HoldPriority(bool),
    /// Pass for the rest of the turn unless an opponent does something.
    YieldTurn,
    Pace(u64),
    ListOptions(bool),
    /// Confirm the planned attack or block.
    Declare,
    ReplayStep(usize),
    ReplayPlay(bool),
}

impl App {
    pub fn new(saves_dir: PathBuf, art: Art, pace_ms: u64) -> Self {
        let mut app = App {
            screen: Screen::Menu,
            session: None,
            setup_index: 0,
            decks: harness::precon_seats(),
            seed: 1,
            opponents: Agents::Heuristic,
            status: String::new(),
            saves: Vec::new(),
            saves_dir,
            replay_playing: false,
            decision_id: u64::MAX,
            picked: Vec::new(),
            number: 0,
            alloc: Vec::new(),
            order: Vec::new(),
            focus: None,
            art,
            pace_ms,
            resume_at: None,
            zoomed: None,
            plan: None,
            zone_view: None,
            zone_place: None,
            list_options: false,
            animate: true,
            card_rects: BTreeMap::new(),
            player_rects: BTreeMap::new(),
            table_rect: egui::Rect::NOTHING,
        };
        app.refresh_saves();
        app
    }

    /// An app that keeps its saved games in `dir`, never touches the network or the image
    /// cache, and doesn't pause for AI actions or animate (tests).
    pub fn with_saves_dir(dir: PathBuf) -> Self {
        let mut app = App::new(dir, Art::disabled(), 0);
        app.animate = false;
        app
    }

    pub fn refresh_saves(&mut self) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.saves_dir)
            .map(|d| {
                d.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|x| x == "json"))
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        self.saves = files;
    }

    /// Real cards need their printed data in the cache. Says so (and returns false) if
    /// it isn't there yet.
    fn cards_ready(&mut self, setup: Setup) -> bool {
        if !matches!(setup, Setup::Real { .. } | Setup::Precon { .. }) {
            return true;
        }
        match harness::real_cards_ready() {
            Ok(()) => true,
            Err(e) => {
                self.status = if self.art.enabled() && self.art.online {
                    "The card data is still being downloaded from Scryfall — try again in a \
                     moment."
                        .to_string()
                } else {
                    format!("Real cards aren't available: {e}")
                };
                false
            }
        }
    }

    pub fn start_game(&mut self) {
        let setup = match SETUPS[self.setup_index].1 {
            Setup::Precon { players, .. } => Setup::Precon {
                players,
                decks: self.decks,
            },
            other => other,
        };
        if !self.cards_ready(setup) {
            return;
        }
        self.session = Some(Session::new(setup, self.seed, Some(HUMAN), self.opponents));
        self.screen = Screen::Game;
        self.status.clear();
        self.decision_id = u64::MAX;
        self.zoomed = None;
        self.plan = None;
        self.zone_view = None;
        self.resume_at = None;
    }

    /// Write the current game to the saves directory. Returns the file written.
    pub fn save_game(&mut self) -> Option<PathBuf> {
        let s = self.session.take()?;
        let path = self.save_session(&s);
        self.session = Some(s);
        path
    }

    fn save_session(&mut self, s: &Session) -> Option<PathBuf> {
        let dir = self.saves_dir.clone();
        let name = format!(
            "{}-seed{}-{}.json",
            s.setup.label(),
            s.seed,
            s.responses.len()
        );
        let path = dir.join(name);
        let result = std::fs::create_dir_all(&dir).and_then(|()| {
            std::fs::write(&path, serde_json::to_string(&s.save()).expect("serializes"))
        });
        match result {
            Ok(()) => {
                self.status = format!("Saved to {}", path.display());
                self.refresh_saves();
                Some(path)
            }
            Err(e) => {
                self.status = format!("Couldn't save: {e}");
                None
            }
        }
    }

    fn read_save(&mut self, path: &PathBuf) -> Option<Replay> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| self.status = format!("Couldn't read {}: {e}", path.display()))
            .ok()?;
        serde_json::from_str(&text)
            .map_err(|e| self.status = format!("{} isn't a save file: {e}", path.display()))
            .ok()
    }

    /// Continue a saved game as the human in seat 0.
    pub fn load_game(&mut self, path: &PathBuf) {
        if let Some(replay) = self.read_save(path)
            && self.cards_ready(replay.setup)
        {
            self.session = Some(Session::load(&replay, Some(HUMAN), self.opponents));
            self.screen = Screen::Game;
            self.decision_id = u64::MAX;
        }
    }

    /// Watch a saved game from the beginning.
    pub fn watch_replay(&mut self, path: &PathBuf) {
        if let Some(replay) = self.read_save(path)
            && self.cards_ready(replay.setup)
        {
            self.session = Some(Session::replay(&replay));
            self.screen = Screen::Game;
            self.replay_playing = false;
            self.decision_id = u64::MAX;
        }
    }
}

/// The title of a decision. Exhaustive on purpose: a new `ChoiceKind` must get a prompt.
pub fn prompt(kind: ChoiceKind) -> &'static str {
    match kind {
        ChoiceKind::ChooseStartingPlayer => "Choose who takes the first turn",
        ChoiceKind::Mulligan => "Keep this hand or mulligan?",
        ChoiceKind::MulliganBottom => "Put cards on the bottom of your library",
        ChoiceKind::Priority => "You have priority",
        ChoiceKind::ChooseTargets { .. } => "Choose a target",
        ChoiceKind::ChooseModes => "Choose a mode",
        ChoiceKind::ChooseX => "Choose the value of X",
        ChoiceKind::PayMana => "Pay mana",
        ChoiceKind::DeclareAttackers => "Declare attackers",
        ChoiceKind::DeclareBlockers => "Declare blockers",
        ChoiceKind::AssignCombatDamage => "Assign combat damage",
        ChoiceKind::OrderTriggers => "Order your triggered abilities (the last one resolves first)",
        ChoiceKind::LegendRule => "Legend rule: choose one to keep",
        ChoiceKind::ChooseReplacement => "Choose which replacement effect applies",
        ChoiceKind::CommanderToCommandZone => "Put your commander into the command zone?",
        ChoiceKind::SearchLibrary => "Search your library",
        ChoiceKind::Discard => "Discard",
        ChoiceKind::Sacrifice => "Sacrifice",
        ChoiceKind::MayAbility => "Use this ability?",
        ChoiceKind::PayUnlessCost => "Pay the cost?",
        ChoiceKind::ScryOrder => "Order the cards for the top (the first one is on top)",
        ChoiceKind::ChooseColor => "Choose a color",
        ChoiceKind::ChoosePlayer => "Choose a player",
        ChoiceKind::ChooseCards => "Choose cards",
        ChoiceKind::ScryBottom => "Scry: choose cards to put on the bottom of your library",
        ChoiceKind::SurveilGraveyard => "Surveil: choose cards to put into your graveyard",
        ChoiceKind::OrderBottom => "Order the cards for the bottom (the last one is bottom-most)",
        ChoiceKind::AuraHost => "Choose what the Aura enchants",
        ChoiceKind::AsEnters { .. } => "As it enters",
        ChoiceKind::ExileFromHand => "Exile from your hand to pay the cost",
        ChoiceKind::ReturnToHand => "Return to its owner's hand to pay the cost",
        ChoiceKind::Untap => "Choose what to untap",
        ChoiceKind::Proliferate => {
            "Proliferate: choose permanents to get another counter of each kind"
        }
    }
}

/// The card or player an option belongs to. For attack/block decisions the option is
/// anchored to what is being attacked/blocked.
pub fn anchor(o: &OptionDesc) -> Option<Anchor> {
    Some(match o {
        OptionDesc::PlayLand { card }
        | OptionDesc::CastSpell { card }
        | OptionDesc::PlayLandFace { card, .. }
        | OptionDesc::CastFace { card, .. }
        | OptionDesc::CastWith { card, .. }
        | OptionDesc::Card { card } => Anchor::Obj(*card),
        OptionDesc::ActivateAbility { source, .. }
        | OptionDesc::TapForMana { source, .. }
        | OptionDesc::Replacement { source, .. }
        | OptionDesc::Trigger { source, .. } => Anchor::Obj(*source),
        OptionDesc::DamageRecipient { object } => Anchor::Obj(*object),
        OptionDesc::Attack { defender, .. } => Anchor::Player(*defender),
        OptionDesc::Block { attacker, .. } => Anchor::Obj(*attacker),
        OptionDesc::Target { target } => match target {
            Target::Object(id) => Anchor::Obj(*id),
            Target::Player(p) => Anchor::Player(*p),
        },
        OptionDesc::Player { player } => Anchor::Player(*player),
        OptionDesc::Lasting { .. }
        | OptionDesc::Pass
        | OptionDesc::Keep
        | OptionDesc::Mulligan
        | OptionDesc::Mode { .. }
        | OptionDesc::Mana { .. }
        | OptionDesc::Shield { .. }
        | OptionDesc::Accept
        | OptionDesc::Decline => return None,
    })
}

fn find_object(v: &PlayerView, id: ObjectId) -> Option<&ObjectView> {
    v.hand
        .iter()
        .chain(&v.battlefield)
        .chain(&v.command)
        .chain(&v.exile)
        .chain(v.players.iter().flat_map(|p| &p.graveyard))
        // The top card of a library the viewer may see (CR 401.5).
        .chain(v.players.iter().filter_map(|p| p.library_top.as_ref()))
        // Cards the decision lets the player look at: a library being searched, a scry.
        .chain(&v.revealed)
        .find(|o| o.id == id)
}

/// The button label for an option.
pub fn option_label(
    v: &PlayerView,
    cat: &CardCatalog,
    d: &PendingDecision,
    o: &OptionDesc,
) -> String {
    match o {
        OptionDesc::Mode { index } => {
            // Describe the mode from the spell or ability being put on the stack.
            let text = v.stack.last().and_then(|top| {
                // The card the text is written on: an ability can be another card's gift.
                let (def, body) = match top.ability {
                    None => {
                        let def = cat.get(cat.face(top.card, top.face));
                        (def, def.spell_ability())
                    }
                    Some(a) => (
                        cat.get(a.card),
                        match cat.ability(a) {
                            AbilityDef::Activated { body, .. }
                            | AbilityDef::Triggered { body, .. } => Some(body),
                            _ => None,
                        },
                    ),
                };
                body.and_then(|b| b.modes().get(*index as usize))
                    .map(|m| describe::mode_text(def, m))
            });
            text.unwrap_or_else(|| format!("mode {index}"))
        }
        OptionDesc::Attack { defender, .. } => format!("attack P{}", defender.0),
        // CR 614.12a: "As this enters, you may pay 2 life. If you don't, it enters tapped."
        OptionDesc::Accept if matches!(d.kind, ChoiceKind::AsEnters { .. }) => match d.kind {
            ChoiceKind::AsEnters { ability, .. } => match cat.ability(ability) {
                AbilityDef::Replacement {
                    with: ReplaceWith::EntersTappedUnlessYou(AsEnters::PayLife(n)),
                    ..
                } => format!("pay {n} life"),
                _ => "Yes".into(),
            },
            _ => "Yes".into(),
        },
        OptionDesc::Decline if matches!(d.kind, ChoiceKind::AsEnters { .. }) => match d.kind {
            ChoiceKind::AsEnters { ability, .. }
                if matches!(
                    cat.ability(ability),
                    AbilityDef::Replacement {
                        with: ReplaceWith::EntersOnlyIfYou(_),
                        ..
                    }
                ) =>
            {
                "No: it goes to the graveyard".into()
            }
            _ => "No: it enters tapped".into(),
        },
        OptionDesc::Accept => "Yes".into(),
        OptionDesc::Decline => "No".into(),
        OptionDesc::Pass if d.kind == ChoiceKind::Priority => "Pass priority".into(),
        OptionDesc::TapForMana {
            source,
            mana,
            ability,
        } => {
            // Two ways to tap it for the same mana: say which ability this one is.
            let twice = d.options.iter().any(|x| {
                matches!(x, OptionDesc::TapForMana { source: s, mana: m, ability: a }
                    if s == source && m == mana && a != ability)
            });
            // A mana ability that does more than tap for one mana: say what it is.
            let more = ability.is_some_and(|a| match cat.ability(a) {
                AbilityDef::Mana {
                    cost,
                    produce,
                    then,
                    ..
                } => {
                    !cost.tap
                        || cost.mana.is_some()
                        || cost.life > 0
                        || cost.sacrifice.is_some()
                        || then.is_some()
                        || produce.count() > 1
                        || matches!(
                            produce,
                            card_model::ManaProduce::Counted { .. }
                                | card_model::ManaProduce::Reflected { .. }
                        )
                }
                _ => false,
            });
            match (find_object(v, *source), ability) {
                (Some(src), Some(a)) if twice || more => format!(
                    "{}: {} (first mana: {})",
                    src.name,
                    describe::ability_text(cat.get(a.card), cat.ability(*a)),
                    mana.symbol(),
                ),
                (Some(src), _) => format!("tap {} for {}", src.name, mana.symbol()),
                (None, _) => harness::text::option_line(v, o),
            }
        }
        // An effect in force on no object: what it says.
        OptionDesc::Lasting { id, index } => v
            .lasting
            .iter()
            .find(|e| e.id == *id)
            .and_then(|e| e.abilities.get(*index as usize))
            .map(|a| {
                let def = cat.get(a.card);
                format!(
                    "{}: {}",
                    def.name,
                    describe::ability_text(def, cat.ability(*a))
                )
            })
            .unwrap_or_else(|| harness::text::option_line(v, o)),
        // A card a rule lets them play from somewhere other than their hand: say where.
        OptionDesc::PlayLand { card } | OptionDesc::CastSpell { card }
            if !v.hand.iter().any(|o| o.id == *card)
                && !v.command.iter().any(|o| o.id == *card) =>
        {
            let verb = if matches!(o, OptionDesc::PlayLand { .. }) {
                "play"
            } else {
                "cast"
            };
            match find_object(v, *card) {
                Some(c) => format!("{verb} {} from {}", c.name, whereabouts(v, *card)),
                None => harness::text::option_line(v, o),
            }
        }
        // Casting it for an alternative cost: which one (CR 118.9).
        OptionDesc::CastWith {
            card,
            face,
            ability,
        } => match find_object(v, *card) {
            Some(c) => {
                let mut text = describe::ability_text(cat.get(ability.card), cat.ability(*ability));
                if text.chars().count() > 90 {
                    text = text.chars().take(89).chain(['…']).collect();
                }
                format!("cast {} — {text}", cat.get(cat.face(c.card, *face)).name)
            }
            None => harness::text::option_line(v, o),
        },
        // The ability itself, so that two abilities of one card (or casting it and
        // cycling it) can be told apart.
        OptionDesc::ActivateAbility { source, ability } => {
            let text = find_object(v, *source).map(|src| {
                let mut text = describe::ability_text(cat.get(ability.card), cat.ability(*ability));
                if text.chars().count() > 90 {
                    text = text.chars().take(89).chain(['…']).collect();
                }
                format!("{} — {text}", src.name)
            });
            text.unwrap_or_else(|| harness::text::option_line(v, o))
        }
        // A face that isn't up names itself (CR 712.8b, 712.9): "Play Malakir Mire".
        OptionDesc::PlayLandFace { card, face } | OptionDesc::CastFace { card, face } => {
            match find_object(v, *card) {
                Some(ov) => {
                    let name = &cat.get(cat.face(ov.card, *face)).name;
                    match o {
                        OptionDesc::PlayLandFace { .. } => format!("play {name}"),
                        _ => format!("cast {name}"),
                    }
                }
                None => harness::text::option_line(v, o),
            }
        }
        other => harness::text::option_line(v, other),
    }
}

fn step_name(s: Step) -> &'static str {
    match s {
        Step::Untap => "Untap",
        Step::Upkeep => "Upkeep",
        Step::Draw => "Draw",
        Step::PrecombatMain => "Main 1",
        Step::BeginningOfCombat => "Combat",
        Step::DeclareAttackers => "Attackers",
        Step::DeclareBlockers => "Blockers",
        Step::CombatDamage => "Damage",
        Step::EndOfCombat => "End combat",
        Step::PostcombatMain => "Main 2",
        Step::End => "End",
        Step::Cleanup => "Cleanup",
    }
}

const STOP_STEPS: [Step; 10] = [
    Step::Upkeep,
    Step::Draw,
    Step::PrecombatMain,
    Step::BeginningOfCombat,
    Step::DeclareAttackers,
    Step::DeclareBlockers,
    Step::CombatDamage,
    Step::EndOfCombat,
    Step::PostcombatMain,
    Step::End,
];

const GOLD: Color32 = Color32::from_rgb(235, 190, 60);
const BLUE: Color32 = Color32::from_rgb(110, 170, 255);
const GREEN: Color32 = Color32::from_rgb(120, 215, 130);
const RED: Color32 = Color32::from_rgb(225, 85, 75);
const DARK: Color32 = Color32::from_rgb(26, 26, 32);

/// Everything the board needs to know about the decision being shown.
struct Ctx<'a> {
    view: &'a PlayerView,
    cat: &'a CardCatalog,
    decision: Option<&'a PendingDecision>,
    focus: Option<Anchor>,
    /// Options chosen so far in a multi-select decision.
    picked: &'a [usize],
    /// The attack or block being planned, if that is what the human is doing.
    plan: Option<&'a Plan>,
    animate: bool,
}

impl Ctx<'_> {
    /// Indices of the options anchored to `a`.
    fn options_at(&self, a: Anchor) -> Vec<usize> {
        self.decision
            .map(|d| {
                d.options
                    .iter()
                    .enumerate()
                    .filter(|(_, o)| anchor(o) == Some(a))
                    .map(|(i, _)| i)
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// What the pointer did on the board this frame.
#[derive(Default)]
struct Pointer {
    /// The options of the card or player that was clicked.
    clicked: Option<Vec<usize>>,
    hovered: Option<Hover>,
    /// While planning combat: a creature that was clicked, a player that was clicked.
    plan_click: Option<ObjectId>,
    player_click: Option<PlayerId>,
    /// A graveyard or exile counter (drawn here) was clicked: open or close that pile.
    pile_click: Option<(PlayerId, Pile, egui::Rect)>,
    /// A creature being dragged, and where the pointer is.
    dragging: Option<(ObjectId, egui::Pos2)>,
    /// A creature that was dropped, and where.
    dropped: Option<(ObjectId, egui::Pos2)>,
    /// Where each permanent and each player's area and header were drawn.
    cards: BTreeMap<ObjectId, egui::Rect>,
    players: BTreeMap<PlayerId, egui::Rect>,
    headers: BTreeMap<PlayerId, egui::Rect>,
}

/// The card under the pointer.
#[derive(Clone, Copy)]
struct Hover {
    card: Preview,
    /// Where it is drawn.
    rect: egui::Rect,
    /// It is in the hand, along the bottom edge: the enlarged view goes above it.
    in_hand: bool,
}

/// Where a card is being drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Hand,
    Battlefield,
    Command,
    /// In a graveyard or in exile, shown in the pile viewer.
    Pile,
}

fn counter_name(kind: CounterKind) -> &'static str {
    match kind {
        CounterKind::PlusOne => "+1/+1",
        CounterKind::MinusOne => "-1/-1",
    }
}

/// CR 903.8: the additional generic mana a commander in the command zone costs its owner
/// to cast now ({2} for each time they have cast it from there).
fn commander_tax(v: &PlayerView, o: &ObjectView) -> u32 {
    if !o.is_commander || !v.command.iter().any(|c| c.id == o.id) {
        return 0;
    }
    v.players
        .get(o.owner.index())
        .and_then(|p| {
            p.commander_casts
                .iter()
                .find(|(c, _)| *c == o.card_instance)
        })
        .map_or(0, |(_, n)| 2 * n)
}

/// Where a card that isn't on the table is.
fn whereabouts(v: &PlayerView, id: ObjectId) -> String {
    for p in &v.players {
        if p.graveyard.iter().any(|o| o.id == id) {
            return if p.id == v.viewer {
                "your graveyard".to_string()
            } else {
                format!("P{}'s graveyard", p.id.0)
            };
        }
    }
    if v.exile.iter().any(|o| o.id == id) {
        return "exile".to_string();
    }
    for p in &v.players {
        if p.library_top.as_ref().is_some_and(|o| o.id == id) {
            return if p.id == v.viewer {
                "the top of your library".to_string()
            } else {
                format!("the top of P{}'s library", p.id.0)
            };
        }
    }
    if v.revealed.iter().any(|o| o.id == id) {
        return "library".to_string();
    }
    String::new()
}

/// A card's accessible name: everything the picture and its badges show, as text.
fn card_text(o: &ObjectView, cat: &CardCatalog) -> String {
    let def = cat.get(cat.face(o.card, o.face));
    let cost = def
        .mana_cost
        .as_ref()
        .map_or(String::new(), |m| m.to_text());
    let mut line2: Vec<String> = Vec::new();
    if let (Some(p), Some(t)) = (o.power, o.toughness) {
        line2.push(format!("{p}/{t}"));
    }
    if !o.keywords.is_empty() {
        let k: Vec<&str> = o.keywords.iter().map(|k| describe::keyword(*k)).collect();
        line2.push(k.join(", "));
    }
    let mut flags: Vec<String> = Vec::new();
    if o.tapped {
        flags.push("TAPPED".into());
    }
    if o.damage_marked > 0 {
        flags.push(format!("{} damage", o.damage_marked));
    }
    for (kind, n) in &o.counters {
        flags.push(format!(
            "{n} {} counter{}",
            counter_name(*kind),
            if *n == 1 { "" } else { "s" }
        ));
    }
    if o.summoning_sick && o.card_types.contains(&CardType::Creature) {
        flags.push("sick".into());
    }
    if o.is_commander {
        flags.push("COMMANDER".into());
    }
    let types: Vec<String> = o.card_types.iter().map(|t| format!("{t:?}")).collect();
    let mut text = format!("{}  {}\n{}", o.name, cost, types.join(" "));
    if !line2.is_empty() {
        text.push_str(&format!(" — {}", line2.join(" · ")));
    }
    if !flags.is_empty() {
        text.push_str(&format!("\n{}", flags.join(" · ")));
    }
    text
}

/// The colour of a creature's power/toughness badge — or `None` when it has exactly its
/// printed power and toughness, which the card itself already shows.
fn pt_badge(printed: (Option<i32>, Option<i32>), now: (i32, i32)) -> Option<Color32> {
    match printed {
        (Some(p), Some(t)) if (p, t) == now => None,
        (Some(p), Some(t)) if now.0 >= p && now.1 >= t => Some(GREEN),
        (Some(p), Some(t)) if now.0 <= p && now.1 <= t => Some(RED),
        // Up in one, down in the other — or nothing printed to compare with (an animated
        // land, a power set by an ability).
        _ => Some(Color32::WHITE),
    }
}

/// The state a printed card can't show, as corner badges.
fn badges(cx: &Ctx<'_>, o: &ObjectView, place: Place) -> Vec<Badge> {
    let mut out = Vec::new();
    if o.is_commander {
        out.push(Badge::new(Corner::TopLeft, "CMD", GOLD, DARK));
    }
    if place == Place::Command {
        out.push(Badge::new(
            Corner::BottomLeft,
            "command zone",
            DARK,
            Color32::from_gray(220),
        ));
        // CR 903.8: what it costs more to cast from here now.
        let tax = commander_tax(cx.view, o);
        if tax > 0 {
            out.push(Badge::new(
                Corner::TopRight,
                format!("tax +{tax}"),
                RED,
                Color32::WHITE,
            ));
        }
    }
    if place != Place::Battlefield {
        return out;
    }
    let v = cx.view;
    for (kind, n) in &o.counters {
        let (bg, fg) = match kind {
            CounterKind::PlusOne => (Color32::from_rgb(32, 84, 44), Color32::WHITE),
            CounterKind::MinusOne => (Color32::from_rgb(96, 34, 34), Color32::WHITE),
        };
        out.push(Badge::new(
            Corner::TopLeft,
            format!("{} x{n}", counter_name(*kind)),
            bg,
            fg,
        ));
    }
    let creature = o.card_types.contains(&CardType::Creature);
    let combat = v.combat.as_ref();
    // Declared (the engine's combat state) or planned (not confirmed yet).
    let attacking = combat
        .and_then(|c| c.attackers.iter().find(|a| a.attacker == o.id))
        .map(|a| a.defender)
        .or_else(|| cx.plan.and_then(|p| p.attacks(o.id)));
    let blocking = combat.is_some_and(|c| c.attackers.iter().any(|a| a.blockers.contains(&o.id)))
        || cx.plan.is_some_and(|p| p.blocks(o.id).is_some());
    if let Some(defender) = attacking {
        out.push(Badge::new(
            Corner::TopRight,
            format!("ATK P{}", defender.0),
            RED,
            Color32::WHITE,
        ));
    } else if blocking {
        out.push(Badge::new(Corner::TopRight, "BLK", BLUE, DARK));
    } else if creature && o.summoning_sick {
        out.push(Badge::new(
            Corner::TopRight,
            "sick",
            Color32::from_gray(70),
            Color32::from_gray(225),
        ));
    }
    if let Some(host) = o.attached_to {
        let name = find_object(v, host).map_or("?", |h| h.name.as_str());
        out.push(Badge::new(
            Corner::BottomLeft,
            format!("on {name}"),
            DARK,
            Color32::from_gray(220),
        ));
    }
    if o.owner != o.controller {
        out.push(Badge::new(
            Corner::BottomLeft,
            format!("P{}'s card", o.owner.0),
            DARK,
            BLUE,
        ));
    }
    if let (true, Some(p), Some(t)) = (creature, o.power, o.toughness) {
        let def = cx.cat.get(cx.cat.face(o.card, o.face));
        if let Some(fg) = pt_badge((def.power, def.toughness), (p, t)) {
            out.push(Badge::new(
                Corner::BottomRight,
                format!("{p}/{t}"),
                DARK,
                fg,
            ));
        }
        if o.damage_marked > 0 {
            out.push(Badge::new(
                Corner::BottomRight,
                format!("{} dmg", o.damage_marked),
                RED,
                Color32::WHITE,
            ));
        }
    }
    out
}

/// Draw a card — or a pile of identical ones, `group[0]` in front.
/// How far a creature in combat steps towards the middle of the table, in card heights.
const ATTACK_STEP: f32 = 0.16;
const BLOCK_STEP: f32 = 0.08;

/// Draw a card — or a pile of identical ones, `group[0]` in front. `forward` is the
/// direction (+1 down, -1 up) creatures in combat step in; 0 where that doesn't apply.
#[allow(clippy::too_many_arguments)]
fn card(
    ui: &mut egui::Ui,
    art: &mut Art,
    cx: &Ctx<'_>,
    group: &[&ObjectView],
    place: Place,
    height: f32,
    forward: f32,
    ptr: &mut Pointer,
) {
    let o = group[0];
    let opts = group
        .iter()
        .map(|m| cx.options_at(Anchor::Obj(m.id)))
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    let focused = group.iter().any(|m| cx.focus == Some(Anchor::Obj(m.id)));
    let chosen = opts.iter().any(|i| cx.picked.contains(i));
    let on_battlefield = place == Place::Battlefield;
    // Combat being planned on the board.
    let plan = cx.plan.filter(|_| on_battlefield);
    let candidate = plan.is_some_and(|p| p.candidate(o.id).is_some());
    let planned = plan.is_some_and(|p| p.choices.contains_key(&o.id));
    let waiting = plan.is_some_and(|p| p.selected == Some(o.id));
    let blockable = plan.is_some_and(|p| p.selected.is_some_and(|b| p.can_block(b, o.id)));
    let outline = if focused || chosen || waiting {
        Some(Stroke::new(3.0, Color32::WHITE))
    } else if planned {
        let color = if plan.is_some_and(Plan::attacking) {
            RED
        } else {
            BLUE
        };
        Some(Stroke::new(3.0, color))
    } else if !opts.is_empty() || candidate || blockable {
        Some(Stroke::new(2.5, GOLD))
    } else {
        None
    };
    // Attackers step forward, blockers half as far (declared or planned).
    let combat = cx.view.combat.as_ref().filter(|_| on_battlefield);
    let attacking = combat.is_some_and(|c| c.attackers.iter().any(|a| a.attacker == o.id))
        || plan.is_some_and(|p| p.attacks(o.id).is_some());
    let blocking = combat.is_some_and(|c| c.attackers.iter().any(|a| a.blockers.contains(&o.id)))
        || plan.is_some_and(|p| p.blocks(o.id).is_some());
    let step = if attacking {
        ATTACK_STEP
    } else if blocking {
        BLOCK_STEP
    } else {
        0.0
    };
    let offset = vec2(0.0, (forward * step * height).round());
    let mut label = card_text(o, cx.cat);
    let tax = commander_tax(cx.view, o);
    if tax > 0 {
        label.push_str(&format!(" · commander tax +{tax}"));
    }
    if group.len() > 1 {
        label.push_str(&format!("\nx{}", group.len()));
    }
    let look = CardLook {
        // The face that is up (CR 712.6).
        def: cx.cat.get(cx.cat.face(o.card, o.face)),
        label: &label,
        height,
        tapped: o.tapped && on_battlefield,
        pile: group.len() as u32,
        outline,
        badges: &badges(cx, o, place),
        offset,
        motion: (cx.animate && on_battlefield).then(|| egui::Id::unique(("card", o.id.0))),
        draggable: candidate,
    };
    let resp = cardview::card_widget(ui, art, &look);
    if on_battlefield {
        ptr.cards.insert(o.id, resp.rect.translate(offset));
    }
    if resp.hovered() {
        let pv = Preview {
            card: cx.cat.face(o.card, o.face),
            object: Some(o.id),
        };
        ptr.hovered = Some(Hover {
            card: pv,
            rect: resp.rect,
            in_hand: place == Place::Hand,
        });
    }
    if plan.is_some() {
        if resp.clicked() {
            ptr.plan_click = Some(o.id);
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        if let Some(pos) = pointer {
            if resp.dragged() {
                ptr.dragging = Some((o.id, pos));
            }
            if resp.drag_stopped() {
                ptr.dropped = Some((o.id, pos));
            }
        }
    } else if resp.clicked() && !opts.is_empty() {
        ptr.clicked = Some(opts);
    }
}

/// Group identical plain lands into piles (same card, same tapped state, same
/// clickability, nothing on them), keeping battlefield order.
fn piles<'a>(cx: &Ctx<'_>, lands: &[&'a ObjectView]) -> Vec<Vec<&'a ObjectView>> {
    let plain = |o: &ObjectView| {
        o.counters.is_empty()
            && o.attached_to.is_none()
            && !o.is_commander
            && o.damage_marked == 0
            && !cx
                .view
                .battlefield
                .iter()
                .any(|x| x.attached_to == Some(o.id))
    };
    let key = |o: &ObjectView| {
        (
            o.card,
            o.face,
            o.tapped,
            o.owner,
            cx.options_at(Anchor::Obj(o.id)).is_empty(),
        )
    };
    let mut out: Vec<Vec<&ObjectView>> = Vec::new();
    for &o in lands {
        let home = if plain(o) {
            out.iter_mut().find(|g| plain(g[0]) && key(g[0]) == key(o))
        } else {
            None
        };
        match home {
            Some(g) => g.push(o),
            None => out.push(vec![o]),
        }
    }
    out
}

/// A pile holding something that can be chosen right now is outlined.
fn pile_outline<'a>(
    cx: &Ctx<'_>,
    mut pile: impl Iterator<Item = &'a ObjectView>,
) -> Option<Stroke> {
    pile.any(|o| !cx.options_at(Anchor::Obj(o.id)).is_empty())
        .then_some(Stroke::new(2.0, GOLD))
}

/// What one player has on the table, in drawing order.
struct Rows<'a> {
    /// Their cards in the command zone.
    command: Vec<&'a ObjectView>,
    /// Nonland permanents (and lands that are creatures), each followed by what is
    /// attached to it.
    others: Vec<&'a ObjectView>,
    /// Lands, identical ones piled.
    lands: Vec<Vec<&'a ObjectView>>,
}

fn rows<'a>(cx: &Ctx<'a>, p: PlayerId) -> Rows<'a> {
    let v = cx.view;
    let mine = |o: &&ObjectView| o.controller == p;
    let command: Vec<&ObjectView> = v.command.iter().filter(|o| o.owner == p).collect();
    let is_plain_land = |o: &&ObjectView| {
        o.card_types.contains(&CardType::Land) && !o.card_types.contains(&CardType::Creature)
    };
    let lands: Vec<&ObjectView> = v
        .battlefield
        .iter()
        .filter(mine)
        .filter(is_plain_land)
        .collect();
    let others: Vec<&ObjectView> = v
        .battlefield
        .iter()
        .filter(mine)
        .filter(|o| !is_plain_land(o))
        .collect();
    // Each permanent is followed by what is attached to it (if that is here too).
    let mut ordered: Vec<&ObjectView> = Vec::new();
    for o in &others {
        let host_here = o
            .attached_to
            .is_some_and(|h| others.iter().any(|x| x.id == h));
        if !host_here {
            ordered.push(o);
            ordered.extend(others.iter().filter(|a| a.attached_to == Some(o.id)));
        }
    }
    Rows {
        command,
        others: ordered,
        lands: piles(cx, &lands),
    }
}

/// Space between cards in a row.
const CARD_GAP: f32 = 6.0;

impl Rows<'_> {
    /// The width of each thing in the two rows (nonlands, lands), in card heights.
    fn items(&self) -> [Vec<f32>; 2] {
        let one = |o: &ObjectView| {
            if o.tapped { 1.0 } else { cardview::CARD_ASPECT }
        };
        let mut top: Vec<f32> = self.command.iter().map(|_| cardview::CARD_ASPECT).collect();
        top.extend(self.others.iter().map(|o| one(o)));
        let lands = self
            .lands
            .iter()
            .map(|g| {
                let peek =
                    (g.len() as f32 - 1.0).min(4.0) * cardview::PILE_STEP * cardview::CARD_ASPECT;
                one(g[0]) + peek
            })
            .collect();
        [top, lands]
    }
}

/// One seat at the table, for working out how big the cards can be: how wide it is, and
/// the widths (in card heights) of the things in its two rows.
#[derive(Clone, Debug, Default)]
pub struct SeatSize {
    pub width: f32,
    pub rows: [Vec<f32>; 2],
    /// The player has left the game: the seat is just their line.
    pub out: bool,
}

/// Space a seat needs besides its cards: the player's line and the frame.
const SEAT_HEADER: f32 = 46.0;
const SEAT_MARGIN: f32 = 7.0;
const SEAT_GAP: f32 = 6.0;
/// How wide the seat of a player who has left the game is.
const OUT_SEAT_WIDTH: f32 = 260.0;
/// Card heights the table chooses between.
const CARD_MIN: f32 = 84.0;
const CARD_MAX: f32 = 380.0;

/// How many lines a row takes at card height `card` when it wraps at `width`.
fn row_lines(items: &[f32], card: f32, width: f32) -> usize {
    // A separator and a little air.
    let width = width - 24.0;
    let (mut lines, mut used) = (1, 0.0);
    for w in items {
        let w = w * card + CARD_GAP;
        if used > 0.0 && used + w > width {
            lines += 1;
            used = 0.0;
        }
        used += w;
    }
    lines
}

/// The height of the whole table (`grid` = its rows of seats) at card height `card`.
pub fn table_height(grid: &[Vec<SeatSize>], card: f32) -> f32 {
    let seat = |s: &SeatSize| {
        let lines: usize = if s.out {
            0
        } else {
            s.rows.iter().map(|r| row_lines(r, card, s.width)).sum()
        };
        SEAT_HEADER + 2.0 * SEAT_MARGIN + lines as f32 * (card + CARD_GAP) + SEAT_GAP
    };
    grid.iter()
        .map(|seats| seats.iter().map(seat).fold(0.0, f32::max) + SEAT_GAP)
        .sum()
}

/// The tallest card at which the whole table fits in `available` height — rows may wrap if
/// that lets the cards be bigger. If it can't fit even with the smallest cards, the
/// smallest it is, and the table scrolls.
pub fn fit_card_height(grid: &[Vec<SeatSize>], available: f32) -> f32 {
    let mut card = CARD_MAX;
    while card > CARD_MIN && table_height(grid, card) > available {
        card -= 4.0;
    }
    card.max(CARD_MIN)
}

/// Where the players sit: the viewer bottom left, the others clockwise in turn order —
/// along the top from left to right, then back along the bottom from the right. Returns
/// the top and the bottom row as offsets from the viewer in turn order (0 = the viewer).
/// Two players face each other; three put both opponents on top.
pub fn table_layout(players: usize) -> (Vec<usize>, Vec<usize>) {
    let cols = players.div_ceil(2).max(1);
    let top: Vec<usize> = (1..players.min(cols + 1)).collect();
    let mut bottom = vec![0];
    bottom.extend((cols + 1..players).rev());
    (top, bottom)
}

/// A row of cards that wraps, centred vertically so sideways cards line up.
fn card_row(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), height), layout, |ui| {
        ui.spacing_mut().item_spacing = vec2(CARD_GAP, CARD_GAP);
        ui.set_min_height(height);
        add(ui);
    });
}

fn player_area(
    ui: &mut egui::Ui,
    art: &mut Art,
    cx: &Ctx<'_>,
    p: PlayerId,
    height: f32,
    // In the top half of the table: lands at the far edge, creatures step downwards.
    top: bool,
    ptr: &mut Pointer,
) {
    let v = cx.view;
    let pl = &v.players[p.index()];
    let me = p == v.viewer;
    let mut head = format!(
        "P{}{} — {} life",
        p.0,
        if me { " (you)" } else { "" },
        pl.life
    );
    if pl.has_lost {
        head.push_str(" — OUT");
    }
    let opts = cx.options_at(Anchor::Player(p));
    // Planning an attack with more than one player to attack: players are clickable, and
    // the one new attackers are sent at is marked.
    let attackable = cx
        .plan
        .filter(|plan| plan.attacking())
        .map(Plan::defenders)
        .filter(|d| d.len() > 1 && d.contains(&p));
    let is_target = attackable.is_some() && cx.plan.and_then(|plan| plan.target) == Some(p);
    let who = if me {
        "your".to_string()
    } else {
        format!("P{}'s", p.0)
    };
    let cards = |n: usize| {
        if n == 1 {
            "1 card".to_string()
        } else {
            format!("{n} cards")
        }
    };
    let exiled: Vec<&ObjectView> = v.exile.iter().filter(|o| o.owner == p).collect();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let outline = if is_target {
            Some(Stroke::new(3.0, RED))
        } else if cx.focus == Some(Anchor::Player(p)) {
            Some(Stroke::new(3.0, Color32::WHITE))
        } else if attackable.is_some() || !opts.is_empty() {
            Some(Stroke::new(2.0, GOLD))
        } else {
            None
        };
        let name = if me {
            format!("P{} (you)", p.0)
        } else {
            format!("P{}", p.0)
        };
        let plate = cardview::player_plate(
            ui,
            &Plate {
                label: &head,
                name: &name,
                life: pl.life,
                active: v.active == p,
                out: pl.has_lost,
                clickable: attackable.is_some() || !opts.is_empty(),
                outline,
            },
        );
        ptr.headers.insert(p, plate.rect);
        if plate.clicked() {
            if attackable.is_some() {
                ptr.player_click = Some(p);
            } else if !opts.is_empty() {
                ptr.clicked = Some(opts);
            }
        }
        if is_target {
            ui.label(RichText::new("attacking this player").strong().color(RED));
        }
        ui.add_space(6.0);
        cardview::zone_count(
            ui,
            Icon::Hand,
            pl.hand_size,
            &format!("P{} hand: {}", p.0, cards(pl.hand_size)),
            false,
            None,
        )
        .on_hover_text(format!("Cards in {who} hand"));
        cardview::zone_count(
            ui,
            Icon::Library,
            pl.library_size,
            &format!("P{} library: {}", p.0, cards(pl.library_size)),
            false,
            None,
        )
        .on_hover_text(format!("Cards in {who} library"));
        // CR 401.5: the top card, if it is revealed or the viewer may look at it.
        if let Some(top) = &pl.library_top {
            let opts = cx.options_at(Anchor::Obj(top.id));
            let look = CardLook {
                def: cx.cat.get(top.card),
                label: &top.name,
                height: 64.0,
                tapped: false,
                pile: 1,
                outline: (!opts.is_empty()).then(|| Stroke::new(2.5, GOLD)),
                badges: &[],
                offset: egui::Vec2::ZERO,
                motion: None,
                draggable: false,
            };
            let resp = cardview::card_widget(ui, art, &look).on_hover_text(format!(
                "The top card of {who} library{}",
                if opts.is_empty() {
                    ""
                } else {
                    " — click to play it"
                }
            ));
            if resp.hovered() {
                ptr.hovered = Some(Hover {
                    card: Preview {
                        card: top.card,
                        object: Some(top.id),
                    },
                    rect: resp.rect,
                    in_hand: false,
                });
            }
            if resp.clicked() && !opts.is_empty() {
                ptr.clicked = Some(opts);
            }
        }
        // Rules about the player: "no maximum hand size", "hexproof", extra land plays.
        if !pl.rules.is_empty() {
            let text: Vec<String> = pl.rules.iter().map(player_rule_text).collect();
            ui.label(RichText::new("ⓘ").color(BLUE))
                .on_hover_text(text.join("\n"));
        }
        let n = pl.graveyard.len();
        let graveyard = cardview::zone_count(
            ui,
            Icon::Graveyard,
            n,
            &format!("P{} graveyard: {}", p.0, cards(n)),
            n > 0,
            pile_outline(cx, pl.graveyard.iter()),
        )
        .on_hover_text(if n > 0 {
            format!("Cards in {who} graveyard — click to look through it")
        } else {
            format!("Nothing in {who} graveyard")
        });
        if graveyard.clicked() {
            ptr.pile_click = Some((p, Pile::Graveyard, graveyard.rect));
        }
        // Exile is shown only when there is something in it.
        if !exiled.is_empty() {
            let exile = cardview::zone_count(
                ui,
                Icon::Exile,
                exiled.len(),
                &format!("P{} exile: {}", p.0, cards(exiled.len())),
                true,
                pile_outline(cx, exiled.iter().copied()),
            )
            .on_hover_text(format!("{who} cards in exile — click to look through them"));
            if exile.clicked() {
                ptr.pile_click = Some((p, Pile::Exile, exile.rect));
            }
        }
        if pl.mana_pool.total() > 0 {
            let mut pool = String::new();
            for t in engine_core::mana::ManaType::ALL {
                let n = pl.mana_pool.amounts[t.index()];
                if n > 6 {
                    pool.push_str(&format!("{n}x{} ", t.symbol()));
                } else {
                    for _ in 0..n {
                        pool.push_str(t.symbol());
                    }
                }
            }
            ui.add_space(6.0);
            cardview::rich_label(ui, art, pool.trim_end(), 17.0, Color32::from_gray(225));
        }
        // Commander damage this player has taken (CR 903.10a).
        for (card, n) in &pl.commander_damage {
            let name = v
                .commanders
                .iter()
                .find(|c| c.card_instance == *card)
                .map_or("a commander".to_string(), |c| c.name.clone());
            ui.label(
                RichText::new(format!("{n}/21 from {name}")).color(if *n >= 14 {
                    RED
                } else {
                    Color32::from_gray(190)
                }),
            );
        }
    });
    if pl.has_lost {
        // CR 800.4a: everything they owned has left the game; only the header remains.
        return;
    }
    let Rows {
        command,
        others: ordered,
        lands,
    } = rows(cx, p);
    let forward = if top { 1.0 } else { -1.0 };
    let draw_others = |ui: &mut egui::Ui, art: &mut Art, ptr: &mut Pointer| {
        for o in &command {
            card(ui, art, cx, &[*o], Place::Command, height, 0.0, ptr);
        }
        if !command.is_empty() && !ordered.is_empty() {
            ui.separator();
        }
        for o in &ordered {
            card(ui, art, cx, &[*o], Place::Battlefield, height, forward, ptr);
        }
    };
    let draw_lands = |ui: &mut egui::Ui, art: &mut Art, ptr: &mut Pointer| {
        for g in &lands {
            card(ui, art, cx, g, Place::Battlefield, height, forward, ptr);
        }
    };
    // Creatures face the middle of the table; lands sit behind them.
    if top {
        card_row(ui, height, |ui| draw_lands(ui, art, ptr));
        card_row(ui, height, |ui| draw_others(ui, art, ptr));
    } else {
        card_row(ui, height, |ui| draw_others(ui, art, ptr));
        card_row(ui, height, |ui| draw_lands(ui, art, ptr));
    }
}

fn menu_ui(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show(ui, |ui| {
        ui.heading("apnap");
        ui.label("A Magic: The Gathering rules engine. You are seat P0; the other seats are AI.");
        ui.add_space(8.0);
        ui.strong("Game");
        for (i, (name, _)) in SETUPS.iter().enumerate() {
            ui.radio_value(&mut app.setup_index, i, *name);
        }
        if let Setup::Precon { players, .. } = SETUPS[app.setup_index].1 {
            let names = harness::precon_names();
            ui.add_space(4.0);
            for seat in 0..(players as usize).min(app.decks.len()) {
                ui.horizontal(|ui| {
                    let who = if seat == 0 {
                        "Your deck:".to_string()
                    } else {
                        format!("P{seat}'s deck:")
                    };
                    ui.label(who);
                    for (d, name) in names.iter().enumerate() {
                        ui.radio_value(&mut app.decks[seat], d as u8, name);
                    }
                });
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let label = ui.label("Seed:");
            ui.add(egui::DragValue::new(&mut app.seed))
                .labelled_by(label.id);
            if ui.button("Random seed").clicked() {
                app.seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(1, |d| d.as_secs());
            }
        });
        ui.horizontal(|ui| {
            ui.label("Opponents:");
            ui.radio_value(&mut app.opponents, Agents::Heuristic, "Heuristic AI");
            ui.radio_value(&mut app.opponents, Agents::Random, "Random");
        });
        ui.horizontal(|ui| {
            ui.label("Opponents act:");
            for (name, ms) in SPEEDS {
                ui.radio_value(&mut app.pace_ms, *ms, *name);
            }
        });
        let mut images = app.art.enabled();
        if ui
            .checkbox(&mut images, "Card images and mana symbols (from Scryfall)")
            .changed()
        {
            app.art = if images {
                Art::new(true)
            } else {
                Art::disabled()
            };
        }
        if images {
            ui.weak(format!(
                "Downloaded as needed and kept in {}",
                card_db::scryfall::cache_dir().display()
            ));
        }
        ui.add_space(6.0);
        if ui.button(RichText::new("Start game").size(18.0)).clicked() {
            app.start_game();
        }
        ui.add_space(12.0);
        ui.strong("Saved games");
        if app.saves.is_empty() {
            ui.weak("None yet. Use Save during a game.");
        }
        let mut load = None;
        let mut watch = None;
        for path in &app.saves {
            ui.horizontal(|ui| {
                let name = path
                    .file_stem()
                    .map_or_else(String::new, |n| n.to_string_lossy().to_string());
                ui.label(&name);
                if ui.button(format!("Continue {name}")).clicked() {
                    load = Some(path.clone());
                }
                if ui.button(format!("Watch replay {name}")).clicked() {
                    watch = Some(path.clone());
                }
            });
        }
        if let Some(p) = load {
            app.load_game(&p);
        }
        if let Some(p) = watch {
            app.watch_replay(&p);
        }
        if !app.status.is_empty() {
            ui.add_space(8.0);
            ui.label(&app.status);
        }
        ui.add_space(12.0);
        ui.weak(
            "apnap is unofficial Fan Content permitted under the Fan Content Policy. Not \
             approved/endorsed by Wizards. Portions of the materials used are property of Wizards \
             of the Coast. ©Wizards of the Coast LLC. Card images and data are provided by \
             Scryfall; apnap is not produced by or endorsed by Scryfall.",
        );
    });
}

fn top_bar(
    ui: &mut egui::Ui,
    s: &Session,
    playing: bool,
    pace_ms: u64,
    list_options: bool,
    actions: &mut Vec<Action>,
) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Menu").clicked() {
            actions.push(Action::ToMenu);
        }
        if ui.button("Save").clicked() {
            actions.push(Action::Save);
        }
        ui.separator();
        let t = &s.st.turn;
        let (whose, color) = if Some(t.active) == s.human {
            ("YOUR TURN".to_string(), GREEN)
        } else {
            (format!("P{}'s turn", t.active.0), GOLD)
        };
        // A fixed width: the bar must not reflow (and the table resize) as turns change.
        let turn = RichText::new(format!("Turn {} — {whose}", t.turn_number))
            .strong()
            .size(17.0)
            .color(color);
        ui.allocate_ui_with_layout(
            vec2(230.0, 22.0),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(230.0);
                ui.add(egui::Label::new(turn).truncate());
            },
        );
        for step in STOP_STEPS {
            let text = RichText::new(step_name(step));
            if step == t.step {
                ui.label(
                    text.strong()
                        .color(GOLD)
                        .background_color(Color32::from_gray(60)),
                );
            } else {
                ui.label(text.weak());
            }
        }
        ui.separator();
        if let Some(q) = &s.replay_queue {
            ui.label(format!("Replay — {} decisions left", q.len()));
            if ui.button("Step").clicked() {
                actions.push(Action::ReplayStep(1));
            }
            if ui.button("Step 25").clicked() {
                actions.push(Action::ReplayStep(25));
            }
            if ui.button(if playing { "Pause" } else { "Play" }).clicked() {
                actions.push(Action::ReplayPlay(!playing));
            }
        } else {
            let mut auto = s.stops.auto_pay;
            if ui.checkbox(&mut auto, "Auto-pay mana").changed() {
                actions.push(Action::AutoPay(auto));
            }
            let mut list = list_options;
            if ui
                .checkbox(&mut list, "List every option")
                .on_hover_text(
                    "Also show a button for each thing you can do by clicking a card or a \
                     player.",
                )
                .changed()
            {
                actions.push(Action::ListOptions(list));
            }
            ui.separator();
            // One drop-down rather than a button per speed: the bar stays on one line.
            let label = ui.label("Opponents act:");
            let current = SPEEDS
                .iter()
                .find(|(_, ms)| *ms == pace_ms)
                .map_or("Custom", |(name, _)| *name);
            egui::ComboBox::from_id_salt("pace")
                .selected_text(current)
                .width(86.0)
                .show_ui(ui, |ui| {
                    for (name, ms) in SPEEDS {
                        if ui.selectable_label(pace_ms == *ms, *name).clicked() {
                            actions.push(Action::Pace(*ms));
                        }
                    }
                })
                .response
                .labelled_by(label.id);
        }
    });
    if s.replay_queue.is_none() {
        ui.collapsing("Stops and automatic passing", |ui| {
            let mut hold = s.stops.hold_priority;
            if ui
                .checkbox(
                    &mut hold,
                    "Hold priority after I cast a spell or use an ability",
                )
                .on_hover_text(
                    "Off: while your own spell or ability is on top of the stack, priority is \
                     passed for you so it can resolve. On: you are asked every time.",
                )
                .changed()
            {
                actions.push(Action::HoldPriority(hold));
            }
            ui.weak(
                "With an empty stack you get priority only at these steps (and whenever an \
                 opponent does something you could respond to):",
            );
            for (mine, label) in [(true, "My turn:"), (false, "Opponents' turns:")] {
                ui.horizontal_wrapped(|ui| {
                    ui.label(label);
                    let set = if mine { &s.stops.mine } else { &s.stops.theirs };
                    for step in STOP_STEPS {
                        let mut on = set.contains(&step);
                        if ui.toggle_value(&mut on, step_name(step)).changed() {
                            actions.push(Action::ToggleStop { mine, step });
                        }
                    }
                });
            }
        });
    }
}

/// Where the enlarged card goes: next to the card it enlarges, never over it, and on
/// screen. A card in the hand gets it above (the hand runs along the bottom, so its
/// neighbours stay visible); any other card gets it to the right, or to the left if there
/// is no room.
fn zoom_position(
    screen: egui::Rect,
    near: egui::Rect,
    size: egui::Vec2,
    in_hand: bool,
) -> (f32, f32) {
    let gap = 10.0;
    if in_hand && near.top() - gap - size.y >= screen.top() {
        let x = (near.center().x - size.x / 2.0)
            .min(screen.right() - size.x - 4.0)
            .max(screen.left() + 4.0);
        return (x, near.top() - gap - size.y);
    }
    let x = if near.right() + gap + size.x <= screen.right() {
        near.right() + gap
    } else if near.left() - gap - size.x >= screen.left() {
        near.left() - gap - size.x
    } else {
        screen.right() - size.x - 4.0
    };
    let y = (near.center().y - size.y / 2.0)
        .min(screen.bottom() - size.y - 4.0)
        .max(screen.top() + 4.0);
    (x, y)
}

/// The card under the pointer, enlarged right next to it (never over it), with the artist
/// credit and what the game currently says about the object.
fn zoom_popup(ctx: &egui::Context, art: &mut Art, cx: &Ctx<'_>, hover: Hover) {
    let (pv, near) = (hover.card, hover.rect);
    let screen = ctx.content_rect();
    let ppp = ctx.pixels_per_point();
    // The image's own size where the screen allows (one image pixel per screen pixel).
    let mut height = (680.0 / ppp).max(500.0).min(screen.height() - 70.0);
    let def = cx.cat.get(pv.card);
    // Players may look at both faces of a double-faced card (CR 712.4): the other face is
    // shown beside the one that is up.
    let front = cx.cat.card_of(pv.card);
    let others: Vec<&card_model::CardDef> = if cx.cat.get(front).layout.double_faced() {
        (0..=cx.cat.get(front).faces.len() as u8)
            .map(|i| cx.cat.face(front, i))
            .filter(|&e| e != pv.card)
            .map(|e| cx.cat.get(e))
            .collect()
    } else {
        Vec::new()
    };
    let (mut card, _) = cardview::card_size(height, ppp);
    let across = 1.0 + others.len() as f32;
    if card.x * across + 6.0 * across + 12.0 > screen.width() {
        height *= (screen.width() - 12.0 - 6.0 * across) / (card.x * across);
        card = cardview::card_size(height, ppp).0;
    }
    let credit = if def.token {
        art.token_credit(ctx, def)
    } else {
        art.credit(&def.name)
    };
    // What the picture can't show: the object's current state.
    let mut now: Vec<String> = Vec::new();
    // Abilities other cards give it, each once ("x2" if it has one twice).
    let mut gained: Vec<(card_model::AbilityId, usize)> = Vec::new();
    if let Some(o) = pv.object.and_then(|id| find_object(cx.view, id)) {
        for a in &o.granted {
            match gained.iter_mut().find(|(b, _)| b == a) {
                Some((_, n)) => *n += 1,
                None => gained.push((*a, 1)),
            }
        }
        if let (Some(p), Some(t)) = (o.power, o.toughness) {
            now.push(format!("{p}/{t}"));
        }
        if o.damage_marked > 0 {
            now.push(format!("{} damage", o.damage_marked));
        }
        for (kind, n) in &o.counters {
            now.push(format!("{} x{n}", counter_name(*kind)));
        }
        if !o.keywords.is_empty() {
            let k: Vec<&str> = o.keywords.iter().map(|k| describe::keyword(*k)).collect();
            now.push(k.join(", "));
        }
        if o.owner != o.controller {
            now.push(format!(
                "owned by P{}, controlled by P{}",
                o.owner.0, o.controller.0
            ));
        }
        let tax = commander_tax(cx.view, o);
        if tax > 0 {
            now.push(format!("commander tax: {{{tax}}} more to cast"));
        }
    }
    let mut gained: Vec<String> = gained
        .iter()
        .map(|(a, n)| {
            let text = describe::ability_text(cx.cat.get(a.card), cx.cat.ability(*a));
            if *n > 1 {
                format!("Has \"{text}\" x{n}")
            } else {
                format!("Has \"{text}\"")
            }
        })
        .collect();
    // Effects in force that are about this object ("can't be blocked this turn").
    for e in &cx.view.lasting {
        if e.about.is_some() && e.about == pv.object {
            for a in &e.abilities {
                let def = cx.cat.get(a.card);
                let text = describe::ability_text(def, cx.cat.ability(*a));
                let text = text.replace("That permanent", "It");
                gained.push(if e.until_end_of_turn {
                    format!("This turn ({}): {text}", def.name)
                } else {
                    format!("{}: {text}", def.name)
                });
            }
        }
    }
    let lines = usize::from(credit.is_some()) + usize::from(!now.is_empty()) + gained.len();
    let size = card
        + vec2(
            12.0 + others.len() as f32 * (card.x + 6.0),
            12.0 + lines as f32 * 18.0,
        );
    let (x, y) = zoom_position(screen, near, size, hover.in_hand);
    egui::Area::new(egui::Id::unique("zoomed card"))
        .order(egui::Order::Tooltip)
        .fixed_pos(egui::pos2(x, y))
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgb(18, 18, 22))
                .stroke(Stroke::new(1.0, Color32::from_gray(90)))
                .corner_radius(10.0)
                .inner_margin(6.0)
                .show(ui, |ui| {
                    ui.set_max_width(card.x * across + 6.0 * (across - 1.0));
                    ui.horizontal(|ui| {
                        cardview::card_large(ui, art, def, height);
                        for other in &others {
                            cardview::card_large(ui, art, other, height);
                        }
                    });
                    if let Some(credit) = credit {
                        ui.add(egui::Label::new(RichText::new(credit).small().weak()).truncate());
                    }
                    if !now.is_empty() {
                        let text = format!("Now: {}", now.join(" · "));
                        ui.add(egui::Label::new(RichText::new(text).small()).truncate());
                    }
                    for text in &gained {
                        ui.add(egui::Label::new(RichText::new(text).small()).truncate());
                    }
                });
        });
}

fn side_panel(ui: &mut egui::Ui, art: &mut Art, cx: &Ctx<'_>, s: &Session, ptr: &mut Pointer) {
    let v = cx.view;
    // Image loading is reported here, in a row that is always there, so that nothing
    // moves when it comes and goes.
    ui.horizontal(|ui| {
        ui.strong("Stack (top first)");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if art.pending > 0 {
                ui.weak(format!("loading {} image(s)…", art.pending));
            } else if let Some(e) = &art.last_error {
                ui.weak("some images unavailable").on_hover_text(e);
            }
        });
    });
    if v.stack.is_empty() {
        ui.weak("empty");
    }
    for so in v.stack.iter().rev() {
        let mut line = match so.ability_of {
            Some(_) => format!("ability of {} (P{})", so.name, so.controller.0),
            None => format!("{} (P{})", so.name, so.controller.0),
        };
        if !so.targets.is_empty() {
            let ts: Vec<String> = so
                .targets
                .iter()
                .map(|t| match t {
                    Target::Player(p) => format!("P{}", p.0),
                    Target::Object(id) => find_object(v, *id).map_or_else(
                        || {
                            v.stack
                                .iter()
                                .find(|x| x.id == *id)
                                .map_or("?".to_string(), |x| x.name.clone())
                        },
                        |o| o.name.clone(),
                    ),
                })
                .collect();
            line.push_str(&format!(" -> {}", ts.join(", ")));
        }
        ui.horizontal(|ui| {
            let opts = cx.options_at(Anchor::Obj(so.id));
            let look = CardLook {
                def: cx.cat.get(cx.cat.face(so.card, so.face)),
                label: &format!("on the stack: {line}"),
                height: 96.0,
                tapped: false,
                pile: 1,
                outline: (!opts.is_empty()).then_some(Stroke::new(2.5, GOLD)),
                badges: &[],
                offset: egui::Vec2::ZERO,
                motion: None,
                draggable: false,
            };
            let resp = cardview::card_widget(ui, art, &look);
            if resp.hovered() {
                let pv = Preview {
                    card: cx.cat.face(so.card, so.face),
                    object: None,
                };
                ptr.hovered = Some(Hover {
                    card: pv,
                    rect: resp.rect,
                    in_hand: false,
                });
            }
            if resp.clicked() && !opts.is_empty() {
                ptr.clicked = Some(opts);
            }
            ui.add(egui::Label::new(line).wrap());
        });
    }
    ui.separator();
    ui.strong("Log");
    egui::ScrollArea::vertical()
        .id_salt("log")
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            let start = s.log.len().saturating_sub(400);
            for line in &s.log[start..] {
                if line.starts_with("---") {
                    ui.label(RichText::new(line).strong().color(GOLD));
                } else {
                    ui.label(RichText::new(line).size(12.0));
                }
            }
            // Room below the last line, so it isn't flush against the window's edge.
            ui.add_space(28.0);
        });
}

/// Room always kept for the prompt: its title and one row of buttons.
const PROMPT_HEIGHT: f32 = 60.0;

/// The main thing to do right now: a big button, also triggered by the space bar.
fn primary_button(ui: &mut egui::Ui, label: &str) -> bool {
    let text = RichText::new(label)
        .strong()
        .size(15.0)
        .color(Color32::WHITE);
    let button = egui::Button::new(text)
        .fill(Color32::from_rgb(172, 92, 22))
        .min_size(vec2(0.0, 30.0));
    let clicked = ui.add(button).on_hover_text("Space").clicked();
    let space = !ui.ctx().egui_wants_keyboard_input()
        && ui.input(|i| i.key_pressed(egui::Key::Space) && i.modifiers.is_none());
    clicked || space
}

/// Is the thing an option belongs to drawn on the board, where it can be clicked?
fn on_board(v: &PlayerView, a: Anchor) -> bool {
    match a {
        Anchor::Player(p) => p.index() < v.players.len(),
        Anchor::Obj(id) => {
            v.hand.iter().any(|o| o.id == id)
                || v.battlefield.iter().any(|o| o.id == id)
                || v.command.iter().any(|o| o.id == id)
                || v.stack.iter().any(|s| s.id == id)
        }
    }
}

/// Planning an attack or a block: the creatures are chosen on the board; this is the hint
/// and the buttons.
fn plan_panel(app: &mut App, ui: &mut egui::Ui, cx: &Ctx<'_>, actions: &mut Vec<Action>) {
    let Some(plan) = app.plan.as_mut() else {
        return;
    };
    let name = |id: ObjectId| find_object(cx.view, id).map_or("it", |o| o.name.as_str());
    let (title, hint) = if plan.attacking() {
        let hint = match (plan.defenders().len(), plan.target) {
            (n, Some(t)) if n > 1 => format!(
                "Click your creatures to attack P{}. Click another player's name — or drag a \
                 creature onto their side — to attack someone else.",
                t.0
            ),
            _ => "Click the creatures that attack.".to_string(),
        };
        ("Declare attackers", hint)
    } else {
        let hint = match plan.selected {
            Some(b) => format!("Now click the attacker {} blocks.", name(b)),
            None => "Click a creature, then the attacker it blocks — or drag it onto the \
                     attacker."
                .to_string(),
        };
        ("Declare blockers", hint)
    };
    ui.strong(RichText::new(title).size(15.0).color(GOLD));
    ui.horizontal_wrapped(|ui| {
        if primary_button(ui, &plan.confirm_label()) {
            actions.push(Action::Declare);
        }
        if plan.attacking()
            && plan.choices.len() < plan.candidates.len()
            && ui.button("All attack").clicked()
        {
            plan.all_attack();
        }
        if !plan.choices.is_empty() && ui.button("Clear").clicked() {
            plan.clear();
        }
        ui.weak(hint);
    });
}

/// The card an option is about, if that card isn't on the table (it is in a graveyard, in
/// exile, or shown from a library) and no other option is about it: such options are
/// picked from a row of the cards themselves.
fn off_table_card<'a>(cx: &Ctx<'a>, d: &PendingDecision, i: usize) -> Option<&'a ObjectView> {
    let a = anchor(&d.options[i])?;
    let Anchor::Obj(id) = a else {
        return None;
    };
    if on_board(cx.view, a) || d.options.iter().filter(|o| anchor(o) == Some(a)).count() != 1 {
        return None;
    }
    find_object(cx.view, id)
}

/// A row of cards to pick from (see [`off_table_card`]). Returns the option clicked.
fn card_strip(
    ui: &mut egui::Ui,
    art: &mut Art,
    cx: &Ctx<'_>,
    items: &[(usize, &ObjectView, &str)],
    ptr: &mut Pointer,
) -> Option<usize> {
    let mut clicked = None;
    egui::ScrollArea::horizontal()
        .id_salt("card picker")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (i, o, label) in items {
                    ui.vertical(|ui| {
                        let chosen = cx.picked.contains(i);
                        let look = CardLook {
                            def: cx.cat.get(cx.cat.face(o.card, o.face)),
                            label,
                            height: 150.0,
                            tapped: false,
                            pile: 1,
                            outline: Some(if chosen {
                                Stroke::new(3.5, GREEN)
                            } else {
                                Stroke::new(2.5, GOLD)
                            }),
                            badges: &[],
                            offset: egui::Vec2::ZERO,
                            motion: None,
                            draggable: false,
                        };
                        let resp = cardview::card_widget(ui, art, &look);
                        if resp.hovered() {
                            ptr.hovered = Some(Hover {
                                card: Preview {
                                    card: cx.cat.face(o.card, o.face),
                                    object: Some(o.id),
                                },
                                rect: resp.rect,
                                in_hand: true,
                            });
                        }
                        if resp.clicked() {
                            clicked = Some(*i);
                        }
                        ui.add(
                            egui::Label::new(
                                RichText::new(whereabouts(cx.view, o.id)).small().weak(),
                            )
                            .truncate(),
                        );
                    });
                }
            });
        });
    clicked
}

/// The decision panel: prompt plus a button for every option, by primitive.
fn decision_panel(
    app: &mut App,
    art: &mut Art,
    ui: &mut egui::Ui,
    cx: &Ctx<'_>,
    actions: &mut Vec<Action>,
    ptr: &mut Pointer,
) {
    if app.plan.is_some() {
        plan_panel(app, ui, cx, actions);
        return;
    }
    let Some(d) = cx.decision else {
        return;
    };
    // Options that belong to a card or player on the board are chosen there; they get a
    // button too only if the player asks for the full list.
    let listed =
        |i: usize| app.list_options || !anchor(&d.options[i]).is_some_and(|a| on_board(cx.view, a));
    let mut title = prompt(d.kind).to_string();
    if let (
        ChoiceKind::ChooseTargets { .. }
        | ChoiceKind::ChooseModes
        | ChoiceKind::ChooseX
        | ChoiceKind::PayMana
        | ChoiceKind::PayUnlessCost
        | ChoiceKind::MayAbility,
        Some(top),
    ) = (d.kind, cx.view.stack.last())
    {
        title.push_str(&format!(" for {}", top.name));
    }
    // "As Test Shockland enters, you may pay 2 life. If you don't, it enters tapped."
    if let ChoiceKind::AsEnters {
        object,
        ability,
        tapped,
    } = d.kind
    {
        let name = find_object(cx.view, object).map_or("it".to_string(), |o| o.name.clone());
        title = describe::ability_text(cx.cat.get(ability.card), cx.cat.ability(ability))
            .replace("this permanent", &name);
        if tapped {
            title.push_str(" (It enters tapped anyway.)");
        }
    }
    ui.strong(RichText::new(title).size(15.0).color(GOLD));
    let labels: Vec<String> = d
        .options
        .iter()
        .map(|o| option_label(cx.view, cx.cat, d, o))
        .collect();
    match &d.prim {
        Primitive::PickN { min, max } if *max <= 1 => {
            let shown: Vec<usize> = match app.focus {
                Some(a) => (0..labels.len())
                    .filter(|&i| anchor(&d.options[i]) == Some(a))
                    .collect(),
                None => (0..labels.len()).filter(|&i| listed(i)).collect(),
            };
            let on_the_board = match app.focus {
                Some(_) => 0,
                None => labels.len() - shown.len(),
            };
            // Cards that aren't on the table are shown as cards.
            let strip: Vec<(usize, &ObjectView, &str)> = shown
                .iter()
                .filter_map(|&i| off_table_card(cx, d, i).map(|o| (i, o, labels[i].as_str())))
                .collect();
            if !strip.is_empty()
                && let Some(i) = card_strip(ui, art, cx, &strip, ptr)
            {
                actions.push(Action::Respond(Response::Pick(vec![i])));
            }
            let shown: Vec<usize> = shown
                .into_iter()
                .filter(|i| !strip.iter().any(|(s, _, _)| s == i))
                .collect();
            ui.horizontal_wrapped(|ui| {
                for i in shown {
                    // Passing priority is the main button (and the space bar).
                    let pass = d.kind == ChoiceKind::Priority && d.options[i] == OptionDesc::Pass;
                    let clicked = if pass {
                        primary_button(ui, &labels[i])
                    } else {
                        cardview::rich_button(ui, art, &labels[i], false).clicked()
                    };
                    if clicked {
                        actions.push(Action::Respond(Response::Pick(vec![i])));
                    }
                }
                if *min == 0 {
                    let none = match d.kind {
                        ChoiceKind::DeclareAttackers => "Don't attack",
                        ChoiceKind::DeclareBlockers => "Don't block",
                        _ => "None",
                    };
                    if ui.button(none).clicked() {
                        actions.push(Action::Respond(Response::Pick(vec![])));
                    }
                }
                if app.focus.is_some() && ui.button("Show all options").clicked() {
                    app.focus = None;
                }
                if d.kind == ChoiceKind::Priority {
                    // MTGO's yields: F4 = until an opponent does something, F6 = no
                    // matter what.
                    let ctx = ui.ctx().clone();
                    let key = |k: egui::Key| {
                        !ctx.egui_wants_keyboard_input()
                            && ctx.input(|i| i.key_pressed(k) && i.modifiers.is_none())
                    };
                    if ui
                        .button("Pass unless an opponent acts")
                        .on_hover_text(
                            "F4 — pass priority for the rest of this turn (no attack), but \
                             stop when an opponent casts a spell or uses an ability.",
                        )
                        .clicked()
                        || key(egui::Key::F4)
                    {
                        actions.push(Action::YieldTurn);
                    }
                    if ui
                        .button("Pass until end of turn")
                        .on_hover_text(
                            "F6 — pass priority for the rest of this turn (no attack), whatever \
                             happens. You are still asked how to block.",
                        )
                        .clicked()
                        || key(egui::Key::F6)
                    {
                        actions.push(Action::PassTurn);
                    }
                }
                if on_the_board > 0 {
                    ui.weak(match d.kind {
                        ChoiceKind::Priority => {
                            "Cards you can play or use are outlined in gold: click one."
                        }
                        ChoiceKind::ChooseTargets { .. } => {
                            "Click a target: they are outlined in gold."
                        }
                        _ => "Click one of the cards or players outlined in gold.",
                    });
                }
            });
        }
        Primitive::PickN { min, max } => {
            ui.label(format!(
                "Choose {min}{} — {} chosen",
                if min == max {
                    String::new()
                } else {
                    format!(" to {max}")
                },
                app.picked.len()
            ));
            let strip: Vec<(usize, &ObjectView, &str)> = (0..labels.len())
                .filter(|&i| listed(i))
                .filter_map(|i| off_table_card(cx, d, i).map(|o| (i, o, labels[i].as_str())))
                .collect();
            if !strip.is_empty()
                && let Some(i) = card_strip(ui, art, cx, &strip, ptr)
            {
                if app.picked.contains(&i) {
                    app.picked.retain(|x| *x != i);
                } else {
                    app.picked.push(i);
                }
            }
            ui.horizontal_wrapped(|ui| {
                let mut on_the_board = false;
                for (i, label) in labels.iter().enumerate() {
                    if !listed(i) {
                        on_the_board = true;
                        continue;
                    }
                    if strip.iter().any(|(s, _, _)| *s == i) {
                        continue;
                    }
                    let on = app.picked.contains(&i);
                    if cardview::rich_button(ui, art, label, on).clicked() {
                        if on {
                            app.picked.retain(|x| *x != i);
                        } else {
                            app.picked.push(i);
                        }
                    }
                }
                let ok = (*min..=*max).contains(&app.picked.len());
                let confirmed = if ok {
                    primary_button(ui, "Confirm")
                } else {
                    ui.add_enabled(false, egui::Button::new("Confirm"));
                    false
                };
                if confirmed {
                    let mut p = app.picked.clone();
                    p.sort_unstable();
                    actions.push(Action::Respond(Response::Pick(p)));
                }
                if on_the_board {
                    ui.weak("Click the cards outlined in gold; click again to take one back.");
                }
            });
        }
        Primitive::PickNumber { min, max } => {
            app.number = app.number.clamp(*min, *max);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(app.number > *min, egui::Button::new("-"))
                    .clicked()
                {
                    app.number -= 1;
                }
                ui.strong(RichText::new(app.number.to_string()).size(18.0));
                if ui
                    .add_enabled(app.number < *max, egui::Button::new("+"))
                    .clicked()
                {
                    app.number += 1;
                }
                if ui.button(format!("Min ({min})")).clicked() {
                    app.number = *min;
                }
                if ui.button(format!("Max ({max})")).clicked() {
                    app.number = *max;
                }
                if ui.button("Confirm").clicked() {
                    actions.push(Action::Respond(Response::Number(app.number)));
                }
            });
        }
        Primitive::Allocate {
            total,
            per_min,
            per_max,
        } => {
            if app.alloc.len() != labels.len() {
                app.alloc = per_min.clone();
            }
            let sum: u32 = app.alloc.iter().sum();
            ui.label(format!("Distribute {total} — {sum} assigned"));
            for (i, label) in labels.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            app.alloc[i] > per_min[i],
                            egui::Button::new(format!("- {label}")),
                        )
                        .clicked()
                    {
                        app.alloc[i] -= 1;
                    }
                    ui.strong(app.alloc[i].to_string());
                    if ui
                        .add_enabled(
                            app.alloc[i] < per_max[i] && sum < *total,
                            egui::Button::new(format!("+ {label}")),
                        )
                        .clicked()
                    {
                        app.alloc[i] += 1;
                    }
                });
            }
            if ui
                .add_enabled(sum == *total, egui::Button::new("Confirm"))
                .clicked()
            {
                actions.push(Action::Respond(Response::Allocate(app.alloc.clone())));
            }
        }
        Primitive::Order => {
            if app.order.len() != labels.len() {
                app.order = (0..labels.len()).collect();
            }
            for pos in 0..app.order.len() {
                ui.horizontal(|ui| {
                    ui.label(format!("{}. {}", pos + 1, labels[app.order[pos]]));
                    if ui
                        .add_enabled(pos > 0, egui::Button::new(format!("Up {}", pos + 1)))
                        .clicked()
                    {
                        app.order.swap(pos, pos - 1);
                    }
                    if ui
                        .add_enabled(
                            pos + 1 < app.order.len(),
                            egui::Button::new(format!("Down {}", pos + 1)),
                        )
                        .clicked()
                    {
                        app.order.swap(pos, pos + 1);
                    }
                });
            }
            if ui.button("Confirm").clicked() {
                actions.push(Action::Respond(Response::Order(app.order.clone())));
            }
        }
    }
}

/// Let the AI seats (and the automatic passes) run, pausing after each thing an AI seat
/// visibly does so the human can follow the game.
fn advance(app: &mut App, session: &mut Session, ctx: &egui::Context) {
    if session.replay_queue.is_some() {
        if app.replay_playing && session.pump(3) > 0 {
            ctx.request_repaint();
        }
        return;
    }
    if app.pace_ms == 0 {
        app.resume_at = None;
        session.pump(5_000);
    } else {
        let now = Instant::now();
        if app.resume_at.is_none_or(|t| now >= t) {
            app.resume_at = session
                .pump_visible(5_000)
                .then(|| now + Duration::from_millis(app.pace_ms));
        }
        if let Some(t) = app.resume_at {
            ctx.request_repaint_after(t.saturating_duration_since(now));
            return;
        }
    }
    if !session.is_over() && session.human_decision().is_none() {
        ctx.request_repaint();
    }
}

fn game_ui(app: &mut App, ui: &mut egui::Ui) {
    let Some(mut session) = app.session.take() else {
        app.screen = Screen::Menu;
        return;
    };
    // The art is lent to the drawing code for this frame.
    let mut art = std::mem::replace(&mut app.art, Art::disabled());
    let to_menu = game_frame(app, &mut session, &mut art, ui);
    app.art = art;
    app.session = Some(session);
    if to_menu {
        app.screen = Screen::Menu;
        app.refresh_saves();
    }
}

/// One frame of the game screen. Returns true to go back to the menu.
fn game_frame(app: &mut App, session: &mut Session, art: &mut Art, ui: &mut egui::Ui) -> bool {
    advance(app, session, ui.ctx());
    if session.st.decision_count != app.decision_id {
        app.decision_id = session.st.decision_count;
        app.picked.clear();
        app.alloc.clear();
        app.order.clear();
        app.number = 0;
        app.focus = None;
    }
    let view = session.view();
    let decision = session.human_decision().cloned();
    // Attacks and blocks are planned on the board as a whole (`combat`), not answered one
    // creature at a time.
    let planning = decision.as_ref().is_some_and(|d| {
        matches!(
            d.kind,
            ChoiceKind::DeclareAttackers | ChoiceKind::DeclareBlockers
        )
    });
    if !planning {
        app.plan = None;
    } else if app
        .plan
        .as_ref()
        .is_none_or(|p| p.decision_id != session.st.decision_count)
    {
        app.plan = session
            .declaration()
            .map(|d| Plan::new(session.st.decision_count, d));
    }
    let cat = session.st.catalog.clone();
    let picked = app.picked.clone();
    let plan = app.plan.clone();
    let cx = Ctx {
        view: &view,
        cat: &cat,
        // While planning, the per-creature question itself isn't shown.
        decision: decision.as_ref().filter(|_| plan.is_none()),
        focus: app.focus,
        picked: &picked,
        plan: plan.as_ref(),
        animate: app.animate,
    };
    let mut actions: Vec<Action> = Vec::new();
    let mut ptr = Pointer::default();
    let window = ui.max_rect().size();

    egui::Panel::top("top").show(ui, |ui| {
        top_bar(
            ui,
            session,
            app.replay_playing,
            app.pace_ms,
            app.list_options,
            &mut actions,
        );
    });
    egui::Panel::right("side")
        .default_size(280.0)
        .show(ui, |ui| {
            side_panel(ui, art, &cx, session, &mut ptr);
        });
    egui::Panel::bottom("bottom").show(ui, |ui| {
        // The prompt and its buttons get the same room whether or not there is anything
        // to decide, so the table above doesn't resize every time a decision comes or goes.
        let prompt_top = ui.cursor().top();
        if let Some(result) = &session.st.result {
            let text = match result.winner {
                Some(w) if Some(w) == session.human => "Game over — you win!".to_string(),
                Some(w) => format!("Game over — P{} wins", w.0),
                None => "Game over — it's a draw".to_string(),
            };
            ui.strong(RichText::new(text).size(18.0).color(GOLD));
        } else if decision.is_some() {
            decision_panel(app, art, ui, &cx, &mut actions, &mut ptr);
        } else if session.replay_queue.is_some() {
            ui.weak("Replay: use Step or Play above.");
        } else {
            let who = session
                .st
                .pending
                .as_ref()
                .map_or("Opponents are".to_string(), |d| {
                    format!("P{} is", d.player.0)
                });
            ui.weak(format!("{who} acting…"));
        }
        if !app.status.is_empty() {
            ui.small(&app.status);
        }
        let used = ui.cursor().top() - prompt_top;
        if used < PROMPT_HEIGHT {
            ui.add_space(PROMPT_HEIGHT - used);
        }
        ui.separator();
        ui.small("Your hand");
        // The hand fans out: cards overlap when there are too many to lay side by side.
        // An empty hand takes no room, so the board gets it.
        let n = view.hand.len();
        let hand_h = if n == 0 {
            0.0
        } else {
            (window.y * 0.2).clamp(120.0, 300.0)
        };
        let w = cardview::card_size(hand_h, ui.pixels_per_point()).0.x;
        let (row, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), hand_h), egui::Sense::hover());
        let step = if n > 1 {
            ((row.width() - w) / (n - 1) as f32).clamp(24.0, w + 6.0)
        } else {
            0.0
        };
        for (i, o) in view.hand.iter().enumerate() {
            let at = egui::Rect::from_min_size(
                row.min + vec2((i as f32 * step).round(), 0.0),
                vec2(w, hand_h),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
                card(ui, art, &cx, &[o], Place::Hand, hand_h, 0.0, &mut ptr);
            });
        }
        ui.add_space(4.0);
    });
    egui::CentralPanel::default().show(ui, |ui| {
        app.table_rect = ui.max_rect();
        let n = view.players.len();
        let seat = |k: usize| PlayerId(((view.viewer.index() + k) % n) as u8);
        let (top, bottom) = table_layout(n);
        let grid: Vec<(Vec<PlayerId>, bool)> = [(top, true), (bottom, false)]
            .into_iter()
            .filter(|(seats, _)| !seats.is_empty())
            .map(|(seats, is_top)| (seats.into_iter().map(seat).collect(), is_top))
            .collect();
        // The players still in the game share their row equally; a player who is out
        // keeps only a narrow seat.
        let gap = SEAT_GAP;
        let margin = SEAT_MARGIN; // the frame around a seat: its inner margin and its line
        let out = |p: &PlayerId| view.players[p.index()].has_lost;
        let mut widths = BTreeMap::new();
        for (seats, _) in &grid {
            let total = ui.available_width() - gap * (seats.len() as f32 - 1.0);
            let equal = (total / seats.len() as f32).floor();
            let gone = seats.iter().filter(|p| out(p)).count();
            let narrow = equal.min(OUT_SEAT_WIDTH);
            let live = seats.len() - gone;
            let wide = if live == 0 {
                equal
            } else {
                ((total - narrow * gone as f32) / live as f32).floor()
            };
            for p in seats {
                let w = if out(p) && live > 0 { narrow } else { wide };
                widths.insert(*p, w - 2.0 * margin);
            }
        }
        // Every seat has two rows of cards (which may wrap); the cards are as big as lets
        // the whole table fit.
        let sizes: Vec<Vec<SeatSize>> = grid
            .iter()
            .map(|(seats, _)| {
                seats
                    .iter()
                    .map(|p| SeatSize {
                        width: widths[p],
                        rows: if out(p) {
                            Default::default()
                        } else {
                            rows(&cx, *p).items()
                        },
                        out: out(p),
                    })
                    .collect()
            })
            .collect();
        let height = fit_card_height(&sizes, ui.available_height());
        let seat_height = SEAT_HEADER + 2.0 * (height + gap);
        egui::ScrollArea::vertical()
            .id_salt("board")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = vec2(gap, gap);
                for (seats, is_top) in &grid {
                    ui.horizontal_top(|ui| {
                        for &p in seats {
                            let mut frame = egui::Frame::group(ui.style()).inner_margin(6.0);
                            if view.active == p {
                                frame = frame.stroke(Stroke::new(1.5, GOLD));
                            }
                            let inner = frame.show(ui, |ui| {
                                // (The frame inherits the row's left-to-right layout.)
                                ui.vertical(|ui| {
                                    ui.set_width(widths[&p]);
                                    if !out(&p) {
                                        ui.set_min_height(seat_height);
                                    }
                                    player_area(ui, art, &cx, p, height, *is_top, &mut ptr);
                                });
                            });
                            ptr.players.insert(p, inner.response.rect);
                        }
                    });
                }
            });
    });

    // Combat planned with the pointer: clicks, then anything dragged and dropped.
    if let Some(plan) = app.plan.as_mut() {
        if let Some(id) = ptr.plan_click {
            if plan.candidate(id).is_some() {
                plan.click_creature(id);
            } else {
                plan.click_attacker(id);
            }
        }
        if let Some(p) = ptr.player_click {
            plan.click_player(p);
        }
        if let Some((id, pos)) = ptr.dropped {
            if plan.attacking() {
                let onto = ptr.players.iter().find(|(_, r)| r.contains(pos));
                if let Some((&p, _)) = onto {
                    plan.drop_on_player(id, p);
                }
            } else {
                let onto = ptr
                    .cards
                    .iter()
                    .find(|(other, r)| **other != id && r.contains(pos));
                if let Some((&other, _)) = onto {
                    plan.drop_on_creature(id, other);
                }
            }
        }
    }
    combat_arrows(ui.ctx(), &view, app.plan.as_ref(), &ptr);
    if let Some((p, pile, counter)) = ptr.pile_click {
        // Clicking the counter of the pile that is open closes it.
        app.zone_view = (app.zone_view != Some((p, pile))).then_some((p, pile));
        // The window opens below a counter in the top half of the screen and above one in
        // the bottom half, so it never hides the counter.
        let screen = ui.ctx().content_rect();
        app.zone_place = Some(if counter.center().y < screen.center().y {
            (
                counter.left_bottom() + vec2(0.0, 8.0),
                egui::Align2::LEFT_TOP,
            )
        } else {
            (
                counter.left_top() - vec2(0.0, 8.0),
                egui::Align2::LEFT_BOTTOM,
            )
        });
    }
    let place = app.zone_place.take();
    pile_window(ui.ctx(), art, &cx, &mut app.zone_view, place, &mut ptr);
    app.card_rects = std::mem::take(&mut ptr.cards);
    app.player_rects = std::mem::take(&mut ptr.players);

    app.zoomed = ptr.hovered.map(|h| h.card);
    if let Some(hover) = ptr.hovered {
        zoom_popup(ui.ctx(), art, &cx, hover);
    }
    // A click on a card or player: choose its option, or focus it if it has several.
    if let (Some(opts), Some(d)) = (ptr.clicked, decision.as_ref()) {
        let single_pick = matches!(d.prim, Primitive::PickN { max, .. } if max <= 1);
        match (&d.prim, opts.as_slice()) {
            (_, [i]) if single_pick => actions.push(Action::Respond(Response::Pick(vec![*i]))),
            (Primitive::PickN { .. }, [i]) => {
                if app.picked.contains(i) {
                    app.picked.retain(|x| x != i);
                } else {
                    app.picked.push(*i);
                }
            }
            (_, [first, ..]) if single_pick => app.focus = anchor(&d.options[*first]),
            _ => {}
        }
    }
    for a in actions {
        match a {
            Action::Respond(r) => {
                if let Err(e) = session.respond(r) {
                    app.status = format!("That isn't a legal answer: {e}");
                } else {
                    app.status.clear();
                }
            }
            Action::PassTurn => session.pass_the_turn(),
            Action::ToMenu => return true,
            Action::Save => {
                app.save_session(session);
            }
            Action::ToggleStop { mine, step } => {
                let set = if mine {
                    &mut session.stops.mine
                } else {
                    &mut session.stops.theirs
                };
                if !set.remove(&step) {
                    set.insert(step);
                }
            }
            Action::AutoPay(on) => session.stops.auto_pay = on,
            Action::HoldPriority(on) => session.stops.hold_priority = on,
            Action::YieldTurn => session.yield_the_turn(),
            Action::Pace(ms) => {
                app.pace_ms = ms;
                app.resume_at = None;
            }
            Action::ListOptions(on) => app.list_options = on,
            Action::Declare => {
                if let Some(plan) = app.plan.take() {
                    match session.declare(&plan.choices) {
                        Err(e) => app.status = format!("That isn't a legal answer: {e}"),
                        // Asked again straight away: the engine sent the blocks back.
                        Ok(())
                            if session
                                .human_decision()
                                .is_some_and(|d| d.kind == plan.kind && !plan.attacking()) =>
                        {
                            app.status = "Those blocks aren't legal: a creature with menace \
                                          can't be blocked by just one creature. Declare \
                                          blockers again."
                                .into();
                        }
                        Ok(()) => app.status.clear(),
                    }
                }
            }
            Action::ReplayStep(n) => {
                session.pump(n);
            }
            Action::ReplayPlay(on) => app.replay_playing = on,
        }
        ui.ctx().request_repaint();
    }
    false
}

/// The window for looking through a graveyard or a player's exiled cards. The cards can be
/// pointed at (to enlarge them) and, when one is a choice in the current decision, clicked.
fn pile_window(
    ctx: &egui::Context,
    art: &mut Art,
    cx: &Ctx<'_>,
    open: &mut Option<(PlayerId, Pile)>,
    place: Option<(egui::Pos2, egui::Align2)>,
    ptr: &mut Pointer,
) {
    let Some((p, pile)) = *open else {
        return;
    };
    let v = cx.view;
    // Most recent first: the top of a graveyard is its last card.
    let cards: Vec<&ObjectView> = match pile {
        Pile::Graveyard => v.players[p.index()].graveyard.iter().rev().collect(),
        Pile::Exile => v.exile.iter().filter(|o| o.owner == p).rev().collect(),
    };
    let who = if p == v.viewer {
        "Your".to_string()
    } else {
        format!("P{}'s", p.0)
    };
    let title = match pile {
        Pile::Graveyard => format!("{who} graveyard — most recent first"),
        Pile::Exile => format!("{who} exiled cards — most recent first"),
    };
    let mut keep = true;
    let screen = ctx.content_rect();
    let mut window = egui::Window::new(title)
        .id(egui::Id::unique("pile viewer"))
        .open(&mut keep)
        .collapsible(false)
        .resizable(true);
    if let Some((pos, pivot)) = place {
        window = window.pivot(pivot).current_pos(pos);
    }
    window
        .default_size(vec2(
            (screen.width() * 0.5).clamp(420.0, 1100.0),
            (screen.height() * 0.45).clamp(280.0, 620.0),
        ))
        .show(ctx, |ui| {
            if cards.is_empty() {
                ui.weak("No cards.");
            }
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(CARD_GAP, CARD_GAP);
                        for o in cards {
                            card(ui, art, cx, &[o], Place::Pile, 240.0, 0.0, ptr);
                        }
                    });
                });
        });
    if !keep || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        *open = None;
    }
}

/// The point on the edge of `rect` nearest to `toward` along the line from its centre.
fn edge_point(rect: egui::Rect, toward: egui::Pos2) -> egui::Pos2 {
    let c = rect.center();
    let d = toward - c;
    let sx = if d.x.abs() > f32::EPSILON {
        rect.width() / 2.0 / d.x.abs()
    } else {
        f32::INFINITY
    };
    let sy = if d.y.abs() > f32::EPSILON {
        rect.height() / 2.0 / d.y.abs()
    } else {
        f32::INFINITY
    };
    let s = sx.min(sy);
    if s.is_finite() { c + d * s.min(1.0) } else { c }
}

/// Arrows over the table: who attacks whom (when there is more than one player to attack),
/// who blocks what, and the creature being dragged. Declared combat is drawn softer than
/// what is still being planned.
fn combat_arrows(ctx: &egui::Context, view: &PlayerView, plan: Option<&Plan>, ptr: &Pointer) {
    // On the table's own layer (drawn after it, so on top of the cards), which keeps the
    // arrows underneath windows and the enlarged card.
    let painter = ctx.layer_painter(egui::LayerId::background());
    let between = |from: egui::Rect, to: egui::Rect, color: Color32, width: f32| {
        let a = edge_point(from, to.center());
        let b = edge_point(to, from.center());
        cardview::paint_arrow(&painter, a, b, color, width);
    };
    let several_to_attack = view.players.iter().filter(|p| !p.has_lost).count() > 2;
    let soft = |c: Color32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 190);
    if let Some(combat) = &view.combat {
        for a in &combat.attackers {
            let Some(&attacker) = ptr.cards.get(&a.attacker) else {
                continue;
            };
            if several_to_attack && let Some(&header) = ptr.headers.get(&a.defender) {
                between(attacker, header, soft(RED), 3.5);
            }
            for b in &a.blockers {
                if let Some(&blocker) = ptr.cards.get(b) {
                    between(blocker, attacker, soft(BLUE), 3.5);
                }
            }
        }
    }
    if let Some(plan) = plan {
        for (creature, choice) in &plan.choices {
            let Some(&from) = ptr.cards.get(creature) else {
                continue;
            };
            match choice {
                OptionDesc::Attack { defender, .. } if plan.defenders().len() > 1 => {
                    if let Some(&header) = ptr.headers.get(defender) {
                        between(from, header, RED, 4.5);
                    }
                }
                OptionDesc::Block { attacker, .. } => {
                    if let Some(&to) = ptr.cards.get(attacker) {
                        between(from, to, BLUE, 4.5);
                    }
                }
                _ => {}
            }
        }
    }
    if let Some((creature, pos)) = ptr.dragging
        && let Some(&from) = ptr.cards.get(&creature)
        && !from.contains(pos)
    {
        cardview::paint_arrow(&painter, edge_point(from, pos), pos, Color32::WHITE, 4.5);
    }
}

/// Draw the whole app.
pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    app.art.poll(ui.ctx());
    if app.art.enabled() && app.art.online {
        // Printed data (image addresses) for the precon pool and the basic lands; a no-op
        // once it has been asked for.
        app.art.want_data(ui.ctx(), harness::scryfall_card_names);
    }
    match app.screen {
        Screen::Menu => menu_ui(app, ui),
        Screen::Game => game_ui(app, ui),
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui_: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui(self, ui_);
    }
}

/// A rule about a player, as the player panel lists it.
fn player_rule_text(r: &card_model::PlayerRule) -> String {
    use card_model::PlayerRule as R;
    match r {
        R::NoMaximumHandSize => "No maximum hand size".into(),
        R::MaximumHandSize(n) => format!("Maximum hand size {n}"),
        R::MaximumHandSizeBy(n) => format!("Maximum hand size {n:+}"),
        R::AdditionalLandPlays(n) => format!("{n} additional land play(s) each turn"),
        R::AnyNumberOfLandPlays => "Any number of land plays".into(),
        R::Hexproof => "Hexproof".into(),
        R::Shroud => "Shroud".into(),
        R::TopCardRevealed => "Plays with the top card of their library revealed".into(),
        R::LookAtTopCard => "May look at the top card of their library".into(),
        R::PlayFrom { zone, .. } => format!(
            "May play cards from {}",
            match zone {
                card_model::PlayZone::Graveyard => "their graveyard",
                card_model::PlayZone::LibraryTop => "the top of their library",
                card_model::PlayZone::Exile => "exile",
            }
        ),
        R::PlayThisFrom(_) => "May cast a card from another zone".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Rect, pos2};

    #[test]
    fn power_and_toughness_are_badged_only_when_they_differ_from_the_card() {
        let printed = (Some(2), Some(3));
        assert_eq!(pt_badge(printed, (2, 3)), None, "the card already says 2/3");
        assert_eq!(pt_badge(printed, (3, 4)), Some(GREEN), "+1/+1 counter");
        assert_eq!(pt_badge(printed, (2, 4)), Some(GREEN));
        assert_eq!(pt_badge(printed, (1, 2)), Some(RED), "-1/-1");
        assert_eq!(pt_badge(printed, (2, 1)), Some(RED));
        assert_eq!(
            pt_badge(printed, (3, 2)),
            Some(Color32::WHITE),
            "up and down"
        );
        // Nothing printed to compare with (a land that became a creature).
        assert_eq!(pt_badge((None, None), (1, 1)), Some(Color32::WHITE));
    }

    #[test]
    fn players_sit_clockwise_around_the_table() {
        // (top row, bottom row), as places after the viewer in turn order; 0 = the viewer.
        assert_eq!(table_layout(2), (vec![1], vec![0]));
        assert_eq!(table_layout(3), (vec![1, 2], vec![0]));
        // Four: a 2 x 2 grid — viewer bottom left, then clockwise.
        assert_eq!(table_layout(4), (vec![1, 2], vec![0, 3]));
        assert_eq!(table_layout(5), (vec![1, 2, 3], vec![0, 4]));
        assert_eq!(table_layout(6), (vec![1, 2, 3], vec![0, 5, 4]));
        for n in 2..=8 {
            let (top, bottom) = table_layout(n);
            let mut all: Vec<usize> = top.iter().chain(&bottom).copied().collect();
            all.sort_unstable();
            assert_eq!(
                all,
                (0..n).collect::<Vec<_>>(),
                "{n} players, each seated once"
            );
            assert_eq!(bottom[0], 0, "the viewer is bottom left");
        }
    }

    #[test]
    fn cards_are_as_big_as_lets_the_table_fit() {
        let seat = |creatures: usize, lands: usize| SeatSize {
            width: 1100.0,
            rows: [
                vec![cardview::CARD_ASPECT; creatures],
                vec![cardview::CARD_ASPECT; lands],
            ],
            out: false,
        };
        // An empty table: the biggest cards that fit four rows into the height.
        let empty = vec![vec![seat(0, 0), seat(0, 0)], vec![seat(0, 0), seat(0, 0)]];
        let h = fit_card_height(&empty, 1000.0);
        assert!(table_height(&empty, h) <= 1000.0);
        assert!(
            table_height(&empty, h + 4.0) > 1000.0,
            "not the biggest: {h}"
        );
        assert!((180.0..=230.0).contains(&h), "{h}");
        // Plenty of room: capped.
        assert_eq!(fit_card_height(&empty, 5000.0), CARD_MAX);
        // A crowded seat: cards shrink (or its rows wrap) so that it still fits.
        let crowded = vec![vec![seat(30, 8), seat(2, 3)], vec![seat(4, 4), seat(0, 0)]];
        let c = fit_card_height(&crowded, 1000.0);
        assert!(c < h && c >= CARD_MIN, "{c}");
        assert!(table_height(&crowded, c) <= 1000.0);
        // Impossible: the smallest size, and the table scrolls.
        let absurd = vec![vec![seat(400, 0)], vec![seat(400, 0)]];
        assert_eq!(fit_card_height(&absurd, 600.0), CARD_MIN);
        // A row of seats whose players are all out needs only their lines.
        let gone = SeatSize {
            out: true,
            ..seat(0, 0)
        };
        let half_empty = vec![vec![gone.clone(), gone], vec![seat(0, 0), seat(0, 0)]];
        assert!(fit_card_height(&half_empty, 1000.0) > h + 100.0);
        // Wrapping: 30 cards 60 wide (+ gap) in 1100 are two lines.
        assert_eq!(row_lines(&[0.5; 30], 120.0, 1100.0), 2);
        assert_eq!(row_lines(&[], 120.0, 1100.0), 1);
    }

    #[test]
    fn the_enlarged_card_sits_beside_the_card_and_on_screen() {
        let screen = Rect::from_min_max(pos2(0.0, 0.0), pos2(2560.0, 1440.0));
        let size = vec2(500.0, 720.0);
        let overlaps =
            |near: Rect, (x, y): (f32, f32)| Rect::from_min_size(pos2(x, y), size).intersects(near);
        let inside =
            |(x, y): (f32, f32)| screen.contains_rect(Rect::from_min_size(pos2(x, y), size));
        // A card on the board: to its right.
        let near = Rect::from_min_size(pos2(300.0, 200.0), vec2(200.0, 280.0));
        let at = zoom_position(screen, near, size, false);
        assert!(at.0 >= near.right() && !overlaps(near, at) && inside(at));
        // No room on the right: to its left.
        let near = Rect::from_min_size(pos2(2300.0, 200.0), vec2(200.0, 280.0));
        let at = zoom_position(screen, near, size, false);
        assert!(at.0 + size.x <= near.left() && !overlaps(near, at) && inside(at));
        // Near the top or bottom edge it slides to stay on screen.
        let near = Rect::from_min_size(pos2(300.0, 1200.0), vec2(200.0, 230.0));
        let at = zoom_position(screen, near, size, false);
        assert!(!overlaps(near, at) && inside(at));
        // A card in the hand: above it, so the rest of the hand stays visible.
        let at = zoom_position(screen, near, size, true);
        assert!(at.1 + size.y <= near.top() && !overlaps(near, at) && inside(at));
        // …unless the window is too short for that: then beside it.
        let short = Rect::from_min_max(pos2(0.0, 0.0), pos2(1280.0, 760.0));
        let near = Rect::from_min_size(pos2(100.0, 600.0), vec2(110.0, 150.0));
        let (x, y) = zoom_position(short, near, size, true);
        assert!(x >= near.right());
        assert!(short.contains_rect(Rect::from_min_size(pos2(x, y), size)));
    }

    // An ability a permanent was given by another card (D-064): the view lists it, and
    // its button says what it does — in the words of the card that gives it.
    #[test]
    fn an_ability_given_by_another_card_is_shown_with_its_text() {
        use engine_core::testing::Scenario;
        use engine_core::zones::Zone;
        let gift = "Name:Test Ember Gift\nManaCost:R\nTypes:Enchantment Aura\nK:Enchant:Creature\n\
            S:Mode$ Continuous | AffectedDefined$ Enchanted | Affected$ Creature | AddAbility$ Ping | Description$ Text.\n\
            SVar:Ping:AB$ DealDamage | Cost$ T | ValidTgts$ Any | NumDmg$ 1 | SpellDescription$ Text.\nOracle:Text.";
        let cat = harness::forge_test_catalog(&[gift], &[]);
        let me = PlayerId(0);
        let mut sc = Scenario::new(engine_core::GameConfig::commander(2), cat.clone(), 7);
        for p in [me, PlayerId(1)] {
            sc.library(p, "Forest", 10);
        }
        let golem = sc.card(me, Zone::Battlefield, "Test Golem");
        let aura = sc.card(me, Zone::Battlefield, "Test Ember Gift");
        sc.attach(aura, golem);
        let mut st = sc.build();
        let d = engine_core::start(&mut st).next.expect("a decision");
        let v = engine_core::view::player_view(&st, me);
        let o = find_object(&v, golem).expect("the Golem");
        assert_eq!(o.granted.len(), 1);
        let option = d
            .options
            .iter()
            .find(|o| matches!(o, OptionDesc::ActivateAbility { source, .. } if *source == golem))
            .expect("the Golem's ability is offered");
        assert_eq!(
            option_label(&v, &cat, &d, option),
            "Test Golem — {T}: This deals 1 damage to any target."
        );
    }

    // A card with two faces (D-071): each option names the face it is about (CR 712.8b,
    // 712.9), and the card is drawn with the face that is up.
    #[test]
    fn options_name_the_face_they_are_about() {
        use engine_core::testing::Scenario;
        use engine_core::zones::Zone;
        let tide = "Name:Test Tide Charm\nManaCost:1 U\nTypes:Instant\n\
            A:SP$ Draw | NumCards$ 1 | SpellDescription$ Draw a card.\nAlternateMode:Modal\n\
            Oracle:Draw a card.\nALTERNATE\nName:Test Tide Flat\nManaCost:no cost\nTypes:Land\n\
            A:AB$ Mana | Cost$ T | Produced$ U | SpellDescription$ Add {U}.\nOracle:{T}: Add {U}.";
        let cat = harness::forge_test_catalog(&[tide], &[]);
        let me = PlayerId(0);
        let mut sc = Scenario::new(engine_core::GameConfig::commander(2), cat.clone(), 7);
        for p in [me, PlayerId(1)] {
            sc.library(p, "Forest", 10);
        }
        sc.cards(me, Zone::Battlefield, &["Island", "Island"]);
        let card = sc.card(me, Zone::Hand, "Test Tide Charm");
        let mut st = sc.build();
        let d = engine_core::start(&mut st).next.expect("a decision");
        let v = engine_core::view::player_view(&st, me);
        let labels: Vec<String> = d
            .options
            .iter()
            .filter(|o| anchor(o) == Some(Anchor::Obj(card)))
            .map(|o| option_label(&v, &cat, &d, o))
            .collect();
        assert_eq!(labels, ["play Test Tide Flat", "cast Test Tide Charm"]);
        let o = find_object(&v, card).unwrap();
        assert_eq!(cat.get(cat.face(o.card, o.face)).name, "Test Tide Charm");
    }
}

//! The egui view (spec §11): a pure function of the session's `PlayerView`, the pending
//! decision and a little widget state. Every decision can be answered with the mouse alone:
//! each option is a button in the decision panel, and options that belong to a card or
//! player can also be chosen by clicking that card or player on the board.

use crate::art::Art;
use crate::cardview::{self, Badge, CardLook, Corner};
use crate::session::Session;
use card_model::describe;
use card_model::{AbilityDef, CardCatalog, CardDefId, CardType, CounterKind};
use eframe::egui::{self, Color32, RichText, Stroke, vec2};
use engine_core::decision::{ChoiceKind, OptionDesc, PendingDecision, Primitive, Response};
use engine_core::stack::Target;
use engine_core::turn::Step;
use engine_core::view::{ObjectView, PlayerView};
use engine_core::{ObjectId, PlayerId};
use harness::{Agents, Replay, Setup};
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
    /// The card shown enlarged: the last one the pointer was over.
    pub preview: Option<Preview>,
}

/// The card shown enlarged in the side panel.
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
    ToggleStop { mine: bool, step: Step },
    AutoPay(bool),
    Pace(u64),
    ReplayStep(usize),
    ReplayPlay(bool),
}

impl App {
    pub fn new(saves_dir: PathBuf, art: Art, pace_ms: u64) -> Self {
        let mut app = App {
            screen: Screen::Menu,
            session: None,
            setup_index: 0,
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
            preview: None,
        };
        app.refresh_saves();
        app
    }

    /// An app that keeps its saved games in `dir`, never touches the network or the image
    /// cache, and doesn't pause for AI actions (tests).
    pub fn with_saves_dir(dir: PathBuf) -> Self {
        App::new(dir, Art::disabled(), 0)
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
        if !matches!(setup, Setup::Real { .. }) {
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
        let setup = SETUPS[self.setup_index].1;
        if !self.cards_ready(setup) {
            return;
        }
        self.session = Some(Session::new(setup, self.seed, Some(HUMAN), self.opponents));
        self.screen = Screen::Game;
        self.status.clear();
        self.decision_id = u64::MAX;
        self.preview = None;
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
        ChoiceKind::ScryOrder => "Order the cards",
        ChoiceKind::ChooseColor => "Choose a color",
        ChoiceKind::ChoosePlayer => "Choose a player",
    }
}

/// The card or player an option belongs to. For attack/block decisions the option is
/// anchored to what is being attacked/blocked (the creature asked about is the subject).
pub fn anchor(o: &OptionDesc) -> Option<Anchor> {
    Some(match o {
        OptionDesc::PlayLand { card }
        | OptionDesc::CastSpell { card }
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
        OptionDesc::Pass
        | OptionDesc::Keep
        | OptionDesc::Mulligan
        | OptionDesc::Mode { .. }
        | OptionDesc::Mana { .. }
        | OptionDesc::Shield { .. }
        | OptionDesc::Accept
        | OptionDesc::Decline => return None,
    })
}

/// The creature a per-creature decision is about (the attacker or blocker being asked).
fn subject(d: &PendingDecision) -> Option<ObjectId> {
    d.options.iter().find_map(|o| match o {
        OptionDesc::Attack { attacker, .. } => Some(*attacker),
        OptionDesc::Block { blocker, .. } => Some(*blocker),
        _ => None,
    })
}

fn find_object(v: &PlayerView, id: ObjectId) -> Option<&ObjectView> {
    v.hand
        .iter()
        .chain(&v.battlefield)
        .chain(&v.command)
        .chain(&v.exile)
        .chain(v.players.iter().flat_map(|p| &p.graveyard))
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
                let def = cat.get(top.card);
                let body = match top.ability {
                    None => def.spell_ability(),
                    Some(i) => match &def.abilities[i as usize] {
                        AbilityDef::Activated { body, .. } | AbilityDef::Triggered { body, .. } => {
                            Some(body)
                        }
                        _ => None,
                    },
                };
                body.and_then(|b| b.modes().get(*index as usize))
                    .map(describe::mode_text)
            });
            text.unwrap_or_else(|| format!("mode {index}"))
        }
        OptionDesc::Attack { defender, .. } => format!("attack P{}", defender.0),
        OptionDesc::Accept => "Yes".into(),
        OptionDesc::Decline => "No".into(),
        OptionDesc::Pass if d.kind == ChoiceKind::Priority => "Pass priority".into(),
        OptionDesc::TapForMana { source, mana } => match find_object(v, *source) {
            Some(src) => format!("tap {} for {}", src.name, mana.symbol()),
            None => harness::text::option_line(v, o),
        },
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
    subject: Option<ObjectId>,
    focus: Option<Anchor>,
    /// Options chosen so far in a multi-select decision.
    picked: &'a [usize],
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
    hovered: Option<Preview>,
}

/// Where a card is being drawn.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Hand,
    Battlefield,
    Command,
}

fn counter_name(kind: CounterKind) -> &'static str {
    match kind {
        CounterKind::PlusOne => "+1/+1",
        CounterKind::MinusOne => "-1/-1",
    }
}

/// A card's accessible name: everything the picture and its badges show, as text.
fn card_text(o: &ObjectView, cat: &CardCatalog) -> String {
    let def = cat.get(o.card);
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
    let attacking = combat.and_then(|c| c.attackers.iter().find(|a| a.attacker == o.id));
    let blocking = combat.is_some_and(|c| c.attackers.iter().any(|a| a.blockers.contains(&o.id)));
    if let Some(a) = attacking {
        out.push(Badge::new(
            Corner::TopRight,
            format!("ATK P{}", a.defender.0),
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
        let def = cx.cat.get(o.card);
        let fg = match (def.power, def.toughness) {
            (Some(pp), Some(pt)) if p > pp || t > pt => GREEN,
            (Some(pp), Some(pt)) if p < pp || t < pt => RED,
            _ => Color32::WHITE,
        };
        out.push(Badge::new(
            Corner::BottomRight,
            format!("{p}/{t}"),
            DARK,
            fg,
        ));
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
fn card(
    ui: &mut egui::Ui,
    art: &mut Art,
    cx: &Ctx<'_>,
    group: &[&ObjectView],
    place: Place,
    height: f32,
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
    let is_subject = group.iter().any(|m| cx.subject == Some(m.id));
    let outline = if focused || chosen {
        Some(Stroke::new(3.0, Color32::WHITE))
    } else if !opts.is_empty() {
        Some(Stroke::new(2.5, GOLD))
    } else if is_subject {
        Some(Stroke::new(2.5, BLUE))
    } else {
        None
    };
    let mut label = card_text(o, cx.cat);
    if group.len() > 1 {
        label.push_str(&format!("\nx{}", group.len()));
    }
    let look = CardLook {
        def: cx.cat.get(o.card),
        label: &label,
        height,
        tapped: o.tapped && place == Place::Battlefield,
        pile: group.len() as u32,
        outline,
        badges: &badges(cx, o, place),
    };
    let resp = cardview::card_widget(ui, art, &look);
    if resp.hovered() {
        ptr.hovered = Some(Preview {
            card: o.card,
            object: Some(o.id),
        });
    }
    if resp.clicked() && !opts.is_empty() {
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
            && cx.subject != Some(o.id)
    };
    let key = |o: &ObjectView| {
        (
            o.card,
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

/// A row of cards that wraps, centred vertically so sideways cards line up.
fn card_row(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let layout = egui::Layout::left_to_right(egui::Align::Center).with_main_wrap(true);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), height), layout, |ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
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
    compact: bool,
    ptr: &mut Pointer,
) {
    let v = cx.view;
    let pl = &v.players[p.index()];
    let me = p == v.viewer;
    let mut head = format!(
        "P{}{} — {} life — hand {} — library {} — graveyard {}",
        p.0,
        if me { " (you)" } else { "" },
        pl.life,
        pl.hand_size,
        pl.library_size,
        pl.graveyard.len()
    );
    if pl.has_lost {
        head.push_str(" — OUT");
    }
    let opts = cx.options_at(Anchor::Player(p));
    let active = v.active == p;
    ui.horizontal(|ui| {
        let text = RichText::new(head).strong().size(14.0).color(if active {
            GOLD
        } else {
            Color32::from_gray(225)
        });
        let resp = if opts.is_empty() {
            ui.label(text)
        } else {
            let stroke = if cx.focus == Some(Anchor::Player(p)) {
                Stroke::new(3.0, Color32::WHITE)
            } else {
                Stroke::new(2.0, GOLD)
            };
            let r = ui.add(egui::Button::new(text).stroke(stroke));
            if r.clicked() {
                ptr.clicked = Some(opts);
            }
            r
        };
        resp.on_hover_text(if pl.graveyard.is_empty() {
            "Graveyard: empty".to_string()
        } else {
            let names: Vec<&str> = pl.graveyard.iter().map(|o| o.name.as_str()).collect();
            format!("Graveyard (top last): {}", names.join(", "))
        });
        if pl.mana_pool.total() > 0 {
            let mut pool = String::from("mana ");
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
            cardview::rich_label(ui, art, pool.trim_end(), 15.0, Color32::from_gray(225));
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
    let lands = piles(cx, &lands);
    let draw_others = |ui: &mut egui::Ui, art: &mut Art, ptr: &mut Pointer| {
        for o in &command {
            card(ui, art, cx, &[*o], Place::Command, height, ptr);
        }
        if !command.is_empty() && !ordered.is_empty() {
            ui.separator();
        }
        for o in &ordered {
            card(ui, art, cx, &[*o], Place::Battlefield, height, ptr);
        }
    };
    let draw_lands = |ui: &mut egui::Ui, art: &mut Art, ptr: &mut Pointer| {
        for g in &lands {
            card(ui, art, cx, g, Place::Battlefield, height, ptr);
        }
    };
    if compact {
        // An opponent: everything in one row, lands last.
        card_row(ui, height, |ui| {
            draw_others(ui, art, ptr);
            if !lands.is_empty() && (!ordered.is_empty() || !command.is_empty()) {
                ui.separator();
            }
            draw_lands(ui, art, ptr);
        });
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
    art: &Art,
    playing: bool,
    pace_ms: u64,
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
        ui.label(
            RichText::new(format!("Turn {} — {whose}", t.turn_number))
                .strong()
                .size(17.0)
                .color(color),
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
            ui.separator();
            ui.label("Opponents act:");
            for (name, ms) in SPEEDS {
                if ui.selectable_label(pace_ms == *ms, *name).clicked() {
                    actions.push(Action::Pace(*ms));
                }
            }
        }
        if art.pending > 0 {
            ui.separator();
            ui.weak(format!("loading {} image(s)…", art.pending));
        } else if let Some(e) = &art.last_error {
            ui.separator();
            ui.weak("some images are unavailable").on_hover_text(e);
        }
    });
    if s.replay_queue.is_none() {
        ui.collapsing("Stops (when you get priority with an empty stack)", |ui| {
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

/// The enlarged card, with what the game currently says about it.
fn preview_panel(ui: &mut egui::Ui, art: &mut Art, cx: &Ctx<'_>, preview: Option<Preview>) {
    let width = ui.available_width().min(300.0);
    let Some(pv) = preview else {
        let size = vec2(width, (width / cardview::CARD_ASPECT).round());
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, width * 0.048, Color32::from_gray(36));
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Point at a card to see it here",
            egui::FontId::proportional(13.0),
            Color32::from_gray(140),
        );
        ui.small(" ");
        ui.small(" ");
        return;
    };
    let def = cx.cat.get(pv.card);
    cardview::card_large(ui, art, def, width);
    let credit = if def.token {
        None
    } else {
        art.credit(&def.name)
    };
    ui.add(
        egui::Label::new(
            RichText::new(credit.unwrap_or_else(|| " ".into()))
                .small()
                .weak(),
        )
        .truncate(),
    );
    // What the picture can't show: the object's current state.
    let mut now: Vec<String> = Vec::new();
    if let Some(o) = pv.object.and_then(|id| find_object(cx.view, id)) {
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
    }
    let now = if now.is_empty() {
        " ".to_string()
    } else {
        format!("Now: {}", now.join(" · "))
    };
    ui.add(egui::Label::new(RichText::new(now).small()).truncate());
}

fn side_panel(
    ui: &mut egui::Ui,
    art: &mut Art,
    cx: &Ctx<'_>,
    s: &Session,
    preview: Option<Preview>,
    ptr: &mut Pointer,
) {
    let v = cx.view;
    preview_panel(ui, art, cx, preview);
    ui.separator();
    ui.strong("Stack (top first)");
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
                def: cx.cat.get(so.card),
                label: &format!("on the stack: {line}"),
                height: 64.0,
                tapped: false,
                pile: 1,
                outline: (!opts.is_empty()).then_some(Stroke::new(2.5, GOLD)),
                badges: &[],
            };
            let resp = cardview::card_widget(ui, art, &look);
            if resp.hovered() {
                ptr.hovered = Some(Preview {
                    card: so.card,
                    object: None,
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
        });
}

/// The decision panel: prompt plus a button for every option, by primitive.
fn decision_panel(
    app: &mut App,
    art: &mut Art,
    ui: &mut egui::Ui,
    cx: &Ctx<'_>,
    actions: &mut Vec<Action>,
) {
    let Some(d) = cx.decision else {
        return;
    };
    let mut title = prompt(d.kind).to_string();
    if let Some(o) = cx.subject.and_then(|id| find_object(cx.view, id)) {
        title.push_str(&format!(" — {}", o.name));
    }
    if let (
        ChoiceKind::ChooseTargets { .. }
        | ChoiceKind::ChooseModes
        | ChoiceKind::ChooseX
        | ChoiceKind::PayMana,
        Some(top),
    ) = (d.kind, cx.view.stack.last())
    {
        title.push_str(&format!(" for {}", top.name));
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
                None => (0..labels.len()).collect(),
            };
            ui.horizontal_wrapped(|ui| {
                for i in shown {
                    if cardview::rich_button(ui, art, &labels[i], false).clicked() {
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
                if d.kind == ChoiceKind::Priority && ui.button("Pass until end of turn").clicked() {
                    actions.push(Action::PassTurn);
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
            ui.horizontal_wrapped(|ui| {
                for (i, label) in labels.iter().enumerate() {
                    let on = app.picked.contains(&i);
                    if cardview::rich_button(ui, art, label, on).clicked() {
                        if on {
                            app.picked.retain(|x| *x != i);
                        } else {
                            app.picked.push(i);
                        }
                    }
                }
            });
            let ok = (*min..=*max).contains(&app.picked.len());
            if ui.add_enabled(ok, egui::Button::new("Confirm")).clicked() {
                let mut p = app.picked.clone();
                p.sort_unstable();
                actions.push(Action::Respond(Response::Pick(p)));
            }
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
    let cat = session.st.catalog.clone();
    let picked = app.picked.clone();
    let cx = Ctx {
        view: &view,
        cat: &cat,
        decision: decision.as_ref(),
        subject: decision.as_ref().and_then(subject),
        focus: app.focus,
        picked: &picked,
    };
    let mut actions: Vec<Action> = Vec::new();
    let mut ptr = Pointer::default();
    let window = ui.max_rect().size();

    egui::Panel::top("top").show(ui, |ui| {
        top_bar(
            ui,
            session,
            art,
            app.replay_playing,
            app.pace_ms,
            &mut actions,
        );
    });
    egui::Panel::right("side")
        .default_size(316.0)
        .show(ui, |ui| {
            side_panel(ui, art, &cx, session, app.preview, &mut ptr);
        });
    egui::Panel::bottom("bottom").show(ui, |ui| {
        if let Some(result) = &session.st.result {
            let text = match result.winner {
                Some(w) if Some(w) == session.human => "Game over — you win!".to_string(),
                Some(w) => format!("Game over — P{} wins", w.0),
                None => "Game over — it's a draw".to_string(),
            };
            ui.strong(RichText::new(text).size(18.0).color(GOLD));
        } else if decision.is_some() {
            decision_panel(app, art, ui, &cx, &mut actions);
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
        ui.separator();
        ui.small("Your hand");
        // The hand fans out: cards overlap when there are too many to lay side by side.
        // An empty hand takes no room, so the board gets it.
        let n = view.hand.len();
        let hand_h = if n == 0 {
            0.0
        } else {
            (window.y * 0.19).clamp(120.0, 230.0)
        };
        let w = cardview::card_size(hand_h).x;
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
                card(ui, art, &cx, &[o], Place::Hand, hand_h, &mut ptr);
            });
        }
        ui.add_space(4.0);
    });
    egui::CentralPanel::default().show(ui, |ui| {
        let n = view.players.len();
        // Card height: opponents get one row each, the viewer two; fill the panel.
        // Players who have left the game keep only their header line.
        let opponents = view
            .players
            .iter()
            .enumerate()
            .filter(|(i, pl)| *i != view.viewer.index() && !pl.has_lost)
            .count();
        let chrome = n as f32 * 48.0;
        let rows = opponents as f32 * OPPONENT_SCALE + 2.0;
        let height = ((ui.available_height() - chrome) / rows).clamp(84.0, 210.0);
        egui::ScrollArea::vertical()
            .id_salt("board")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Opponents in turn order after the viewer, then the viewer at the bottom.
                for k in 1..=n {
                    let p = PlayerId(((view.viewer.index() + k) % n) as u8);
                    let me = k == n;
                    let mut frame = egui::Frame::group(ui.style());
                    if view.active == p {
                        frame = frame.stroke(Stroke::new(1.5, GOLD));
                    }
                    frame.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        let h = if me { height } else { height * OPPONENT_SCALE };
                        player_area(ui, art, &cx, p, h.round(), !me, &mut ptr);
                    });
                }
            });
    });

    if ptr.hovered.is_some() {
        app.preview = ptr.hovered;
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
            Action::Pace(ms) => {
                app.pace_ms = ms;
                app.resume_at = None;
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

/// Opponents' cards are drawn a little smaller than the viewer's.
const OPPONENT_SCALE: f32 = 0.8;

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

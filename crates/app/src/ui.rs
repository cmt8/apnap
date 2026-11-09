//! The egui view (spec §11): a pure function of the session's `PlayerView`, the pending
//! decision and a little widget state. Every decision can be answered with the mouse alone:
//! each option is a button in the decision panel, and options that belong to a card or
//! player can also be chosen by clicking that card or player on the board.

use crate::session::Session;
use card_model::describe;
use card_model::{AbilityDef, CardCatalog, CardType};
use eframe::egui::{self, Color32, RichText, Stroke};
use engine_core::decision::{ChoiceKind, OptionDesc, PendingDecision, Primitive, Response};
use engine_core::stack::Target;
use engine_core::turn::Step;
use engine_core::view::{ObjectView, PlayerView};
use engine_core::{ObjectId, PlayerId};
use harness::{Agents, Replay, Setup};
use std::path::PathBuf;

const HUMAN: PlayerId = PlayerId(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Menu,
    Game,
}

/// The setups offered in the menu.
pub const SETUPS: &[(&str, Setup)] = &[
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
}

impl Default for App {
    fn default() -> Self {
        let mut app = App {
            screen: Screen::Menu,
            session: None,
            setup_index: 0,
            seed: 1,
            opponents: Agents::Heuristic,
            status: String::new(),
            saves: Vec::new(),
            saves_dir: saves_dir(),
            replay_playing: false,
            decision_id: u64::MAX,
            picked: Vec::new(),
            number: 0,
            alloc: Vec::new(),
            order: Vec::new(),
            focus: None,
        };
        app.refresh_saves();
        app
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
    ReplayStep(usize),
    ReplayPlay(bool),
}

impl App {
    /// An app that keeps its saved games in `dir` (tests; portable installs).
    pub fn with_saves_dir(dir: PathBuf) -> Self {
        let mut app = App {
            saves_dir: dir,
            ..App::default()
        };
        app.refresh_saves();
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

    pub fn start_game(&mut self) {
        let setup = SETUPS[self.setup_index].1;
        self.session = Some(Session::new(setup, self.seed, Some(HUMAN), self.opponents));
        self.screen = Screen::Game;
        self.status.clear();
        self.decision_id = u64::MAX;
    }

    /// Write the current game to the saves directory. Returns the file written.
    pub fn save_game(&mut self) -> Option<PathBuf> {
        let s = self.session.as_ref()?;
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
        if let Some(replay) = self.read_save(path) {
            self.session = Some(Session::load(&replay, Some(HUMAN), self.opponents));
            self.screen = Screen::Game;
            self.decision_id = u64::MAX;
        }
    }

    /// Watch a saved game from the beginning.
    pub fn watch_replay(&mut self, path: &PathBuf) {
        if let Some(replay) = self.read_save(path) {
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

/// Everything the board needs to know about the decision being shown.
struct Ctx<'a> {
    view: &'a PlayerView,
    cat: &'a CardCatalog,
    decision: Option<&'a PendingDecision>,
    subject: Option<ObjectId>,
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
        let name = match kind {
            card_model::CounterKind::PlusOne => "+1/+1",
            card_model::CounterKind::MinusOne => "-1/-1",
        };
        flags.push(format!(
            "{n} {name} counter{}",
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

fn card_hover(o: &ObjectView, cat: &CardCatalog) -> String {
    let def = cat.get(o.card);
    let mut lines = vec![format!("{} — {}", o.name, describe::type_line(def))];
    lines.extend(describe::rules_text(def));
    if o.owner != o.controller {
        lines.push(format!(
            "Owned by P{}, controlled by P{}",
            o.owner.0, o.controller.0
        ));
    }
    lines.join("\n")
}

/// Draw one card. Returns the options (indices) chosen by clicking it, if any.
fn card(
    ui: &mut egui::Ui,
    cx: &Ctx<'_>,
    o: &ObjectView,
    focus: Option<Anchor>,
) -> Option<Vec<usize>> {
    let opts = cx.options_at(Anchor::Obj(o.id));
    let is_subject = cx.subject == Some(o.id);
    let fill = if o.tapped {
        Color32::from_rgb(52, 52, 58)
    } else if o.card_types.contains(&CardType::Land) {
        Color32::from_rgb(58, 70, 58)
    } else {
        Color32::from_rgb(64, 66, 82)
    };
    let stroke = if focus == Some(Anchor::Obj(o.id)) {
        Stroke::new(3.0, Color32::WHITE)
    } else if !opts.is_empty() {
        Stroke::new(2.0, GOLD)
    } else if is_subject {
        Stroke::new(2.0, BLUE)
    } else {
        Stroke::new(1.0, Color32::from_gray(90))
    };
    let text = RichText::new(card_text(o, cx.cat))
        .size(11.5)
        .color(Color32::from_gray(230));
    let resp = ui
        .add(
            egui::Button::new(text)
                .min_size(egui::vec2(132.0, 54.0))
                .fill(fill)
                .stroke(stroke),
        )
        .on_hover_text(card_hover(o, cx.cat));
    (resp.clicked() && !opts.is_empty()).then_some(opts)
}

fn cards_row(
    ui: &mut egui::Ui,
    cx: &Ctx<'_>,
    cards: &[&ObjectView],
    focus: Option<Anchor>,
    clicked: &mut Option<Vec<usize>>,
) {
    ui.horizontal_wrapped(|ui| {
        for o in cards {
            if let Some(opts) = card(ui, cx, o, focus) {
                *clicked = Some(opts);
            }
        }
    });
}

fn player_area(
    ui: &mut egui::Ui,
    cx: &Ctx<'_>,
    p: PlayerId,
    focus: Option<Anchor>,
    clicked: &mut Option<Vec<usize>>,
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
    if pl.mana_pool.total() > 0 {
        let pool: Vec<String> = engine_core::mana::ManaType::ALL
            .iter()
            .filter(|t| pl.mana_pool.amounts[t.index()] > 0)
            .map(|t| format!("{}×{}", pl.mana_pool.amounts[t.index()], t.symbol()))
            .collect();
        head.push_str(&format!(" — mana {}", pool.join(" ")));
    }
    // Commander damage this player has taken (CR 903.10a).
    for (card, n) in &pl.commander_damage {
        let name = v
            .commanders
            .iter()
            .find(|c| c.card_instance == *card)
            .map_or("a commander".to_string(), |c| c.name.clone());
        head.push_str(&format!(" — {n}/21 from {name}"));
    }
    let opts = cx.options_at(Anchor::Player(p));
    let active = v.active == p;
    ui.horizontal(|ui| {
        let text = RichText::new(head).strong().color(if active {
            GOLD
        } else {
            Color32::from_gray(220)
        });
        if opts.is_empty() {
            ui.label(text);
        } else {
            let stroke = if focus == Some(Anchor::Player(p)) {
                Stroke::new(3.0, Color32::WHITE)
            } else {
                Stroke::new(2.0, GOLD)
            };
            if ui.add(egui::Button::new(text).stroke(stroke)).clicked() {
                *clicked = Some(opts);
            }
        }
    })
    .response
    .on_hover_text(if pl.graveyard.is_empty() {
        "Graveyard: empty".to_string()
    } else {
        let names: Vec<&str> = pl.graveyard.iter().map(|o| o.name.as_str()).collect();
        format!("Graveyard: {}", names.join(", "))
    });
    let mine = |o: &&ObjectView| o.controller == p;
    let command: Vec<&ObjectView> = v.command.iter().filter(|o| o.owner == p).collect();
    let lands: Vec<&ObjectView> = v
        .battlefield
        .iter()
        .filter(mine)
        .filter(|o| {
            o.card_types.contains(&CardType::Land) && !o.card_types.contains(&CardType::Creature)
        })
        .collect();
    let others: Vec<&ObjectView> = v
        .battlefield
        .iter()
        .filter(mine)
        .filter(|o| {
            !o.card_types.contains(&CardType::Land) || o.card_types.contains(&CardType::Creature)
        })
        .collect();
    if !command.is_empty() {
        ui.horizontal(|ui| {
            ui.small("Command zone:");
            cards_row(ui, cx, &command, focus, clicked);
        });
    }
    cards_row(ui, cx, &others, focus, clicked);
    cards_row(ui, cx, &lands, focus, clicked);
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
             of the Coast. ©Wizards of the Coast LLC.",
        );
    });
}

fn top_bar(ui: &mut egui::Ui, s: &Session, playing: bool, actions: &mut Vec<Action>) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("Menu").clicked() {
            actions.push(Action::ToMenu);
        }
        if ui.button("Save").clicked() {
            actions.push(Action::Save);
        }
        ui.separator();
        let t = &s.st.turn;
        ui.strong(format!("Turn {} — P{}'s turn", t.turn_number, t.active.0));
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

fn side_panel(ui: &mut egui::Ui, s: &Session, v: &PlayerView) {
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
        ui.label(line);
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
fn decision_panel(app: &mut App, ui: &mut egui::Ui, cx: &Ctx<'_>, actions: &mut Vec<Action>) {
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
                    if ui.button(&labels[i]).clicked() {
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
                    if ui.add(egui::Button::new(label).selected(on)).clicked() {
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

fn game_ui(app: &mut App, ui: &mut egui::Ui) {
    let Some(mut session) = app.session.take() else {
        app.screen = Screen::Menu;
        return;
    };
    // Let the AI seats (and automatic passes) run.
    if session.replay_queue.is_some() {
        if app.replay_playing && session.pump(3) > 0 {
            ui.ctx().request_repaint();
        }
    } else {
        session.pump(5_000);
        if !session.is_over() && session.human_decision().is_none() {
            ui.ctx().request_repaint();
        }
    }
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
    let cx = Ctx {
        view: &view,
        cat: &cat,
        decision: decision.as_ref(),
        subject: decision.as_ref().and_then(subject),
    };
    let mut actions: Vec<Action> = Vec::new();
    let mut clicked: Option<Vec<usize>> = None;

    egui::Panel::top("top").show(ui, |ui| {
        top_bar(ui, &session, app.replay_playing, &mut actions)
    });
    egui::Panel::right("side")
        .default_size(300.0)
        .show(ui, |ui| side_panel(ui, &session, &view));
    egui::Panel::bottom("bottom").show(ui, |ui| {
        if let Some(result) = &session.st.result {
            let text = match result.winner {
                Some(w) if Some(w) == session.human => "Game over — you win!".to_string(),
                Some(w) => format!("Game over — P{} wins", w.0),
                None => "Game over — it's a draw".to_string(),
            };
            ui.strong(RichText::new(text).size(18.0).color(GOLD));
        } else if decision.is_some() {
            decision_panel(app, ui, &cx, &mut actions);
        } else if session.replay_queue.is_some() {
            ui.weak("Replay: use Step or Play above.");
        } else {
            ui.weak("Opponents are thinking…");
        }
        if !app.status.is_empty() {
            ui.small(&app.status);
        }
        ui.separator();
        ui.small("Your hand");
        let hand: Vec<&ObjectView> = view.hand.iter().collect();
        cards_row(ui, &cx, &hand, app.focus, &mut clicked);
    });
    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("board")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let n = view.players.len();
                // Opponents in turn order after the viewer, then the viewer at the bottom.
                for k in 1..=n {
                    let p = PlayerId(((view.viewer.index() + k) % n) as u8);
                    if k == n {
                        ui.separator();
                    }
                    ui.group(|ui| player_area(ui, &cx, p, app.focus, &mut clicked));
                }
            });
    });

    // A click on a card or player: choose its option, or focus it if it has several.
    if let (Some(opts), Some(d)) = (clicked, decision.as_ref()) {
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
            Action::ToMenu => {
                app.session = Some(session);
                app.screen = Screen::Menu;
                app.refresh_saves();
                return;
            }
            Action::Save => {
                app.session = Some(session);
                app.save_game();
                session = app.session.take().expect("just put back");
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
            Action::ReplayStep(n) => {
                session.pump(n);
            }
            Action::ReplayPlay(on) => app.replay_playing = on,
        }
        ui.ctx().request_repaint();
    }
    app.session = Some(session);
}

/// Draw the whole app.
pub fn ui(app: &mut App, ui: &mut egui::Ui) {
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

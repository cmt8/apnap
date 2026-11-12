//! Resolving the top object of the stack (CR 608). Text: docs/rules-notes/608-resolution.md.
//!
//! An effect resolves as an explicit **work list** inside the frame (D-020): each item is
//! one instruction (CR 608.2c "in the order written"). An instruction that changes the game
//! produces proposed events, which go through the replacement pipeline in an `ApplyFrame`
//! pushed on top of this frame (D-024). Resolution then resumes with the next item. Any
//! item can suspend for a choice (APNAP each-player choices, CR 101.4; replacement choices
//! inside an action, CR 616.1) and resume exactly where it stopped.

use super::{ApplyFrame, Flow, Frame, Input};
use crate::actions::{GameAction, execute};
use crate::decision::{ChoiceKind, OptionDesc, Primitive};
use crate::events::{DamageTarget, Event};
use crate::ids::{CardInstanceId, ObjectId, PlayerId};
use crate::layers::{Characteristics, chars};
use crate::replacement::{MoveCause, Proposed};
use crate::stack::{StackKind, Target};
use crate::state::GameState;
use crate::targeting::is_legal;
use crate::triggers::condition_holds;
use crate::zones::Zone;
use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use card_model::ContinuousEffect;
use card_model::{
    AbilityDef, Amount, Body, CardType, Condition, DamageBy, EachPlayerAction, Effect, Keyword,
    Measure, ObjRef, PlayerRef, Selector, TargetFilter, TargetRef,
};
use card_model::{CardDest, CardZone, Take};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Work {
    /// Perform `effect` for the `k`-th chosen mode (targets = `targets[k]`).
    Eval { k: usize, effect: Box<Effect> },
    /// "You may [effect]": ask the controller, then perform it or not.
    May { k: usize, effect: Box<Effect> },
    /// CR 101.4: players choose in APNAP order (later players know earlier choices,
    /// 101.4b); then all the actions happen simultaneously.
    EachPlayerChoose {
        choice: PlayerChoice,
        /// How many each player chooses (all they can, if fewer).
        n: u32,
        order: Vec<PlayerId>,
        next: usize,
        chosen: Vec<(PlayerId, Vec<ObjectId>)>,
    },
    /// Players choose cards in a zone of their own, in APNAP order, and then all the
    /// chosen cards are put somewhere at once (CR 101.4). In a library this is a search
    /// (CR 701.23; several players: 701.23i).
    ChooseCards {
        spec: Box<ChooseSpec>,
        order: Vec<PlayerId>,
        next: usize,
        chosen: Vec<(PlayerId, Vec<ObjectId>)>,
        /// The player choosing now has said they will ("may search": `spec.optional`).
        #[serde(default)]
        willing: bool,
    },
    /// CR 701.24: these players shuffle their libraries.
    Shuffle { players: Vec<PlayerId> },
    /// Put cards somewhere. Two or more going to the top or bottom of `player`'s library
    /// are put there in an order chosen by `by` — their owner (CR 401.4), or the player an
    /// effect has put them there "in any order" — unless `random`.
    Place {
        player: PlayerId,
        by: PlayerId,
        cards: Vec<ObjectId>,
        to: CardDest,
        random: bool,
        ordered: bool,
    },
    /// CR 701.22 (scry) / 701.25 (surveil) for `players`, one after the other; the first
    /// is deciding about the cards in `looking` (top card first).
    Scry {
        players: Vec<PlayerId>,
        n: u32,
        surveil: bool,
        looking: Vec<ObjectId>,
    },
    /// "Look at the top `n` cards of `player`'s library; put some somewhere and the rest
    /// somewhere else." `chooser` — the effect's controller — looks, picks and orders.
    /// `looking`: the cards, top first, once they look.
    Dig {
        player: PlayerId,
        chooser: PlayerId,
        n: u32,
        spec: Box<DigSpec>,
        looking: Vec<ObjectId>,
    },
    /// Zone changes about to be proposed. An Aura that would be put onto the battlefield
    /// without being cast needs something to enchant first (CR 303.4f–g); `checked` moves
    /// have been looked at.
    Move {
        moves: Vec<Proposed>,
        checked: usize,
    },
    /// CR 118.12: `player` may pay `cost` now; then `paid` or `unpaid` is performed.
    Pay {
        k: usize,
        player: PlayerId,
        cost: Box<card_model::Cost>,
        paid: Box<Effect>,
        unpaid: Box<Effect>,
    },
    /// "[That player] may [effect]": `player`, not the controller, decides (CR 608.2d:
    /// the player the instruction names makes its choices).
    PlayerMay {
        k: usize,
        player: PlayerId,
        effect: Box<Effect>,
    },
    /// "If you do, [then]": the instruction before it has been carried out; `then` happens
    /// if that changed anything — any event since `since` (`GameState::event_count`). An
    /// instruction with nothing left to act on (the card is gone) does nothing at all.
    IfDone {
        k: usize,
        then: Box<Effect>,
        since: u64,
    },
    /// CR 106.4: `player` adds `count` mana of one type they choose from `of`.
    AddManaOf {
        player: PlayerId,
        of: Vec<crate::mana::ManaType>,
        count: u32,
    },
    /// The instruction being noted ([`Effect::Note`]) has been performed: the tokens it
    /// created join "those cards", and noting stops.
    EndNote,
    /// One chosen mode is done: what it noted means nothing to the next (CR 700.2).
    ForgetNoted,
}

/// One of "those cards" (D-069): a card an instruction of this resolution moved, or a
/// token it created — followed through the later moves this resolution makes of it
/// (CR 400.7j: other parts of the effect can find an object it moved to a public zone).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotedCard {
    /// The object it is now, as far as this resolution knows.
    pub object: ObjectId,
    /// Where this resolution last put it (or left it, if a search found it).
    pub at: Zone,
    /// Later instructions can act on it: it went to a public zone, or was found by a
    /// search and stayed where it was.
    pub findable: bool,
    /// As it was just before the noted instruction moved it (CR 608.2h: last known
    /// information of a permanent that left the battlefield).
    pub before: Option<Characteristics>,
}

/// An instruction whose results are being noted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Noting {
    /// Card instances numbered from here on were created while it was performed: tokens.
    pub tokens_from: u32,
    /// Only cards put into this zone are noted (the first place of a Dig).
    pub only_to: Option<Zone>,
}

/// A zone change this resolution proposed: looked at once the events have been performed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Moving {
    pub old: ObjectId,
    pub card: CardInstanceId,
    pub to: Zone,
    pub before: Characteristics,
}

/// The fixed part of a [`Work::ChooseCards`] (amounts already worked out, CR 608.2h).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChooseSpec {
    pub from: CardZone,
    pub cards: Vec<card_model::Selector>,
    pub count: u32,
    pub must: bool,
    pub to: CardDest,
    pub reveal: bool,
    pub shuffle: bool,
    /// Each player decides whether to (`Effect::ChooseCards::optional`).
    #[serde(default)]
    pub optional: bool,
}

/// The fixed part of a [`Work::Dig`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DigSpec {
    pub reveal: bool,
    pub take: Take,
    pub cards: Vec<card_model::Selector>,
    pub to: CardDest,
    pub reveal_taken: bool,
    pub rest: CardDest,
    pub rest_random: bool,
}

/// What each player picks in a [`Work::EachPlayerChoose`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlayerChoice {
    /// CR 701.9: cards from their hand to discard.
    Discard,
    /// CR 701.21: permanents they control that match, to sacrifice.
    Sacrifice(Box<card_model::Selector>),
}

/// A target of the resolving spell or ability as it is now, or as it last existed where
/// it was targeted (CR 608.2h).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetInfo {
    pub object: ObjectId,
    /// It is a spell on the stack (else a permanent).
    pub spell: bool,
    pub controller: PlayerId,
    pub owner: PlayerId,
    pub power: Option<i32>,
    pub toughness: Option<i32>,
    /// CR 202.3e: on the stack, X counts as the number chosen for it.
    pub mana_value: u32,
    /// What its damage does (CR 702.2b, 702.15b).
    pub deathtouch: bool,
    pub lifelink: bool,
}

impl TargetInfo {
    /// The object as it is now, if it is still where it was targeted (CR 400.7: anywhere
    /// else it would be a new object).
    fn now(st: &GameState, object: ObjectId) -> Option<TargetInfo> {
        let o = st.objects.get(&object)?;
        let c = chars(st, object);
        // CR 202.3b: a transformed back face has its front face's mana value.
        let printed = c.mana_value();
        match o.zone {
            Zone::Stack => {
                let s = st.stack_object(object)?;
                let xs = c.mana_cost.as_ref().map_or(0, |m| {
                    m.symbols
                        .iter()
                        .filter(|y| **y == card_model::ManaSymbol::X)
                        .count() as u32
                });
                Some(TargetInfo {
                    object,
                    spell: true,
                    controller: s.controller,
                    owner: o.owner,
                    power: c.power,
                    toughness: c.toughness,
                    mana_value: printed + xs * s.x,
                    deathtouch: c.has(Keyword::Deathtouch),
                    lifelink: c.has(Keyword::Lifelink),
                })
            }
            Zone::Battlefield => Some(TargetInfo {
                object,
                spell: false,
                controller: c.controller,
                owner: o.owner,
                power: c.power,
                toughness: c.toughness,
                mana_value: printed,
                deathtouch: c.has(Keyword::Deathtouch),
                lifelink: c.has(Keyword::Lifelink),
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveFrame {
    /// The stack object being resolved (`None` until started).
    pub object: Option<ObjectId>,
    pub controller: PlayerId,
    /// What deals damage etc.: the spell itself, or the ability's source (CR 113.7a).
    pub source: ObjectId,
    /// The ability's source as it last existed, if it has left the battlefield.
    pub source_lki: Option<Characteristics>,
    /// The object whose event triggered this ability, and its last known information.
    #[serde(default)]
    pub subject: Option<ObjectId>,
    #[serde(default)]
    pub subject_lki: Option<Characteristics>,
    pub x: u32,
    pub targets: Vec<Vec<Target>>,
    pub filters: Vec<Vec<TargetFilter>>,
    /// Remaining work; last = next.
    pub work: Vec<Work>,
    /// The total power of what was sacrificed to pay for this (CR 608.2h).
    #[serde(default)]
    pub sacrificed_power: u32,
    /// What else the triggering event carries ("that player", "that much").
    #[serde(default)]
    pub facts: crate::triggers::EventFacts,
    /// Where the resolving spell goes instead of its owner's graveyard, and whether the
    /// owner then shuffles ("exile this spell", "shuffle it into its owner's library").
    #[serde(default)]
    pub spell_to: Option<(card_model::CardDest, bool)>,
    /// What is known about each target that was legal as resolution began (`[k][slot]`):
    /// kept up to date while it is where it was targeted, so that once it is gone it is
    /// what it last was there (CR 608.2h) — "its controller", "its power". A target that
    /// was illegal tells nothing (CR 608.2b).
    #[serde(default)]
    pub target_info: Vec<Vec<Option<TargetInfo>>>,
    /// The card the resolving text is written on: the spell's, or the ability's (which
    /// is another card's than its source's when the ability was given to the source).
    pub card: card_model::CardDefId,
    /// "Those cards": what instructions of this resolution noted (D-069).
    #[serde(default)]
    pub noted: Vec<NotedCard>,
    /// The instruction being performed is noted.
    #[serde(default)]
    pub noting: Option<Noting>,
    /// Zone changes proposed and not yet looked at (for noting, and to follow those cards).
    #[serde(default)]
    pub moving: Vec<Moving>,
}

impl ResolveFrame {
    pub(crate) fn new() -> Self {
        ResolveFrame {
            object: None,
            controller: PlayerId(0),
            source: ObjectId(0),
            source_lki: None,
            subject: None,
            subject_lki: None,
            x: 0,
            targets: Vec::new(),
            filters: Vec::new(),
            work: Vec::new(),
            sacrificed_power: 0,
            facts: Default::default(),
            spell_to: None,
            target_info: Vec::new(),
            card: card_model::CardDefId(0),
            noted: Vec::new(),
            noting: None,
            moving: Vec::new(),
        }
    }

    /// The events go through the replacement pipeline in an `ApplyFrame`; the zone changes
    /// among them are looked at when this frame resumes, if they concern "those cards".
    fn apply(&mut self, st: &GameState, events: Vec<Proposed>) -> Frame {
        for e in &events {
            if let Proposed::Move { object, to, .. } = e
                && (self.noting.is_some() || self.noted.iter().any(|n| n.object == *object))
                && let Some(o) = st.objects.get(object)
            {
                self.moving.push(Moving {
                    old: *object,
                    card: o.card_instance,
                    to: *to,
                    before: chars(st, *object),
                });
            }
        }
        Frame::Apply(ApplyFrame::new(st, events))
    }

    /// What became of the zone changes this resolution proposed (CR 400.7: a card that
    /// moved is a new object; this resolution can still find it, CR 400.7j).
    fn settle(&mut self, st: &GameState) {
        for m in core::mem::take(&mut self.moving) {
            let Some(now) = st.objects.values().find(|o| o.card_instance == m.card) else {
                continue;
            };
            if now.id == m.old {
                continue; // it didn't move (indestructible, a token that can't, …)
            }
            let findable = !matches!(m.to, Zone::Library | Zone::Hand);
            if let Some(n) = self.noted.iter_mut().find(|n| n.object == m.old) {
                (n.object, n.at, n.findable) = (now.id, m.to, findable);
            } else if let Some(noting) = &self.noting
                && noting.only_to.is_none_or(|z| z == m.to)
            {
                self.noted.push(NotedCard {
                    object: now.id,
                    at: m.to,
                    findable,
                    before: Some(m.before),
                });
            }
        }
    }

    /// Those of "those cards" that later instructions can act on: still where this
    /// resolution put them, in order.
    fn findable_noted(&self, st: &GameState) -> Vec<ObjectId> {
        self.noted
            .iter()
            .filter(|n| n.findable && st.objects.get(&n.object).is_some_and(|o| o.zone == n.at))
            .map(|n| n.object)
            .collect()
    }

    /// Is `id` one of "those cards" (as far as `sel` asks)?
    fn noted_ok(&self, st: &GameState, sel: &Selector, id: ObjectId) -> bool {
        match sel.noted {
            None => true,
            Some(want) => self.findable_noted(st).contains(&id) == want,
        }
    }

    /// Every permanent matching `sel` right now, in timestamp order.
    fn matching(&self, st: &GameState, sel: &Selector) -> Vec<ObjectId> {
        crate::layers::matching(st, sel, self.source, self.controller)
            .into_iter()
            .filter(|&id| self.noted_ok(st, sel, id))
            .collect()
    }

    /// [`card_fits`], with "those cards" known.
    fn fits(&self, st: &GameState, id: ObjectId, selectors: &[Selector]) -> bool {
        selectors.is_empty()
            || selectors.iter().any(|s| {
                card_fits(
                    st,
                    id,
                    core::slice::from_ref(s),
                    self.this_card(st),
                    self.controller,
                ) && self.noted_ok(st, s, id)
            })
    }

    /// One of "those cards" as an amount counts it: as it is now if it is on the
    /// battlefield, otherwise as it was before it moved (CR 608.2h).
    fn noted_chars(&self, st: &GameState, n: &NotedCard) -> Option<Characteristics> {
        let here = st.objects.get(&n.object);
        match (here, &n.before) {
            (Some(o), _) if o.zone == Zone::Battlefield => Some(chars(st, n.object)),
            (_, Some(before)) => Some(before.clone()),
            (Some(_), None) => Some(chars(st, n.object)),
            (None, None) => None,
        }
    }

    /// "If …" of an instruction, asked as it is reached.
    fn holds(&self, st: &GameState, k: usize, c: &Condition) -> bool {
        match c {
            Condition::Compare { amount, cmp, n } => cmp.holds(self.amount(st, k, amount), *n),
            other => condition_holds(st, Some(other), self.controller, self.source),
        }
    }

    /// The rest of what a mana ability does as it resolves (CR 605.3b): no stack object,
    /// no targets (CR 605.5a). `object` is the source itself, which is not on the stack,
    /// so resolving ends without anything leaving the stack.
    pub(crate) fn mana_ability(
        controller: PlayerId,
        source: ObjectId,
        card: card_model::CardDefId,
        effect: Effect,
        source_lki: Option<Characteristics>,
    ) -> Self {
        let mut f = ResolveFrame::new();
        f.object = Some(source);
        f.controller = controller;
        f.source = source;
        f.card = card;
        f.source_lki = source_lki;
        f.targets = vec![Vec::new()];
        f.filters = vec![Vec::new()];
        f.target_info = vec![Vec::new()];
        f.work = vec![Work::Eval {
            k: 0,
            effect: Box::new(effect),
        }];
        f
    }

    /// Everything `r` stands for right now: objects and players.
    fn recipients(&self, st: &GameState, k: usize, r: TargetRef) -> Vec<Target> {
        match r {
            TargetRef::Target(_) => self.target(st, k, r).into_iter().collect(),
            TargetRef::Players(p) => self
                .players(st, k, p)
                .into_iter()
                .map(Target::Player)
                .collect(),
            TargetRef::Object(o) => self
                .objs(st, k, o)
                .into_iter()
                .map(Target::Object)
                .collect(),
        }
    }

    /// The players `r` stands for, in APNAP order (CR 101.4).
    fn players_apnap(&self, st: &GameState, k: usize, r: PlayerRef) -> Vec<PlayerId> {
        let who = self.players(st, k, r);
        st.apnap_order()
            .into_iter()
            .filter(|p| who.contains(p))
            .collect()
    }

    fn target(&self, st: &GameState, k: usize, r: TargetRef) -> Option<Target> {
        let TargetRef::Target(i) = r else {
            return None;
        };
        let t = *self.targets.get(k)?.get(i as usize)?;
        // CR 608.2b: illegal targets aren't affected.
        is_legal(
            st,
            &self.filters[k][i as usize],
            t,
            self.controller,
            self.object,
            self.ability_source(st),
        )
        .then_some(t)
    }

    /// The players an effect applies to (none if a targeted player is no longer legal).
    fn players(&self, st: &GameState, k: usize, r: PlayerRef) -> Vec<PlayerId> {
        match r {
            PlayerRef::You => st
                .in_game(self.controller)
                .then_some(self.controller)
                .into_iter()
                .collect(),
            PlayerRef::Target(i) => match self.target(st, k, TargetRef::Target(i)) {
                Some(Target::Player(p)) => vec![p],
                _ => Vec::new(),
            },
            // In turn order from the controller (CR 101.4); the effects are simultaneous.
            PlayerRef::EachOpponent => st.opponents(self.controller),
            PlayerRef::Each => st.apnap_order(),
            PlayerRef::TriggeringController => self
                .subject_chars(st)
                .map(|c| c.controller)
                .filter(|p| st.in_game(*p))
                .into_iter()
                .collect(),
            PlayerRef::TriggeringPlayer => self
                .facts
                .player
                .filter(|p| st.in_game(*p))
                .into_iter()
                .collect(),
            // The controller or owner of the targeted spell or permanent — as it last
            // existed, if it is gone by now (CR 608.2h). A player who has left the game
            // is no one an effect can apply to.
            PlayerRef::ControllerOf(i) => self
                .target_known(k, i)
                .map(|t| t.controller)
                .filter(|p| st.in_game(*p))
                .into_iter()
                .collect(),
            PlayerRef::OwnerOf(i) => self
                .target_known(k, i)
                .map(|t| t.owner)
                .filter(|p| st.in_game(*p))
                .into_iter()
                .collect(),
            // Who controls "those cards" (as each is now on the battlefield, or last was
            // before it moved, CR 608.2h) — or who controls none of them. APNAP order.
            PlayerRef::NotedControllers | PlayerRef::NotNotedControllers => {
                let controllers: Vec<PlayerId> = self
                    .noted
                    .iter()
                    .filter_map(|n| self.noted_chars(st, n))
                    .map(|c| c.controller)
                    .collect();
                let want = r == PlayerRef::NotedControllers;
                st.apnap_order()
                    .into_iter()
                    .filter(|p| controllers.contains(p) == want)
                    .collect()
            }
        }
    }

    /// What is known about target slot `i` of the `k`-th mode (nothing if it was illegal
    /// as resolution began, CR 608.2b).
    fn target_known(&self, k: usize, i: u8) -> Option<&TargetInfo> {
        self.target_info.get(k)?.get(i as usize)?.as_ref()
    }

    /// CR 608.2h: bring what is known about each target up to date while it is still the
    /// object that was targeted, where it was targeted. Done before every step of the
    /// resolution, so that a target that has just left is known as it last existed.
    fn note_targets(&mut self, st: &GameState) {
        for known in self.target_info.iter_mut().flatten().flatten() {
            if let Some(now) = TargetInfo::now(st, known.object)
                && now.spell == known.spell
            {
                *known = now;
            }
        }
    }

    /// The one player an effect applies to (effects that can't be spread over several).
    fn player(&self, st: &GameState, k: usize, r: PlayerRef) -> Option<PlayerId> {
        self.players(st, k, r).into_iter().next()
    }

    /// An object the effect refers to, if it's still where the effect expects it.
    fn obj(&self, st: &GameState, k: usize, r: ObjRef) -> Option<ObjectId> {
        match r {
            ObjRef::Target(i) => match self.target(st, k, TargetRef::Target(i))? {
                Target::Object(o) => Some(o),
                Target::Player(_) => None,
            },
            ObjRef::This => st
                .objects
                .get(&self.source)
                .is_some_and(|o| o.zone == Zone::Battlefield)
                .then_some(self.source),
            // "This card", off the battlefield: while it is still the same object there
            // (CR 400.7).
            ObjRef::ThisCard => st
                .objects
                .get(&self.source)
                .is_some_and(|o| !matches!(o.zone, Zone::Battlefield | Zone::Stack))
                .then_some(self.source),
            // The card the triggering permanent became, while it is still in the zone it
            // went to (CR 400.7: anywhere else it is a different object).
            ObjRef::TriggeringCard => self.facts.moved.filter(|id| {
                st.objects
                    .get(id)
                    .is_some_and(|o| !matches!(o.zone, Zone::Battlefield | Zone::Stack))
            }),
            // "It": only while it is still the object that triggered this (CR 400.7).
            ObjRef::Triggering => self.subject.filter(|id| {
                st.objects
                    .get(id)
                    .is_some_and(|o| o.zone == Zone::Battlefield)
            }),
            ObjRef::Noted => self.findable_noted(st).first().copied(),
        }
    }

    /// Every object `r` stands for: "those cards" can be several.
    fn objs(&self, st: &GameState, k: usize, r: ObjRef) -> Vec<ObjectId> {
        match r {
            ObjRef::Noted => self.findable_noted(st),
            other => self.obj(st, k, other).into_iter().collect(),
        }
    }

    /// CR 701.27g: an ability of a permanent that would transform it (the source) does
    /// nothing if the permanent has transformed since the ability was put on the stack.
    fn transformed_since(&self, st: &GameState, object: ObjectId) -> bool {
        object == self.source
            && self
                .object
                .and_then(|id| st.stack_object(id))
                .and_then(|so| so.source_transforms)
                .is_some_and(|n| n != st.object(object).transforms)
    }

    /// For an ability: the object it is on ("another target …" is other than it).
    fn ability_source(&self, st: &GameState) -> Option<ObjectId> {
        (self.object != Some(self.source)).then(|| self.this_card(st))
    }

    /// What "another" is other than (`triggers::this_card`): the source, or the card it
    /// became if this ability triggered on its leaving the battlefield.
    fn this_card(&self, st: &GameState) -> ObjectId {
        crate::triggers::this_card(st, self.source, self.subject, self.facts.moved)
    }

    /// The source's current characteristics, or its last known ones (CR 608.2h, 113.7a).
    fn source_chars(&self, st: &GameState) -> Option<Characteristics> {
        if st.objects.contains_key(&self.source) {
            Some(chars(st, self.source))
        } else {
            self.source_lki.clone()
        }
    }

    /// The triggering object's current characteristics, or its last known ones.
    fn subject_chars(&self, st: &GameState) -> Option<Characteristics> {
        match self.subject {
            Some(id)
                if st
                    .objects
                    .get(&id)
                    .is_some_and(|o| o.zone == Zone::Battlefield) =>
            {
                Some(chars(st, id))
            }
            _ => self.subject_lki.clone(),
        }
    }

    fn amount(&self, st: &GameState, k: usize, a: &Amount) -> u32 {
        match a {
            Amount::TriggeringPower => self
                .subject_chars(st)
                .and_then(|c| c.power)
                .unwrap_or(0)
                .max(0) as u32,
            Amount::TriggeringToughness => self
                .subject_chars(st)
                .and_then(|c| c.toughness)
                .unwrap_or(0)
                .max(0) as u32,
            Amount::Fixed(n) => *n,
            Amount::X => self.x,
            Amount::SacrificedPower => self.sacrificed_power,
            Amount::TriggeringAmount => self.facts.amount,
            // CR 608.2h: counted as the effect is applied.
            Amount::HandOf(who) => self
                .player(st, k, *who)
                .map_or(0, |p| st.zones.hands[p.index()].len() as u32),
            Amount::OfTarget { slot, what } => {
                self.target_known(k, *slot).map_or(0, |t| match what {
                    card_model::Measure::Power => t.power.unwrap_or(0).max(0) as u32,
                    card_model::Measure::Toughness => t.toughness.unwrap_or(0).max(0) as u32,
                    card_model::Measure::ManaValue => t.mana_value,
                })
            }
            // Arithmetic and tests on amounts that may themselves need this resolution.
            Amount::Calc(base, op) => op.apply(self.amount(st, k, base)),
            Amount::IfAtLeast {
                test,
                at_least,
                then,
                otherwise,
            } => {
                if self.amount(st, k, test) >= *at_least {
                    *then
                } else {
                    *otherwise
                }
            }
            Amount::SourceToughness => self
                .source_chars(st)
                .and_then(|c| c.toughness)
                .unwrap_or(0)
                .max(0) as u32,
            Amount::Count(_)
            | Amount::TotalPower(_)
            | Amount::CardsIn { .. }
            | Amount::Domain
            | Amount::Devotion(_)
            | Amount::Greatest { .. }
            | Amount::HandSize
            | Amount::LifeTotal
            | Amount::Players(_)
            | Amount::LifeAmong { .. } => {
                crate::layers::count_amount(st, a, self.source, self.controller)
            }
            Amount::SourcePower => self
                .source_chars(st)
                .and_then(|c| c.power)
                .unwrap_or(0)
                .max(0) as u32,
            Amount::Noted(sels) => self
                .noted
                .iter()
                .filter(|n| {
                    sels.is_empty()
                        || self.noted_chars(st, n).is_some_and(|c| {
                            st.objects.get(&n.object).is_some_and(|o| {
                                sels.iter().any(|s| {
                                    crate::layers::matches(
                                        st,
                                        s,
                                        &c,
                                        o,
                                        self.source,
                                        self.controller,
                                    )
                                })
                            })
                        })
                })
                .count() as u32,
            Amount::NotedTotal(what) => self
                .noted
                .iter()
                .filter_map(|n| self.noted_chars(st, n))
                .map(|c| match what {
                    Measure::Power => c.power.unwrap_or(0).max(0) as u32,
                    Measure::Toughness => c.toughness.unwrap_or(0).max(0) as u32,
                    Measure::ManaValue => c.mana_cost.as_ref().map_or(0, |m| m.mana_value()),
                })
                .sum(),
        }
    }

    /// CR 608.2h: amounts in a one-shot continuous effect are worked out once, as it begins.
    fn fix_amounts(
        &self,
        st: &GameState,
        k: usize,
        effects: Vec<ContinuousEffect>,
    ) -> Vec<ContinuousEffect> {
        effects
            .into_iter()
            .map(|e| match e {
                ContinuousEffect::ModifyPTBy { power, toughness } => {
                    let value = |s: &card_model::Signed| {
                        let n = self.amount(st, k, &s.amount) as i32;
                        if s.negative { -n } else { n }
                    };
                    ContinuousEffect::ModifyPT(value(&power), value(&toughness))
                }
                other => other,
            })
            .collect()
    }

    fn damage(&self, st: &GameState, target: DamageTarget, amount: u32) -> Proposed {
        self.damage_by(st, 0, DamageBy::This, target, amount)
            .expect("this spell or ability's source can deal damage")
    }

    /// CR 608.2h: "If an ability states that an object does something, it's the object as
    /// it exists — or as it most recently existed — that does it, not the ability."
    fn damage_by(
        &self,
        st: &GameState,
        k: usize,
        by: DamageBy,
        target: DamageTarget,
        amount: u32,
    ) -> Option<Proposed> {
        // A target that was illegal as resolution began deals nothing (CR 608.2b).
        if let DamageBy::Target(i) = by {
            let t = self.target_known(k, i)?;
            return Some(Proposed::Damage {
                source: t.object,
                deathtouch: t.deathtouch,
                lifelink: t.lifelink,
                target,
                amount,
                combat: false,
            });
        }
        let (source, c) = match (by, self.subject) {
            (DamageBy::Triggering, Some(subject)) => (subject, self.subject_chars(st)),
            _ => (self.source, self.source_chars(st)),
        };
        Some(Proposed::Damage {
            source,
            deathtouch: c.as_ref().is_some_and(|c| c.has(Keyword::Deathtouch)),
            lifelink: c.as_ref().is_some_and(|c| c.has(Keyword::Lifelink)),
            target,
            amount,
            combat: false,
        })
    }

    pub(crate) fn run(mut self, st: &mut GameState, mut input: Option<Input>) -> Flow {
        self.settle(st);
        if self.object.is_none()
            && let Some(flow) = self.start(st)
        {
            return flow;
        }
        while let Some(item) = self.work.pop() {
            self.note_targets(st);
            match item {
                Work::Eval { k, effect } => {
                    let events = self.eval(st, k, *effect);
                    if !events.is_empty() {
                        let apply = self.apply(st, events);
                        return Flow::Continue(vec![Frame::Resolve(Box::new(self)), apply]);
                    }
                }
                Work::ForgetNoted => {
                    self.noted.clear();
                    self.moving.clear();
                }
                Work::EndNote => {
                    if let Some(noting) = self.noting.take()
                        && noting.only_to.is_none_or(|z| z == Zone::Battlefield)
                    {
                        // The tokens it created (new card instances, CR 111.1).
                        let tokens: Vec<ObjectId> = st
                            .objects
                            .values()
                            .filter(|o| {
                                o.card_instance.0 >= noting.tokens_from
                                    && o.zone == Zone::Battlefield
                                    && !self.noted.iter().any(|n| n.object == o.id)
                            })
                            .map(|o| o.id)
                            .collect();
                        self.noted
                            .extend(tokens.into_iter().map(|object| NotedCard {
                                object,
                                at: Zone::Battlefield,
                                findable: true,
                                before: None,
                            }));
                    }
                }
                Work::Pay {
                    k,
                    player,
                    cost,
                    paid,
                    unpaid,
                } => match input.take() {
                    // CR 118.3: a player can't pay a cost they can't pay in full; then
                    // there is nothing to ask.
                    None if st.in_game(player)
                        && super::cast::can_pay(st, player, self.source, &cost) =>
                    {
                        self.work.push(Work::Pay {
                            k,
                            player,
                            cost,
                            paid,
                            unpaid,
                        });
                        return Flow::ask_one(
                            Frame::Resolve(Box::new(self)),
                            player,
                            ChoiceKind::PayUnlessCost,
                            vec![OptionDesc::Accept, OptionDesc::Decline],
                        );
                    }
                    None => self.work.push(Work::Eval { k, effect: unpaid }),
                    Some(inp) => {
                        if matches!(inp.picked_one(), Some(OptionDesc::Accept)) {
                            // The payment runs first (mana abilities, choices); this
                            // resolution carries on with `paid` after it.
                            let source = self.source;
                            self.work.push(Work::Eval { k, effect: paid });
                            return Flow::Continue(vec![
                                Frame::Resolve(Box::new(self)),
                                Frame::Cast(super::cast::CastFrame::payment(player, source, *cost)),
                            ]);
                        }
                        self.work.push(Work::Eval { k, effect: unpaid });
                    }
                },
                Work::IfDone { k, then, since } => {
                    if st.event_count > since {
                        self.work.push(Work::Eval { k, effect: then });
                    }
                }
                Work::PlayerMay { k, player, effect } => match input.take() {
                    None if st.in_game(player) => {
                        self.work.push(Work::PlayerMay { k, player, effect });
                        return Flow::ask_one(
                            Frame::Resolve(Box::new(self)),
                            player,
                            ChoiceKind::MayAbility,
                            vec![OptionDesc::Accept, OptionDesc::Decline],
                        );
                    }
                    // CR 800.4a: a player who has left the game decides nothing.
                    None => {}
                    Some(inp) => {
                        if matches!(inp.picked_one(), Some(OptionDesc::Accept)) {
                            self.work.push(Work::Eval { k, effect });
                        }
                    }
                },
                Work::AddManaOf { player, of, count } => {
                    let mana = match input.take() {
                        Some(inp) => match inp.picked_one() {
                            Some(OptionDesc::Mana { mana }) => Some(*mana),
                            other => unreachable!("{other:?}"),
                        },
                        None if of.len() == 1 => Some(of[0]),
                        None if st.in_game(player) => {
                            let options =
                                of.iter().map(|&mana| OptionDesc::Mana { mana }).collect();
                            self.work.push(Work::AddManaOf { player, of, count });
                            return Flow::ask_one(
                                Frame::Resolve(Box::new(self)),
                                player,
                                ChoiceKind::ChooseColor,
                                options,
                            );
                        }
                        None => None,
                    };
                    if let Some(mana) = mana {
                        for _ in 0..count {
                            execute(st, GameAction::AddMana { player, mana });
                        }
                    }
                }
                Work::May { k, effect } => match input.take() {
                    None if st.in_game(self.controller) => {
                        let controller = self.controller;
                        self.work.push(Work::May { k, effect });
                        return Flow::ask_one(
                            Frame::Resolve(Box::new(self)),
                            controller,
                            ChoiceKind::MayAbility,
                            vec![OptionDesc::Accept, OptionDesc::Decline],
                        );
                    }
                    None => {}
                    Some(inp) => {
                        if matches!(inp.picked_one(), Some(OptionDesc::Accept)) {
                            self.work.push(Work::Eval { k, effect });
                        }
                    }
                },
                Work::EachPlayerChoose {
                    choice,
                    n,
                    order,
                    mut next,
                    mut chosen,
                } => {
                    if let Some(inp) = input.take() {
                        let objs = inp
                            .picked()
                            .into_iter()
                            .map(|o| match o {
                                OptionDesc::Card { card } => *card,
                                _ => unreachable!(),
                            })
                            .collect();
                        chosen.push((order[next], objs));
                        next += 1;
                    }
                    while let Some(&p) = order.get(next) {
                        let (kind, mut pool) =
                            each_player_options(st, &choice, p, self.source, self.controller);
                        if let PlayerChoice::Sacrifice(sel) = &choice {
                            pool.retain(|&id| self.noted_ok(st, sel, id));
                        }
                        let n = (n as usize).min(pool.len());
                        if !st.in_game(p) || n == 0 {
                            chosen.push((p, Vec::new()));
                            next += 1;
                            continue;
                        }
                        self.work.push(Work::EachPlayerChoose {
                            choice,
                            n: n as u32,
                            order,
                            next,
                            chosen,
                        });
                        let options = pool
                            .into_iter()
                            .map(|card| OptionDesc::Card { card })
                            .collect();
                        return Flow::ask(
                            Frame::Resolve(Box::new(self)),
                            p,
                            kind,
                            Primitive::PickN { min: n, max: n },
                            options,
                        );
                    }
                    // All players have chosen: the actions happen simultaneously.
                    let cause = match choice {
                        PlayerChoice::Discard => MoveCause::Discard,
                        PlayerChoice::Sacrifice(_) => MoveCause::Sacrifice,
                    };
                    let events: Vec<Proposed> = chosen
                        .into_iter()
                        .flat_map(|(_, objs)| objs)
                        .map(|o| Proposed::moving(o, Zone::Graveyard, cause))
                        .collect();
                    if !events.is_empty() {
                        let apply = self.apply(st, events);
                        return Flow::Continue(vec![Frame::Resolve(Box::new(self)), apply]);
                    }
                }
                Work::ChooseCards {
                    spec,
                    order,
                    mut next,
                    mut chosen,
                    mut willing,
                } => {
                    if let Some(inp) = input.take() {
                        if inp.decision.kind == ChoiceKind::MayAbility {
                            // "May search": one who won't, doesn't (nor shuffles).
                            willing = matches!(inp.picked_one(), Some(OptionDesc::Accept));
                            if !willing {
                                next += 1;
                            }
                        } else {
                            chosen.push((order[next], picked_cards(&inp)));
                            next += 1;
                            willing = false;
                        }
                    }
                    while let Some(&p) = order.get(next) {
                        if !st.in_game(p) {
                            next += 1;
                            continue;
                        }
                        // Each player decides for themselves, before looking.
                        if spec.optional && !willing {
                            self.work.push(Work::ChooseCards {
                                spec,
                                order,
                                next,
                                chosen,
                                willing,
                            });
                            return Flow::ask_one(
                                Frame::Resolve(Box::new(self)),
                                p,
                                ChoiceKind::MayAbility,
                                vec![OptionDesc::Accept, OptionDesc::Decline],
                            );
                        }
                        willing = false;
                        // Choosing among cards found or revealed before ("put one onto the
                        // battlefield and the other into your hand") is no new search.
                        let library = spec.from == CardZone::Library && !among_noted(&spec);
                        if library {
                            // CR 701.23a: the player looks at every card in the library.
                            st.emit(Event::SearchedLibrary { player: p });
                        }
                        let mut pool: Vec<ObjectId> = zone_cards(st, spec.from, p)
                            .into_iter()
                            .filter(|&id| self.fits(st, id, &spec.cards))
                            .collect();
                        if library {
                            // Offered by name: the order of the library isn't part of
                            // what a search is for.
                            pool.sort_by(|a, b| {
                                (&st.card_def(*a).name, *a).cmp(&(&st.card_def(*b).name, *b))
                            });
                        }
                        let max = (spec.count as usize).min(pool.len());
                        // CR 701.23b: cards with a stated quality needn't be found;
                        // 701.23d: a plain quantity must be (as many as possible).
                        let min = if spec.must { max } else { 0 };
                        if max == 0 || (min == max && max == pool.len()) {
                            pool.truncate(max);
                            chosen.push((p, pool));
                            next += 1;
                            continue;
                        }
                        let kind = if library {
                            ChoiceKind::SearchLibrary
                        } else {
                            ChoiceKind::ChooseCards
                        };
                        let options = pool
                            .into_iter()
                            .map(|card| OptionDesc::Card { card })
                            .collect();
                        self.work.push(Work::ChooseCards {
                            spec,
                            order,
                            next,
                            chosen,
                            willing: false,
                        });
                        return Flow::ask(
                            Frame::Resolve(Box::new(self)),
                            p,
                            kind,
                            Primitive::PickN { min, max },
                            options,
                        );
                    }
                    // Everyone has chosen.
                    let players: Vec<PlayerId> = chosen.iter().map(|(p, _)| *p).collect();
                    if spec.reveal {
                        for (player, cards) in &chosen {
                            if !cards.is_empty() {
                                st.emit(Event::Revealed {
                                    player: *player,
                                    cards: cards.clone(),
                                });
                            }
                        }
                    }
                    // Found and left where they are, for the instructions after this one.
                    if spec.to == CardDest::Stay {
                        if self.noting.is_some() {
                            let at = card_zone(spec.from);
                            for &object in chosen.iter().flat_map(|(_, cards)| cards) {
                                self.noted.push(NotedCard {
                                    object,
                                    at,
                                    findable: true,
                                    before: Some(chars(st, object)),
                                });
                            }
                        }
                        if spec.shuffle {
                            self.work.push(Work::Shuffle { players });
                        }
                        continue;
                    }
                    let to_library =
                        matches!(spec.to, CardDest::LibraryTop | CardDest::LibraryBottom);
                    let within = to_library && spec.from == CardZone::Library;
                    // Work is done last-pushed first. CR 701.24b: "search, shuffle, then
                    // put the card on top" shuffles before the found cards are placed;
                    // otherwise the shuffle comes after the cards have left.
                    if spec.shuffle && !within {
                        self.work.push(Work::Shuffle {
                            players: players.clone(),
                        });
                    }
                    if to_library {
                        for (player, cards) in chosen.into_iter().rev() {
                            if !cards.is_empty() {
                                self.work.push(Work::Place {
                                    player,
                                    by: player,
                                    cards,
                                    to: spec.to,
                                    random: false,
                                    ordered: false,
                                });
                            }
                        }
                    } else {
                        // All at once (CR 101.4).
                        self.work.push(Work::Move {
                            moves: chosen
                                .into_iter()
                                .flat_map(|(_, cards)| cards)
                                .map(|o| move_to(o, spec.to))
                                .collect(),
                            checked: 0,
                        });
                    }
                    if spec.shuffle && within {
                        self.work.push(Work::Shuffle { players });
                    }
                }
                Work::Shuffle { players } => {
                    for player in players {
                        if st.in_game(player) {
                            execute(st, GameAction::Shuffle { player });
                        }
                    }
                }
                Work::Place {
                    player,
                    by,
                    mut cards,
                    to,
                    random,
                    ordered,
                } => {
                    let top = to == CardDest::LibraryTop;
                    let library = top || to == CardDest::LibraryBottom;
                    if let Some(inp) = input.take() {
                        // The answer orders the options, which were these cards.
                        cards = inp
                            .order()
                            .iter()
                            .map(|&i| match &inp.decision.options[i] {
                                OptionDesc::Card { card } => *card,
                                o => unreachable!("not a card: {o:?}"),
                            })
                            .collect();
                    } else if library && cards.len() > 1 && !random && !ordered && st.in_game(by) {
                        // CR 401.4: their owner arranges them (or whoever the effect
                        // lets put them there in any order).
                        let options = cards
                            .iter()
                            .map(|&card| OptionDesc::Card { card })
                            .collect();
                        self.work.push(Work::Place {
                            player,
                            by,
                            cards,
                            to,
                            random,
                            ordered: true,
                        });
                        let kind = if top {
                            ChoiceKind::ScryOrder
                        } else {
                            ChoiceKind::OrderBottom
                        };
                        return Flow::ask(
                            Frame::Resolve(Box::new(self)),
                            by,
                            kind,
                            Primitive::Order,
                            options,
                        );
                    }
                    cards.retain(|c| st.objects.contains_key(c));
                    // Cards already in that library stay in it (and stay the same
                    // objects); anything else changes zones.
                    let (staying, moving): (Vec<ObjectId>, Vec<ObjectId>) =
                        cards.into_iter().partition(|c| {
                            let o = st.object(*c);
                            library && o.zone == Zone::Library && o.owner == player
                        });
                    if !staying.is_empty() {
                        let (top, bottom) = if top {
                            (staying, Vec::new())
                        } else {
                            (Vec::new(), staying)
                        };
                        execute(
                            st,
                            GameAction::ArrangeLibrary {
                                player,
                                top,
                                bottom,
                                random_bottom: random,
                            },
                        );
                    }
                    if !moving.is_empty() {
                        // Each card put on a library goes on top of (or under) the ones
                        // before it: the first listed must end up on top, the last listed
                        // bottom-most.
                        let moves: Vec<Proposed> = if top {
                            moving.into_iter().rev().map(|o| move_to(o, to)).collect()
                        } else {
                            moving.into_iter().map(|o| move_to(o, to)).collect()
                        };
                        self.work.push(Work::Move { moves, checked: 0 });
                    }
                }
                Work::Scry {
                    mut players,
                    n,
                    surveil,
                    looking,
                } => {
                    if let Some(inp) = input.take() {
                        let player = players.remove(0);
                        let picked = picked_cards(&inp);
                        let rest: Vec<ObjectId> = looking
                            .iter()
                            .copied()
                            .filter(|c| !picked.contains(c))
                            .collect();
                        let (count, away) = (looking.len() as u32, picked.len() as u32);
                        st.emit(if surveil {
                            Event::Surveilled {
                                player,
                                count,
                                graveyard: away,
                            }
                        } else {
                            Event::Scried {
                                player,
                                count,
                                bottom: away,
                            }
                        });
                        // Last pushed is done first: the cards that leave the top, then
                        // the order of the rest, then the next player.
                        if !players.is_empty() {
                            self.work.push(Work::Scry {
                                players,
                                n,
                                surveil,
                                looking: Vec::new(),
                            });
                        }
                        if rest.len() > 1 {
                            self.work.push(Work::Place {
                                player,
                                by: player,
                                cards: rest,
                                to: CardDest::LibraryTop,
                                random: false,
                                ordered: false,
                            });
                        }
                        if !picked.is_empty() {
                            self.work.push(Work::Place {
                                player,
                                by: player,
                                cards: picked,
                                to: if surveil {
                                    CardDest::Graveyard
                                } else {
                                    CardDest::LibraryBottom
                                },
                                random: false,
                                ordered: false,
                            });
                        }
                        continue;
                    }
                    while let Some(&player) = players.first() {
                        let looking = top_cards(st, player, n);
                        if !st.in_game(player) || looking.is_empty() {
                            players.remove(0);
                            continue;
                        }
                        st.emit(Event::LookedAtLibrary {
                            player,
                            count: looking.len() as u32,
                        });
                        let options: Vec<OptionDesc> = looking
                            .iter()
                            .map(|&card| OptionDesc::Card { card })
                            .collect();
                        let max = options.len();
                        self.work.push(Work::Scry {
                            players,
                            n,
                            surveil,
                            looking,
                        });
                        return Flow::ask(
                            Frame::Resolve(Box::new(self)),
                            player,
                            if surveil {
                                ChoiceKind::SurveilGraveyard
                            } else {
                                ChoiceKind::ScryBottom
                            },
                            Primitive::PickN { min: 0, max },
                            options,
                        );
                    }
                }
                Work::Dig {
                    player,
                    chooser,
                    n,
                    spec,
                    mut looking,
                } => {
                    let taken: Vec<ObjectId> = if let Some(inp) = input.take() {
                        picked_cards(&inp)
                    } else {
                        looking = top_cards(st, player, n);
                        if !st.in_game(player) || !st.in_game(chooser) || looking.is_empty() {
                            continue;
                        }
                        st.emit(Event::LookedAtLibrary {
                            player,
                            count: looking.len() as u32,
                        });
                        if spec.reveal {
                            st.emit(Event::Revealed {
                                player,
                                cards: looking.clone(),
                            });
                        } else {
                            // CR 701.20e: only the player looking sees them.
                            st.emit(Event::LookedAt {
                                player: chooser,
                                cards: looking.clone(),
                            });
                        }
                        let fit: Vec<ObjectId> = looking
                            .iter()
                            .copied()
                            .filter(|&id| self.fits(st, id, &spec.cards))
                            .collect();
                        let (min, max) = match spec.take {
                            Take::All => (fit.len(), fit.len()),
                            Take::Exactly(k) => {
                                let m = (k as usize).min(fit.len());
                                (m, m)
                            }
                            Take::UpTo(k) => (0, (k as usize).min(fit.len())),
                            Take::Any => (0, fit.len()),
                        };
                        if max > 0 && !(min == max && max == fit.len()) {
                            let options = fit
                                .into_iter()
                                .map(|card| OptionDesc::Card { card })
                                .collect();
                            self.work.push(Work::Dig {
                                player,
                                chooser,
                                n,
                                spec,
                                looking,
                            });
                            return Flow::ask(
                                Frame::Resolve(Box::new(self)),
                                chooser,
                                ChoiceKind::ChooseCards,
                                Primitive::PickN { min, max },
                                options,
                            );
                        }
                        // Nothing to decide: none can be taken, or all of them must be.
                        if max == 0 { Vec::new() } else { fit }
                    };
                    let rest: Vec<ObjectId> = looking
                        .iter()
                        .copied()
                        .filter(|c| !taken.contains(c))
                        .collect();
                    if spec.reveal_taken && !spec.reveal && !taken.is_empty() {
                        st.emit(Event::Revealed {
                            player,
                            cards: taken.clone(),
                        });
                    }
                    let in_library =
                        |d: CardDest| matches!(d, CardDest::LibraryTop | CardDest::LibraryBottom);
                    if !in_library(spec.to) && !in_library(spec.rest) {
                        // Both piles leave the library: at once.
                        let moves: Vec<Proposed> = taken
                            .into_iter()
                            .map(|o| move_to(o, spec.to))
                            .chain(rest.into_iter().map(|o| move_to(o, spec.rest)))
                            .collect();
                        self.work.push(Work::Move { moves, checked: 0 });
                    } else {
                        // Last pushed is done first: the taken cards, then the rest.
                        if !rest.is_empty() {
                            self.work.push(Work::Place {
                                player,
                                by: chooser,
                                cards: rest,
                                to: spec.rest,
                                random: spec.rest_random,
                                ordered: false,
                            });
                        }
                        if !taken.is_empty() {
                            self.work.push(Work::Place {
                                player,
                                by: chooser,
                                cards: taken,
                                to: spec.to,
                                random: false,
                                ordered: false,
                            });
                        }
                    }
                }
                Work::Move {
                    mut moves,
                    mut checked,
                } => {
                    if let Some(inp) = input.take() {
                        let host = match inp.picked_one() {
                            Some(OptionDesc::Card { card }) => *card,
                            o => unreachable!("not an Aura host: {o:?}"),
                        };
                        if let Some(Proposed::Move { attach_to, .. }) = moves.get_mut(checked) {
                            *attach_to = Some(host);
                        }
                        checked += 1;
                    }
                    while checked < moves.len() {
                        let Some((chooser, mut hosts)) = aura_hosts(st, &moves[checked]) else {
                            checked += 1;
                            continue;
                        };
                        match hosts.len() {
                            // CR 303.4g: with nothing to enchant it stays where it is.
                            0 => {
                                moves.remove(checked);
                            }
                            1 => {
                                if let Proposed::Move { attach_to, .. } = &mut moves[checked] {
                                    *attach_to = hosts.pop();
                                }
                                checked += 1;
                            }
                            // CR 303.4f: the player putting it onto the battlefield chooses.
                            _ => {
                                let options = hosts
                                    .into_iter()
                                    .map(|card| OptionDesc::Card { card })
                                    .collect();
                                self.work.push(Work::Move { moves, checked });
                                return Flow::ask_one(
                                    Frame::Resolve(Box::new(self)),
                                    chooser,
                                    ChoiceKind::AuraHost,
                                    options,
                                );
                            }
                        }
                    }
                    if !moves.is_empty() {
                        let apply = self.apply(st, moves);
                        return Flow::Continue(vec![Frame::Resolve(Box::new(self)), apply]);
                    }
                }
            }
        }
        self.finish(st)
    }

    /// The cards in hidden zones that the player answering this frame's pending decision
    /// is looking at (CR 701.20e, 701.23a) beyond the ones offered as options: a library
    /// being searched, the cards a scry or a "look at the top N cards" shows.
    pub fn shown(&self, st: &GameState) -> Vec<ObjectId> {
        match self.work.last() {
            // Only while searching: not while deciding whether to (CR 701.23a).
            Some(Work::ChooseCards {
                spec, order, next, ..
            }) if spec.from == CardZone::Library
                && st
                    .pending
                    .as_ref()
                    .is_some_and(|d| d.kind == ChoiceKind::SearchLibrary) =>
            {
                order
                    .get(*next)
                    .map(|p| st.zones.libraries[p.index()].clone())
                    .unwrap_or_default()
            }
            Some(Work::Scry { looking, .. } | Work::Dig { looking, .. }) => looking.clone(),
            _ => Vec::new(),
        }
    }

    /// CR 608.2a–b and 608.3. Returns `Some` if resolution ends here.
    fn start(&mut self, st: &mut GameState) -> Option<Flow> {
        let top = st.stack.last().expect("resolve with empty stack").clone();
        self.object = Some(top.id);
        self.controller = top.controller;
        self.x = top.x;
        self.sacrificed_power = top.sacrificed_power;
        self.facts = top.facts;
        self.card = match &top.kind {
            StackKind::Spell => st.face_card(top.id),
            StackKind::Ability { ability, .. } => ability.card,
        };
        self.targets = top.targets.clone();
        self.source_lki = top.source_lki.clone();
        self.subject = top.subject;
        self.subject_lki = top.subject_lki.clone();
        let body: Body = match &top.kind {
            StackKind::Spell => {
                self.source = top.id;
                if chars(st, top.id).is_permanent_type() {
                    // CR 608.3b: an Aura spell checks its target; if it's illegal, the spell
                    // doesn't resolve and goes to its owner's graveyard.
                    let mut attach = None;
                    if let Some(filter) = st.card_def(top.id).enchant() {
                        let target = top.targets.first().and_then(|t| t.first()).copied();
                        match target {
                            Some(t @ Target::Object(o))
                                if is_legal(st, &filter, t, top.controller, Some(top.id), None) =>
                            {
                                attach = Some(o)
                            }
                            _ => {
                                st.emit(Event::Fizzled { object: top.id });
                                return Some(self.leave_stack(st, MoveCause::Fizzled));
                            }
                        }
                    }
                    // CR 608.3a: it becomes a permanent under its controller's control (an
                    // Aura enters attached to its target, CR 303.4).
                    st.emit(Event::Resolved { spell: top.id });
                    let mut mv = Proposed::moving(top.id, Zone::Battlefield, MoveCause::Resolve);
                    if let Proposed::Move {
                        controller,
                        attach_to,
                        ..
                    } = &mut mv
                    {
                        *controller = Some(top.controller);
                        *attach_to = attach;
                    }
                    return Some(Flow::then(Frame::Apply(ApplyFrame::new(st, vec![mv]))));
                }
                // CR 702.96c: as an overload cost changed its text.
                super::altcost::spell_body(st, top.id)
                    .expect("instants and sorceries have a spell ability")
            }
            StackKind::Ability {
                source, ability, ..
            } => {
                self.source = *source;
                match st.catalog.ability(*ability) {
                    AbilityDef::Activated { body, .. } => body.clone(),
                    AbilityDef::Triggered {
                        body,
                        intervening_if,
                        ..
                    } => {
                        // CR 603.4 / 608.2a: re-check an intervening "if" on resolution.
                        if !condition_holds(st, intervening_if.as_ref(), top.controller, *source) {
                            execute(st, GameAction::RemoveAbility { ability: top.id });
                            return Some(Flow::done());
                        }
                        body.clone()
                    }
                    other => unreachable!("not a stack ability: {other:?}"),
                }
            }
        };
        self.filters = top
            .modes
            .iter()
            .map(|&m| body.modes()[m as usize].targets.clone())
            .collect();
        // CR 608.2b: if it has targets and all of them are now illegal, it doesn't resolve.
        let mut any = false;
        let mut any_legal = false;
        for (k, ts) in self.targets.iter().enumerate() {
            for (i, &t) in ts.iter().enumerate() {
                any = true;
                any_legal |= is_legal(
                    st,
                    &self.filters[k][i],
                    t,
                    self.controller,
                    Some(top.id),
                    self.ability_source(st),
                );
            }
        }
        if any && !any_legal {
            st.emit(Event::Fizzled { object: top.id });
            return Some(self.leave_stack(st, MoveCause::Fizzled));
        }
        // What each legal target is now: "its controller", "its power" (CR 608.2h).
        self.target_info = (0..self.targets.len())
            .map(|k| {
                (0..self.targets[k].len())
                    .map(|i| match self.target(st, k, TargetRef::Target(i as u8)) {
                        Some(Target::Object(o)) => TargetInfo::now(st, o),
                        _ => None,
                    })
                    .collect()
            })
            .collect();
        // CR 700.2: chosen modes are performed in printed order (modes are stored sorted).
        for (k, &m) in top.modes.iter().enumerate().rev() {
            self.work.push(Work::Eval {
                k,
                effect: Box::new(body.modes()[m as usize].effect.clone()),
            });
            if k > 0 {
                self.work.push(Work::ForgetNoted);
            }
        }
        None
    }

    /// One instruction. Returns the events it would cause (empty if none or done directly).
    fn eval(&mut self, st: &mut GameState, k: usize, effect: Effect) -> Vec<Proposed> {
        match effect {
            Effect::Seq(v) => {
                for e in v.into_iter().rev() {
                    self.work.push(Work::Eval {
                        k,
                        effect: Box::new(e),
                    });
                }
                Vec::new()
            }
            // One event: what each instruction would do, all at once.
            Effect::Together(v) => v.into_iter().flat_map(|e| self.eval(st, k, e)).collect(),
            Effect::DealDamage { to, amount, by } => {
                let amount = self.amount(st, k, &amount);
                // Several recipients ("to each opponent") are dealt damage at once.
                self.recipients(st, k, to)
                    .into_iter()
                    .filter_map(|t| {
                        let t = match t {
                            Target::Player(p) => DamageTarget::Player(p),
                            Target::Object(o) => DamageTarget::Object(o),
                        };
                        self.damage_by(st, k, by, t, amount)
                    })
                    .collect()
            }
            Effect::DamageEachPlayer { amount } => {
                let amount = self.amount(st, k, &amount);
                st.apnap_order()
                    .into_iter()
                    .map(|p| self.damage(st, DamageTarget::Player(p), amount))
                    .collect()
            }
            Effect::Draw { who, amount } => {
                let n = self.amount(st, k, &amount);
                self.players(st, k, who)
                    .into_iter()
                    .flat_map(|player| (0..n).map(move |_| Proposed::Draw { player }))
                    .collect()
            }
            Effect::GainLife { who, amount } => {
                let amount = self.amount(st, k, &amount);
                self.players(st, k, who)
                    .into_iter()
                    .map(|player| Proposed::GainLife { player, amount })
                    .collect()
            }
            Effect::LoseLife { who, amount } => {
                let amount = self.amount(st, k, &amount);
                self.players(st, k, who)
                    .into_iter()
                    .map(|player| Proposed::LoseLife { player, amount })
                    .collect()
            }
            Effect::SetX(amount) => {
                self.x = self.amount(st, k, &amount);
                Vec::new()
            }
            // CR 106.4: the mana goes to the player's pool. (Not a mana ability: this is
            // a spell or an ability on the stack, CR 605.5.)
            Effect::AddMana { who, mana, amount } => {
                let amount = self.amount(st, k, &amount);
                for player in self.players(st, k, who) {
                    for _ in 0..amount {
                        execute(st, GameAction::AddMana { player, mana });
                    }
                }
                Vec::new()
            }
            Effect::AddManaOf { who, of, amount } => {
                let count = self.amount(st, k, &amount);
                if count > 0 {
                    // Each player chooses as their mana is added (last pushed = first).
                    for player in self.players_apnap(st, k, who).into_iter().rev() {
                        self.work.push(Work::AddManaOf {
                            player,
                            of: of.clone(),
                            count,
                        });
                    }
                }
                Vec::new()
            }
            Effect::May(effect) => {
                self.work.push(Work::May { k, effect });
                Vec::new()
            }
            Effect::ReturnToBattlefield { what } => {
                // Through `Work::Move`: an Aura returning this way needs something to
                // enchant (CR 303.4f–g).
                let moves: Vec<Proposed> = self
                    .objs(st, k, what)
                    .into_iter()
                    .map(|o| Proposed::moving(o, Zone::Battlefield, MoveCause::Rule))
                    .collect();
                if !moves.is_empty() {
                    self.work.push(Work::Move { moves, checked: 0 });
                }
                Vec::new()
            }
            Effect::DestroyAll(sel) => self
                .matching(st, &sel)
                .into_iter()
                .map(|o| Proposed::moving(o, Zone::Graveyard, MoveCause::Destroy))
                .collect(),
            Effect::DamageEach { each, amount } => {
                let amount = self.amount(st, k, &amount);
                self.matching(st, &each)
                    .into_iter()
                    .map(|o| self.damage(st, DamageTarget::Object(o), amount))
                    .collect()
            }
            // CR 701.13a
            Effect::Exile { what } => self
                .objs(st, k, what)
                .into_iter()
                .map(|o| Proposed::moving(o, Zone::Exile, MoveCause::Rule))
                .collect(),
            Effect::ExileGraveyard { who } => self
                .players(st, k, who)
                .into_iter()
                .flat_map(|p| st.zones.list(Zone::Graveyard, p).clone())
                .map(|o| Proposed::moving(o, Zone::Exile, MoveCause::Rule))
                .collect(),
            Effect::Mill { who, amount } => {
                // CR 701.17a: the top N cards (the library's last entries), all at once.
                let n = self.amount(st, k, &amount) as usize;
                self.players(st, k, who)
                    .into_iter()
                    .flat_map(|p| {
                        let library = st.zones.list(Zone::Library, p);
                        let take = n.min(library.len());
                        library[library.len() - take..]
                            .iter()
                            .rev()
                            .copied()
                            .collect::<Vec<_>>()
                    })
                    .map(|o| Proposed::moving(o, Zone::Graveyard, MoveCause::Rule))
                    .collect()
            }
            Effect::ApplyAll {
                affected,
                effects,
                until_end_of_turn,
            } => {
                // CR 611.2c: the affected set is locked in now.
                let affected = self.matching(st, &affected);
                let effects = self.fix_amounts(st, k, effects);
                if !affected.is_empty() {
                    execute(
                        st,
                        GameAction::AddContinuous {
                            source: self.source,
                            controller: self.controller,
                            affected,
                            effects,
                            until_end_of_turn,
                            card: self.card,
                        },
                    );
                }
                Vec::new()
            }
            Effect::Counter { what } => match self.target(st, k, what) {
                // CR 101.2, 614.17: "can't be countered" — this does nothing to it (D-073).
                Some(Target::Object(spell)) if crate::replacement::cant_be_countered(st, spell) => {
                    Vec::new()
                }
                // CR 701.6a: a countered spell is put into its owner's graveyard.
                Some(Target::Object(spell)) => {
                    st.emit(Event::Countered { object: spell });
                    vec![Proposed::moving(
                        spell,
                        Zone::Graveyard,
                        MoveCause::Countered,
                    )]
                }
                _ => Vec::new(),
            },
            Effect::ReturnToHand { what } => self
                .objs(st, k, what)
                .into_iter()
                .map(|o| Proposed::moving(o, Zone::Hand, MoveCause::Rule))
                .collect(),
            // CR 701.8a
            Effect::Destroy { what } => self
                .objs(st, k, what)
                .into_iter()
                .map(|o| Proposed::moving(o, Zone::Graveyard, MoveCause::Destroy))
                .collect(),
            Effect::CreateToken { token, count, who } => {
                let token = st.catalog.find(&token).expect("validated by card-db");
                let count = self.amount(st, k, &count);
                // CR 111.2: each player who creates tokens owns and controls them; "each
                // player creates …" is one event (APNAP order, CR 101.4).
                self.players_apnap(st, k, who)
                    .into_iter()
                    .map(|controller| Proposed::CreateTokens {
                        token,
                        count,
                        controller,
                        more: Vec::new(),
                    })
                    .collect()
            }
            Effect::PutCountersAll { what, kind, count } => {
                let count = self.amount(st, k, &count);
                self.matching(st, &what)
                    .into_iter()
                    .map(|object| Proposed::PutCounters {
                        object,
                        kind,
                        count,
                    })
                    .collect()
            }
            Effect::PutCounters { what, kind, count } => {
                let count = self.amount(st, k, &count);
                self.objs(st, k, what)
                    .into_iter()
                    .map(|object| Proposed::PutCounters {
                        object,
                        kind,
                        count,
                    })
                    .collect()
            }
            Effect::Lasting {
                abilities,
                about,
                until_end_of_turn,
                while_source,
            } => {
                // An effect about something that is gone doesn't begin.
                let object = about.map(|what| self.obj(st, k, what));
                if object != Some(None) {
                    let card = self.card;
                    execute(
                        st,
                        GameAction::AddLasting {
                            source: self.source,
                            controller: self.controller,
                            abilities: abilities
                                .into_iter()
                                .map(|index| card_model::AbilityId {
                                    card,
                                    index,
                                    granted: true,
                                })
                                .collect(),
                            about: object.flatten(),
                            until_end_of_turn,
                            while_source,
                        },
                    );
                }
                Vec::new()
            }
            Effect::Apply {
                what,
                effects,
                until_end_of_turn,
            } => {
                // CR 611.2c: the affected set is locked in now.
                let effects = self.fix_amounts(st, k, effects);
                let affected = self.objs(st, k, what);
                if !affected.is_empty() {
                    execute(
                        st,
                        GameAction::AddContinuous {
                            source: self.source,
                            controller: self.controller,
                            affected,
                            effects,
                            until_end_of_turn,
                            card: self.card,
                        },
                    );
                }
                Vec::new()
            }
            Effect::PreventNext { to, amount } => {
                if let Some(target) = self.target(st, k, to) {
                    execute(st, GameAction::AddShield { target, amount });
                }
                Vec::new()
            }
            Effect::EachPlayer(EachPlayerAction::Draw(n)) => st
                .apnap_order()
                .into_iter()
                .flat_map(|player| (0..n).map(move |_| Proposed::Draw { player }))
                .collect(),
            Effect::EachPlayer(action) => {
                let (choice, n) = match action {
                    EachPlayerAction::Discard(n) => (PlayerChoice::Discard, n),
                    EachPlayerAction::SacrificeCreature(n) => (
                        PlayerChoice::Sacrifice(Box::new(card_model::Selector {
                            types: vec![CardType::Creature],
                            ..Default::default()
                        })),
                        n,
                    ),
                    EachPlayerAction::Draw(_) => unreachable!("handled above"),
                };
                self.work.push(Work::EachPlayerChoose {
                    choice,
                    n,
                    order: st.apnap_order(),
                    next: 0,
                    chosen: Vec::new(),
                });
                Vec::new()
            }
            // CR 701.26: only an untapped permanent can be tapped, only a tapped one untapped.
            Effect::Transform { what } => {
                if let Some(object) = self.obj(st, k, what)
                    && can_transform(st, object)
                    && !self.transformed_since(st, object)
                {
                    execute(st, GameAction::Transform { object });
                }
                Vec::new()
            }
            Effect::Tap { what } => {
                for object in self.objs(st, k, what) {
                    if !st.object(object).tapped {
                        execute(st, GameAction::Tap { object });
                    }
                }
                Vec::new()
            }
            Effect::Untap { what } => {
                for object in self.objs(st, k, what) {
                    if st.object(object).tapped {
                        execute(st, GameAction::Untap { object });
                    }
                }
                Vec::new()
            }
            Effect::TapAll(sel) => {
                for object in self.matching(st, &sel) {
                    if !st.object(object).tapped {
                        execute(st, GameAction::Tap { object });
                    }
                }
                Vec::new()
            }
            Effect::UntapAll(sel) => {
                for object in self.matching(st, &sel) {
                    if st.object(object).tapped {
                        execute(st, GameAction::Untap { object });
                    }
                }
                Vec::new()
            }
            Effect::Discard { who, amount } => {
                let n = self.amount(st, k, &amount);
                self.work.push(Work::EachPlayerChoose {
                    choice: PlayerChoice::Discard,
                    n,
                    order: self.players_apnap(st, k, who),
                    next: 0,
                    chosen: Vec::new(),
                });
                Vec::new()
            }
            Effect::DiscardHand { who } => self
                .players_apnap(st, k, who)
                .into_iter()
                .flat_map(|p| st.zones.hands[p.index()].clone())
                .map(|card| Proposed::moving(card, Zone::Graveyard, MoveCause::Discard))
                .collect(),
            Effect::Sacrifice { who, what, amount } => {
                let n = self.amount(st, k, &amount);
                self.work.push(Work::EachPlayerChoose {
                    choice: PlayerChoice::Sacrifice(Box::new(what)),
                    n,
                    order: self.players_apnap(st, k, who),
                    next: 0,
                    chosen: Vec::new(),
                });
                Vec::new()
            }
            // CR 701.3 / 702.6a: attach this permanent (an Equipment) to the object.
            Effect::AttachThis { to } => {
                let here = st
                    .objects
                    .get(&self.source)
                    .is_some_and(|o| o.zone == Zone::Battlefield);
                if here
                    && let Some(to) = self.obj(st, k, to)
                    && to != self.source
                    // CR 301.5c: an Equipment that is also a creature can't equip.
                    && !chars(st, self.source).is(CardType::Creature)
                    && chars(st, to).is(CardType::Creature)
                    // CR 701.3b: attaching it to what it is already on does nothing.
                    && st.object(self.source).attached_to != Some(to)
                {
                    execute(
                        st,
                        GameAction::Attach {
                            aura: self.source,
                            to,
                        },
                    );
                    // CR 701.3c: a new timestamp.
                    execute(
                        st,
                        GameAction::NewTimestamp {
                            object: self.source,
                        },
                    );
                }
                Vec::new()
            }
            // CR 701.21a: a player can sacrifice only a permanent they control. "Its
            // controller sacrifices it" (CR 702.74a): whoever that is now.
            e @ (Effect::SacrificeThis | Effect::ControllerSacrificesThis) => {
                let mine = st
                    .objects
                    .get(&self.source)
                    .is_some_and(|o| o.zone == Zone::Battlefield)
                    && (matches!(e, Effect::ControllerSacrificesThis)
                        || crate::layers::controller_of(st, self.source) == self.controller);
                if mine {
                    vec![Proposed::moving(
                        self.source,
                        Zone::Graveyard,
                        MoveCause::Sacrifice,
                    )]
                } else {
                    Vec::new()
                }
            }
            Effect::ChooseCards {
                who,
                from,
                cards,
                count,
                must,
                to,
                reveal,
                shuffle,
                optional,
            } => {
                // CR 608.2h: how many is worked out once, now.
                let count = self.amount(st, k, &count);
                self.work.push(Work::ChooseCards {
                    spec: Box::new(ChooseSpec {
                        from,
                        cards,
                        count,
                        must,
                        to,
                        reveal,
                        shuffle,
                        optional,
                    }),
                    order: self.players_apnap(st, k, who),
                    next: 0,
                    chosen: Vec::new(),
                    willing: false,
                });
                Vec::new()
            }
            // CR 608.2c: "If you do" — whether the first instruction happened.
            Effect::IfDone { first, then } => {
                self.work.push(Work::IfDone {
                    k,
                    then,
                    since: st.event_count,
                });
                self.work.push(Work::Eval { k, effect: first });
                Vec::new()
            }
            Effect::PlayerMay { who, effect } => {
                if let Some(player) = self.player(st, k, who) {
                    self.work.push(Work::PlayerMay { k, player, effect });
                }
                Vec::new()
            }
            Effect::Scry { who, amount } => {
                self.scry(st, k, who, &amount, false);
                Vec::new()
            }
            Effect::Surveil { who, amount } => {
                self.scry(st, k, who, &amount, true);
                Vec::new()
            }
            Effect::Dig {
                who,
                count,
                reveal,
                take,
                cards,
                to,
                reveal_taken,
                rest,
                rest_random,
            } => {
                let n = self.amount(st, k, &count);
                let spec = DigSpec {
                    reveal,
                    take,
                    cards,
                    to,
                    reveal_taken,
                    rest,
                    rest_random,
                };
                for player in self.players_apnap(st, k, who).into_iter().rev() {
                    self.work.push(Work::Dig {
                        player,
                        // The effect's controller looks and chooses, whoever's library.
                        chooser: self.controller,
                        n,
                        spec: Box::new(spec.clone()),
                        looking: Vec::new(),
                    });
                }
                Vec::new()
            }
            Effect::ThisSpellTo { to, shuffle } => {
                self.spell_to = Some((to, shuffle));
                Vec::new()
            }
            Effect::Pay {
                who,
                cost,
                paid,
                unpaid,
            } => {
                match self.player(st, k, who) {
                    Some(player) => self.work.push(Work::Pay {
                        k,
                        player,
                        cost: Box::new(cost),
                        paid,
                        unpaid,
                    }),
                    // Nobody to pay: it isn't paid.
                    None => self.work.push(Work::Eval { k, effect: unpaid }),
                }
                Vec::new()
            }
            Effect::PutInto {
                what,
                to,
                shuffle,
                controller,
            } => {
                let objs = self.objs(st, k, what);
                let controller = controller.and_then(|p| self.player(st, k, p));
                if let (CardDest::Battlefield { .. }, Some(controller)) = (to, controller) {
                    // "… under your control": all at once.
                    let moves = objs
                        .into_iter()
                        .map(|o| {
                            let mut mv = move_to(o, to);
                            if let Proposed::Move { controller: c, .. } = &mut mv {
                                *c = Some(controller);
                            }
                            mv
                        })
                        .collect::<Vec<_>>();
                    if !moves.is_empty() {
                        self.work.push(Work::Move { moves, checked: 0 });
                    }
                    return Vec::new();
                }
                // Each owner's cards to that owner's zone (CR 400.3), in APNAP order.
                let order = st.apnap_order();
                let mut owners: Vec<PlayerId> = objs.iter().map(|&o| st.object(o).owner).collect();
                owners.sort_by_key(|p| order.iter().position(|q| q == p));
                owners.dedup();
                // CR 701.24c: "shuffle it into its owner's library" shuffles that library
                // whatever becomes of the object on the way.
                if shuffle && !owners.is_empty() {
                    self.work.push(Work::Shuffle {
                        players: owners.clone(),
                    });
                }
                for owner in owners.into_iter().rev() {
                    self.work.push(Work::Place {
                        player: owner,
                        by: owner,
                        cards: objs
                            .iter()
                            .copied()
                            .filter(|&o| st.object(o).owner == owner)
                            .collect(),
                        to,
                        random: false,
                        ordered: false,
                    });
                }
                Vec::new()
            }
            Effect::Note(inner) => {
                let only_to = match &*inner {
                    Effect::Dig { to, .. } => Some(card_dest(*to).0),
                    _ => None,
                };
                self.noting = Some(Noting {
                    tokens_from: st.next_card_instance,
                    only_to,
                });
                self.work.push(Work::EndNote);
                self.work.push(Work::Eval { k, effect: inner });
                Vec::new()
            }
            Effect::If { test, then } => {
                if self.holds(st, k, &test) {
                    self.work.push(Work::Eval { k, effect: then });
                }
                Vec::new()
            }
            Effect::MoveAll {
                from,
                whose,
                cards,
                to,
                shuffle,
            } => {
                let players = self.players_apnap(st, k, whose);
                let mut found: Vec<ObjectId> = Vec::new();
                for &p in &players {
                    found.extend(
                        zone_cards(st, from, p)
                            .into_iter()
                            .filter(|&id| self.fits(st, id, &cards)),
                    );
                }
                // Permanents in timestamp order whoever controls them.
                if from == CardZone::Battlefield {
                    found.sort_by_key(|id| st.zones.battlefield.iter().position(|b| b == id));
                }
                if shuffle {
                    self.work.push(Work::Shuffle { players });
                }
                self.work.push(Work::Move {
                    moves: found.into_iter().map(|o| move_to(o, to)).collect(),
                    checked: 0,
                });
                Vec::new()
            }
        }
    }

    /// CR 701.22 / 701.25: the players scry (or surveil) N.
    fn scry(&mut self, st: &GameState, k: usize, who: PlayerRef, amount: &Amount, surveil: bool) {
        let n = self.amount(st, k, amount);
        // CR 701.22b / 701.25c: scry 0 and surveil 0 aren't events.
        if n > 0 {
            self.work.push(Work::Scry {
                players: self.players_apnap(st, k, who),
                n,
                surveil,
                looking: Vec::new(),
            });
        }
    }

    /// CR 608.2n: as the final part of resolution, the spell goes to its owner's graveyard;
    /// an ability ceases to exist. CR 608.2m: if it already left the stack, nothing to do.
    fn finish(self, st: &mut GameState) -> Flow {
        let id = self.object.expect("started");
        if st.stack_object(id).is_none() {
            return Flow::done();
        }
        st.emit(Event::Resolved { spell: id });
        self.leave_stack(st, MoveCause::Resolve)
    }

    /// Take the object off the stack (CR 608.2n, or 608.2b when it fizzles). A spell card
    /// moving to the graveyard is a replaceable event.
    fn leave_stack(&self, st: &mut GameState, cause: MoveCause) -> Flow {
        let id = self.object.expect("started");
        match st.stack_object(id).map(|s| s.kind.clone()) {
            Some(StackKind::Spell) => {
                let mut mv = Proposed::moving(id, Zone::Graveyard, cause);
                // CR 716.3d / 721.3d: an Adventure is exiled, an Omen shuffled into its
                // owner's library, instead of going to the graveyard as it resolves.
                let o = st.object(id);
                let layout = st.catalog.get(o.card).layout;
                if cause == MoveCause::Resolve
                    && o.face != 0
                    && self.spell_to.is_none()
                    && let Proposed::Move { to, shuffle, .. } = &mut mv
                {
                    match layout {
                        card_model::Layout::Adventure => {
                            *to = Zone::Exile;
                            let player = st.stack_object(id).expect("on the stack").controller;
                            execute(st, GameAction::SendOnAdventure { spell: id, player });
                        }
                        card_model::Layout::Omen => (*to, *shuffle) = (Zone::Library, true),
                        _ => {}
                    }
                }
                // CR 608.2n: a spell that resolved goes to its owner's graveyard, unless
                // its own effect says where it goes.
                if cause == MoveCause::Resolve
                    && let Some((dest, then_shuffle)) = self.spell_to
                    && let Proposed::Move {
                        to,
                        position,
                        shuffle,
                        ..
                    } = &mut mv
                {
                    let (zone, at, _) = card_dest(dest);
                    (*to, *position, *shuffle) = (zone, at, then_shuffle);
                }
                Flow::then(Frame::Apply(ApplyFrame::new(st, vec![mv])))
            }
            Some(StackKind::Ability { .. }) => {
                execute(st, GameAction::RemoveAbility { ability: id });
                Flow::done()
            }
            None => Flow::done(),
        }
    }
}

/// The cards picked in a `PickN` over cards.
fn picked_cards(inp: &Input) -> Vec<ObjectId> {
    inp.picked()
        .into_iter()
        .map(|o| match o {
            OptionDesc::Card { card } => *card,
            o => unreachable!("not a card: {o:?}"),
        })
        .collect()
}

/// The top `n` cards of `player`'s library (all of it, if smaller), top card first.
fn top_cards(st: &GameState, player: PlayerId, n: u32) -> Vec<ObjectId> {
    let library = st.zones.list(Zone::Library, player);
    let take = (n as usize).min(library.len());
    library[library.len() - take..]
        .iter()
        .rev()
        .copied()
        .collect()
}

/// `player`'s cards in a zone: what they own there, or the permanents they control. In
/// the zone's own order.
fn zone_cards(st: &GameState, zone: CardZone, player: PlayerId) -> Vec<ObjectId> {
    match zone {
        CardZone::Library => st.zones.list(Zone::Library, player).clone(),
        CardZone::Hand => st.zones.list(Zone::Hand, player).clone(),
        CardZone::Graveyard => st.zones.list(Zone::Graveyard, player).clone(),
        CardZone::Battlefield => st
            .zones
            .battlefield
            .iter()
            .copied()
            .filter(|&id| crate::layers::controller_of(st, id) == player)
            .collect(),
    }
}

/// Does the card or permanent match any of the selectors (no selectors: anything does)?
/// Outside the battlefield a card has its printed characteristics and its owner stands in
/// for its controller.
fn card_fits(
    st: &GameState,
    id: ObjectId,
    selectors: &[card_model::Selector],
    source: ObjectId,
    controller: PlayerId,
) -> bool {
    if selectors.is_empty() {
        return true;
    }
    let c = chars(st, id);
    selectors
        .iter()
        .any(|s| crate::layers::matches(st, s, &c, st.object(id), source, controller))
}

/// A destination as (zone, position in a library, enters tapped).
fn card_dest(to: CardDest) -> (Zone, crate::zones::LibraryPosition, bool) {
    use crate::zones::LibraryPosition;
    match to {
        CardDest::Hand => (Zone::Hand, LibraryPosition::Top, false),
        CardDest::Battlefield { tapped } => (Zone::Battlefield, LibraryPosition::Top, tapped),
        CardDest::Graveyard => (Zone::Graveyard, LibraryPosition::Top, false),
        CardDest::Exile => (Zone::Exile, LibraryPosition::Top, false),
        CardDest::LibraryTop => (Zone::Library, LibraryPosition::Top, false),
        CardDest::LibraryBottom => (Zone::Library, LibraryPosition::Bottom, false),
        // Validated to occur only where nothing is moved (a search that leaves its cards).
        CardDest::Stay => unreachable!("cards that stay aren't moved"),
    }
}

/// The engine's zone for a zone an effect takes cards from.
fn card_zone(z: CardZone) -> Zone {
    match z {
        CardZone::Library => Zone::Library,
        CardZone::Hand => Zone::Hand,
        CardZone::Graveyard => Zone::Graveyard,
        CardZone::Battlefield => Zone::Battlefield,
    }
}

/// A choice among "those cards" only: the cards were found or seen before.
fn among_noted(spec: &ChooseSpec) -> bool {
    !spec.cards.is_empty() && spec.cards.iter().all(|s| s.noted == Some(true))
}

/// The proposed move of `object` to a destination.
fn move_to(object: ObjectId, to: CardDest) -> Proposed {
    let (zone, at, enters_tapped) = card_dest(to);
    let mut mv = Proposed::moving(object, zone, MoveCause::Rule);
    if let Proposed::Move {
        position, tapped, ..
    } = &mut mv
    {
        *position = at;
        *tapped = enters_tapped;
    }
    mv
}

/// CR 303.4f: if `mv` puts an Aura card onto the battlefield without saying what it
/// enchants, the player who will control it and what it could legally enchant.
fn aura_hosts(st: &GameState, mv: &Proposed) -> Option<(PlayerId, Vec<ObjectId>)> {
    let Proposed::Move {
        object,
        to: Zone::Battlefield,
        controller,
        attach_to: None,
        ..
    } = mv
    else {
        return None;
    };
    let o = st.objects.get(object)?;
    if matches!(o.zone, Zone::Battlefield | Zone::Stack) {
        return None;
    }
    let filter = st.card_def(*object).enchant()?;
    let chooser = controller.unwrap_or(o.owner);
    let hosts = st
        .zones
        .battlefield
        .iter()
        .copied()
        .filter(|&id| {
            crate::targeting::describes(st, &filter, Target::Object(id), chooser, None, None)
        })
        .collect();
    Some((chooser, hosts))
}

/// The decision kind, count, and choosable objects for one player's part of an
/// each-player choice.
fn each_player_options(
    st: &GameState,
    choice: &PlayerChoice,
    p: PlayerId,
    source: ObjectId,
    controller: PlayerId,
) -> (ChoiceKind, Vec<ObjectId>) {
    match choice {
        PlayerChoice::Discard => (ChoiceKind::Discard, st.zones.hands[p.index()].clone()),
        PlayerChoice::Sacrifice(sel) => (
            ChoiceKind::Sacrifice,
            // CR 701.21a: only permanents they control.
            st.zones
                .battlefield
                .iter()
                .copied()
                .filter(|&id| {
                    let c = chars(st, id);
                    c.controller == p
                        && crate::layers::matches(st, sel, &c, st.object(id), source, controller)
                })
                .collect(),
        ),
    }
}

/// CR 701.27d–e, 712.7: only a permanent represented by a double-faced card can transform,
/// and not into an instant or sorcery face.
fn can_transform(st: &GameState, object: ObjectId) -> bool {
    let o = st.object(object);
    let card = st.catalog.get(o.card);
    if o.zone != Zone::Battlefield || !card.layout.double_faced() || card.faces.is_empty() {
        return false;
    }
    let other = st
        .catalog
        .get(st.catalog.face(o.card, if o.face == 0 { 1 } else { 0 }));
    !other.has_type(card_model::CardType::Instant) && !other.has_type(card_model::CardType::Sorcery)
}

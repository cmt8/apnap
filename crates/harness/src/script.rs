//! A small driver for scripted rules tests: answer decisions by describing the option you
//! want ("cast Test Shock", "target P1", "pass") instead of by index. Panics with the list
//! of available options when the requested one isn't there, which makes test failures
//! readable.

use engine_core::stack::Target;
use engine_core::zones::Zone;
use engine_core::{
    ChoiceKind, Event, GameState, ObjectId, OptionDesc, PendingDecision, PlayerId, Response,
};

#[derive(Debug)]
pub struct Script {
    pub st: GameState,
    /// Every event since the script started.
    pub events: Vec<Event>,
    pub next: Option<PendingDecision>,
    /// The decisions answered so far, in order (for APNAP-order assertions).
    pub answered: Vec<PendingDecision>,
}

impl Script {
    pub fn new(mut st: GameState) -> Self {
        let out = engine_core::start(&mut st);
        Script {
            st,
            events: out.events,
            next: out.next,
            answered: Vec::new(),
        }
    }

    pub fn decision(&self) -> &PendingDecision {
        self.next.as_ref().expect("game is over")
    }

    pub fn who(&self) -> PlayerId {
        self.decision().player
    }

    pub fn kind(&self) -> ChoiceKind {
        self.decision().kind
    }

    pub fn options(&self) -> &[OptionDesc] {
        &self.decision().options
    }

    /// Name of a card object or stack object.
    pub fn name(&self, id: ObjectId) -> String {
        if self.st.objects.contains_key(&id) {
            return self.st.card_def(id).name.clone();
        }
        if let Some(so) = self.st.stack_object(id)
            && let engine_core::stack::StackKind::Ability { source_card, .. } = so.kind
        {
            return format!("ability of {}", self.st.catalog.get(source_card).name);
        }
        format!("#{}", id.0)
    }

    /// The first object named `name` owned by `owner` in `zone` (stack: any spell).
    pub fn find(&self, owner: PlayerId, zone: Zone, name: &str) -> Option<ObjectId> {
        self.st
            .objects
            .values()
            .find(|o| o.owner == owner && o.zone == zone && self.st.card_def(o.id).name == name)
            .map(|o| o.id)
    }

    pub fn respond(&mut self, r: Response) -> &mut Self {
        let d = self.next.clone().expect("game is over");
        let out = engine_core::step(&mut self.st, r.clone())
            .unwrap_or_else(|e| panic!("{r:?} rejected for {d:?}: {e}"));
        self.answered.push(d);
        self.events.extend(out.events);
        self.next = out.next;
        self
    }

    pub fn has(&self, pred: impl Fn(&Script, &OptionDesc) -> bool) -> bool {
        self.next
            .as_ref()
            .is_some_and(|d| d.options.iter().any(|o| pred(self, o)))
    }

    /// Pick the single option matching `pred`.
    pub fn pick(&mut self, what: &str, pred: impl Fn(&Script, &OptionDesc) -> bool) -> &mut Self {
        let d = self.decision();
        let Some(i) = d.options.iter().position(|o| pred(self, o)) else {
            let opts: Vec<String> = d.options.iter().map(|o| self.describe(o)).collect();
            panic!(
                "no option '{what}' for {:?} ({:?}); options: {opts:?}",
                d.player, d.kind
            );
        };
        self.respond(Response::Pick(vec![i]))
    }

    pub fn describe(&self, o: &OptionDesc) -> String {
        match o {
            OptionDesc::CastSpell { card } => format!("cast {}", self.name(*card)),
            OptionDesc::PlayLand { card } => format!("play {}", self.name(*card)),
            OptionDesc::ActivateAbility { source, .. } => {
                format!("activate {}", self.name(*source))
            }
            OptionDesc::TapForMana { source, mana } => {
                format!("tap {} for {mana:?}", self.name(*source))
            }
            OptionDesc::Target {
                target: Target::Object(id),
            } => format!("target {}", self.name(*id)),
            OptionDesc::Card { card } => self.name(*card),
            other => format!("{other:?}"),
        }
    }

    pub fn pass(&mut self) -> &mut Self {
        assert_eq!(self.kind(), ChoiceKind::Priority, "pass outside priority");
        self.pick("pass", |_, o| matches!(o, OptionDesc::Pass))
    }

    /// Every player still in the game passes once, in turn order (stops early if a
    /// non-priority decision comes up).
    pub fn pass_all(&mut self) -> &mut Self {
        for _ in 0..self.st.players_in_game() {
            if self
                .next
                .as_ref()
                .is_none_or(|d| d.kind != ChoiceKind::Priority)
            {
                break;
            }
            self.pass();
        }
        self
    }

    pub fn cast(&mut self, name: &str) -> &mut Self {
        self.pick(
            &format!("cast {name}"),
            |s, o| matches!(o, OptionDesc::CastSpell { card } if s.name(*card) == name),
        )
    }

    pub fn can_cast(&self, name: &str) -> bool {
        self.has(|s, o| matches!(o, OptionDesc::CastSpell { card } if s.name(*card) == name))
    }

    pub fn activate(&mut self, name: &str) -> &mut Self {
        self.pick(&format!("activate {name}"), |s, o| {
            matches!(o, OptionDesc::ActivateAbility { source, .. } if s.name(*source) == name)
        })
    }

    pub fn tap(&mut self, name: &str, mana: engine_core::mana::ManaType) -> &mut Self {
        self.pick(&format!("tap {name} for {mana:?}"), |s, o| {
            matches!(o, OptionDesc::TapForMana { source, mana: m } if s.name(*source) == name && *m == mana)
        })
    }

    /// Answer every mana-payment pick with the first offered source until the cost is paid.
    pub fn pay(&mut self) -> &mut Self {
        while self.next.as_ref().is_some_and(|d| {
            d.kind == ChoiceKind::PayMana && matches!(d.prim, engine_core::Primitive::PickN { .. })
        }) {
            self.respond(Response::Pick(vec![0]));
        }
        self
    }

    pub fn target_player(&mut self, p: PlayerId) -> &mut Self {
        self.pick(
            &format!("target {p:?}"),
            |_, o| matches!(o, OptionDesc::Target { target: Target::Player(q) } if *q == p),
        )
    }

    pub fn target(&mut self, name: &str) -> &mut Self {
        self.pick(&format!("target {name}"), |s, o| {
            matches!(o, OptionDesc::Target { target: Target::Object(id) } if s.name(*id) == name)
        })
    }

    pub fn target_object(&mut self, id: ObjectId) -> &mut Self {
        self.pick(
            &format!("target {id:?}"),
            |_, o| matches!(o, OptionDesc::Target { target: Target::Object(t) } if *t == id),
        )
    }

    pub fn mode(&mut self, index: u8) -> &mut Self {
        self.pick(
            &format!("mode {index}"),
            |_, o| matches!(o, OptionDesc::Mode { index: i } if *i == index),
        )
    }

    pub fn x(&mut self, x: u32) -> &mut Self {
        assert_eq!(self.kind(), ChoiceKind::ChooseX);
        self.respond(Response::Number(x))
    }

    pub fn count(&self, pred: impl Fn(&Event) -> bool) -> usize {
        self.events.iter().filter(|e| pred(e)).count()
    }
}

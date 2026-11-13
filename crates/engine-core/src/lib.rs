//! apnap engine-core: a pure, deterministic Magic: The Gathering rules engine.
//!
//! `#![no_std]` + `alloc` by design (docs/decisions.md D-003): the compiler itself forbids
//! I/O, threads, wall-clock time and `HashMap`. The only randomness is `GameState::rng`.
//!
//! Drive a game with [`new_game`] → [`start`] → repeatedly [`step`] with a [`Response`]
//! to the returned [`PendingDecision`], until `StepOutput::next` is `None`.
//!
//! # Host and client (D-092)
//!
//! The **host** owns the [`GameState`]: it calls [`start`] and [`step`], and turns what they
//! return into what each seat may know. A **client** (a UI, an AI agent, a network peer, a
//! narrator) never sees the `GameState`. Its whole input is:
//!
//! 1. the card catalog ([`card_model::CardCatalog`]): shared and read-only, card text only;
//! 2. its view, [`seat_view`] for its [`client::Seat`] (a player: [`player_view`]): what it
//!    may see now — the zones, life, mana, the stack, combat, the turn, step and active
//!    player, who the game waits for (`waiting`), the decision counter, the result;
//! 3. the [`PendingDecision`] when it is its turn to decide (also `PlayerView::decision`);
//! 4. its version of each step's events: [`client::EventView::redact`] of
//!    `StepOutput::truth`, which is engine truth with hidden information and never goes to
//!    a client.
//!
//! It answers with a [`Response`]. All four (and the response) are plain data that
//! serialize and round-trip (`crates/harness/tests/client_boundary.rs` plays whole games
//! with clients that get only their JSON). Anything a client would need beyond them is a
//! missing view field or event, not a reason to hand it the state. The reference clients
//! keep to this, checked by `cargo xtask check-deps`: the app's `ui.rs`, `cardview.rs`,
//! `combat.rs` and `art.rs`, the narrator `harness::text`, and the `ai` crate never name
//! `GameState`; the host side is the app's `session.rs`, the harness driver and tests. (A
//! search AI's `Determinizer` (spec, M5) will be the one documented exception: a host-made
//! sample of the hidden zones consistent with the agent's view, never the real state.)
#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod actions;
pub mod agent;
pub mod chosen;
pub mod client;
pub mod config;
pub mod copy;
pub mod decision;
pub mod effects;
mod engine;
pub mod events;
pub mod frames;
pub mod hash;
pub mod ids;
pub mod layers;
pub mod mana;
pub mod objects;
pub mod players;
pub mod replacement;
pub mod rng;
pub mod sagas;
mod sba;
pub mod stack;
pub mod state;
pub mod targeting;
#[cfg(feature = "test-support")]
pub mod testing;
pub mod triggers;
pub mod turn;
pub mod view;
pub mod zones;

pub use agent::Agent;
pub use config::{Format, GameConfig};
pub use decision::{ChoiceKind, IllegalResponse, OptionDesc, PendingDecision, Primitive, Response};
pub use engine::{SetupError, StepOutput, new_game, new_game_with_commanders, start, step};
pub use events::Event;
pub use ids::{CardInstanceId, ObjectId, PlayerId, Timestamp};
pub use state::GameState;
pub use view::{PlayerView, player_view, seat_view};

/// Rules version this engine implements; cite as `// CR x.y (2026-11-06)`.
pub const CR_VERSION: &str = "2026-11-06";

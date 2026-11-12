//! apnap engine-core: a pure, deterministic Magic: The Gathering rules engine.
//!
//! `#![no_std]` + `alloc` by design (docs/decisions.md D-003): the compiler itself forbids
//! I/O, threads, wall-clock time and `HashMap`. The only randomness is `GameState::rng`.
//!
//! Drive a game with [`new_game`] → [`start`] → repeatedly [`step`] with a [`Response`]
//! to the returned [`PendingDecision`], until `StepOutput::next` is `None`.
#![no_std]

extern crate alloc;
#[cfg(test)]
extern crate std;

pub mod actions;
pub mod agent;
pub mod config;
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
pub use view::{PlayerView, player_view};

/// Rules version this engine implements; cite as `// CR x.y (2026-11-06)`.
pub const CR_VERSION: &str = "2026-11-06";

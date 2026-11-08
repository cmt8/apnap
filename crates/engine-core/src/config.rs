//! Game configuration: format and the numbers that vary by format (docs/decisions.md D-004).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    /// Traditional 60-card constructed (1v1).
    Constructed,
    /// Commander (CR 903). Command zone, tax and commander damage arrive in M3.
    Commander,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameConfig {
    pub format: Format,
    pub num_players: u8,
    /// CR 103.4 (2026-11-06): 20 normally; 40 in Commander (CR 103.4c).
    pub starting_life: i32,
    /// CR 103.5 (2026-11-06): normally seven.
    pub starting_hand_size: u32,
    /// CR 402.2 (2026-11-06): normally seven.
    pub max_hand_size: u32,
}

impl GameConfig {
    pub fn constructed_1v1() -> Self {
        GameConfig {
            format: Format::Constructed,
            num_players: 2,
            starting_life: 20,
            starting_hand_size: 7,
            max_hand_size: 7,
        }
    }

    pub fn commander(num_players: u8) -> Self {
        GameConfig {
            format: Format::Commander,
            num_players,
            starting_life: 40,
            starting_hand_size: 7,
            max_hand_size: 7,
        }
    }

    /// "Multiplayer" in the CR sense: more than two players (CR 103.5c, 103.8).
    pub fn is_multiplayer(&self) -> bool {
        self.num_players > 2
    }
}

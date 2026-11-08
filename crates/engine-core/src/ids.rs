//! Dense, monotonic, deterministic identifiers (spec §5, CLAUDE.md invariant 5).

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident($t:ty)) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub $t);
    };
}

id_type!(
    /// A game object. A new id is allocated on every zone change (CR 400.7 (2026-11-06)).
    ObjectId(u32)
);
id_type!(
    /// Stable identity of a physical card across zone changes (commander tracking, CR 903).
    CardInstanceId(u32)
);
id_type!(
    /// Seat index, `0..num_players`.
    PlayerId(u8)
);
id_type!(
    /// Global ordering counter (CR 613.7 (2026-11-06)).
    Timestamp(u64)
);

impl PlayerId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

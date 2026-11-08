//! The game's ONLY source of randomness (CLAUDE.md invariant 1).
//!
//! ChaCha8 is a portable, specified stream. We deliberately use only `next_u64` from it and
//! implement seeding, bounded sampling and shuffling ourselves, because `rand`'s
//! higher-level algorithms (`seed_from_u64`, `gen_range`, `shuffle`) are not guaranteed to
//! be value-stable across crate versions (docs/decisions.md D-006). Serialization is also
//! ours (seed + stream + word position), independent of rand_chacha's serde format, and it
//! keeps serde's `std` feature out of the engine (rand_chacha's `serde` feature enables it).

use rand_chacha::ChaCha8Rng;
use rand_core::{Rng, SeedableRng};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameRng(ChaCha8Rng);

/// SplitMix64 (Steele, Lea & Flood 2014): expands a `u64` seed into well-mixed words.
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl GameRng {
    pub fn from_seed_u64(seed: u64) -> Self {
        let mut s = seed;
        let mut key = [0u8; 32];
        for chunk in key.as_chunks_mut::<8>().0 {
            *chunk = splitmix64(&mut s).to_le_bytes();
        }
        GameRng(ChaCha8Rng::from_seed(key))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// Uniform integer in `0..n` without modulo bias (rejection sampling). Panics if `n == 0`.
    pub fn below(&mut self, n: u64) -> u64 {
        assert!(n > 0, "GameRng::below(0)");
        // Reject the lowest (2^64 mod n) values so the accepted range is a multiple of n.
        let threshold = n.wrapping_neg() % n;
        loop {
            let v = self.next_u64();
            if v >= threshold {
                return v % n;
            }
        }
    }

    /// Fisher–Yates (Durstenfeld) shuffle.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            items.swap(i, j);
        }
    }
}

#[derive(Serialize, Deserialize)]
struct RngRepr {
    seed: [u8; 32],
    stream: u64,
    word_pos_hi: u64,
    word_pos_lo: u64,
}

impl Serialize for GameRng {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let pos = self.0.get_word_pos();
        RngRepr {
            seed: self.0.get_seed(),
            stream: self.0.get_stream(),
            word_pos_hi: (pos >> 64) as u64,
            word_pos_lo: pos as u64,
        }
        .serialize(s)
    }
}

impl<'de> Deserialize<'de> for GameRng {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = RngRepr::deserialize(d)?;
        let mut rng = ChaCha8Rng::from_seed(r.seed);
        rng.set_stream(r.stream);
        rng.set_word_pos((u128::from(r.word_pos_hi) << 64) | u128::from(r.word_pos_lo));
        Ok(GameRng(rng))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn serde_roundtrip_preserves_stream_position() {
        let mut a = GameRng::from_seed_u64(7);
        for _ in 0..13 {
            a.next_u64();
        }
        let _ = a.0.next_u32(); // odd word offset
        let json = serde_json::to_string(&a).unwrap();
        let mut b: GameRng = serde_json::from_str(&json).unwrap();
        assert_eq!(a, b);
        let xs: Vec<u64> = (0..50).map(|_| a.next_u64()).collect();
        let ys: Vec<u64> = (0..50).map(|_| b.next_u64()).collect();
        assert_eq!(xs, ys);
    }

    #[test]
    fn below_is_in_range_and_shuffle_is_permutation() {
        let mut r = GameRng::from_seed_u64(1);
        for n in 1..40u64 {
            for _ in 0..20 {
                assert!(r.below(n) < n);
            }
        }
        let mut v: Vec<u32> = (0..60).collect();
        r.shuffle(&mut v);
        let mut s = v.clone();
        s.sort_unstable();
        assert_eq!(s, (0..60).collect::<Vec<_>>());
        assert_ne!(v, s, "60-card shuffle left the deck sorted");
    }

    /// Pins the stream so an accidental algorithm change (ours or rand_chacha's) fails loudly.
    #[test]
    fn stream_is_value_stable() {
        let mut r = GameRng::from_seed_u64(42);
        let got = [r.next_u64(), r.next_u64(), r.below(1000)];
        assert_eq!(
            got, PINNED_SEED_42,
            "RNG stream changed: replays would break"
        );
    }

    const PINNED_SEED_42: [u64; 3] = [3536907876931541756, 1681417456739323905, 207];
}

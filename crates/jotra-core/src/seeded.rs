//! A seeded stream of numbers (splitmix64) for the property tests, so a
//! failing case is named by the seed that reproduces it and no random-number
//! dependency is needed.

/// Splitmix64 over a 64-bit state, advanced by every draw.
pub(crate) struct Seeded(u64);

impl Seeded {
    /// A stream that starts from `seed`, so the same seed replays the same
    /// cases.
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..bound`.
    pub(crate) fn below(&mut self, bound: u32) -> u32 {
        (self.next_u64() % u64::from(bound)) as u32
    }

    pub(crate) fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len() as u32) as usize]
    }

    /// A character in `low..=high`, drawn again when the draw lands on a
    /// surrogate, which is not a character.
    pub(crate) fn char_in(&mut self, low: u32, high: u32) -> char {
        loop {
            if let Some(c) = char::from_u32(low + self.below(high - low + 1)) {
                return c;
            }
        }
    }
}

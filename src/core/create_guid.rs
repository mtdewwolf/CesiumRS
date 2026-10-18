//! CesiumJS `createGuid`: a random version-4 UUID string.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

/// Random 64 bits from the standard library's per-process hash keys and a counter.
fn random_u64() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.finish()
}

pub fn create_guid() -> String {
    let template = "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx";
    let mut bits = random_u64() as u128 | ((random_u64() as u128) << 64);
    template
        .chars()
        .map(|c| match c {
            'x' | 'y' => {
                let r = (bits & 0xf) as u32;
                bits >>= 4;
                let v = if c == 'x' { r } else { (r & 0x3) | 0x8 };
                char::from_digit(v, 16).unwrap()
            }
            other => other,
        })
        .collect()
}

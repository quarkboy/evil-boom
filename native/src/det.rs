//! Hash maps and sets that go round in the same order every run. The standard ones are seeded
//! at random in each process, and where a choice fell to the order they went round in (a tie),
//! the same map played the same way came out differently from one run to the next (MAP17: two
//! runs of three exited, the third got stuck). Same hashing, fixed keys.

use std::collections::hash_map::DefaultHasher;
use std::hash::BuildHasherDefault;

pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<DefaultHasher>>;
pub type HashSet<T> = std::collections::HashSet<T, BuildHasherDefault<DefaultHasher>>;

//! The solutions `solver serve` holds loaded between requests, under a memory budget: each one
//! counted at its estimated size, and the least recently used dropped first to make room for
//! another. A value is handed out behind an [`Arc`], so that one dropped from the cache while a
//! request still reads it is freed when that request is done.

use std::sync::Arc;

/// Values by name, each counted at a size in bytes, the sizes held to a budget.
#[derive(Debug)]
pub struct Cache<T> {
    budget: u64,
    entries: Vec<Entry<T>>,
    /// Counts the uses, so that the entry used least recently has the lowest `used`.
    clock: u64,
}

#[derive(Debug)]
struct Entry<T> {
    name: String,
    value: Arc<T>,
    bytes: u64,
    used: u64,
}

/// Why a value was not made room for: it is larger than the whole budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLarge {
    pub bytes: u64,
    pub budget: u64,
}

impl<T> Cache<T> {
    /// An empty cache that holds at most `budget` bytes.
    pub fn new(budget: u64) -> Cache<T> {
        Cache {
            budget,
            entries: Vec::new(),
            clock: 0,
        }
    }

    pub fn budget(&self) -> u64 {
        self.budget
    }

    /// The bytes the cache holds: the sizes of its values, summed.
    pub fn held(&self) -> u64 {
        self.entries.iter().map(|entry| entry.bytes).sum()
    }

    /// The value held under `name`, which counts as a use of it.
    pub fn get(&mut self, name: &str) -> Option<Arc<T>> {
        self.clock += 1;
        let clock = self.clock;
        let entry = self.entries.iter_mut().find(|entry| entry.name == name)?;
        entry.used = clock;
        Some(Arc::clone(&entry.value))
    }

    /// Drops the value held under `name`, if there is one.
    pub fn remove(&mut self, name: &str) {
        self.entries.retain(|entry| entry.name != name);
    }

    /// Drops every value.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Drops the least recently used values until `bytes` more fit in the budget, and gives
    /// their names in the order dropped. Refuses, dropping nothing, if `bytes` is over the whole
    /// budget.
    pub fn make_room(&mut self, bytes: u64) -> Result<Vec<String>, TooLarge> {
        if bytes > self.budget {
            return Err(TooLarge {
                bytes,
                budget: self.budget,
            });
        }
        let mut dropped = Vec::new();
        while self.held() + bytes > self.budget {
            let oldest = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(index, _)| index)
                .expect("a cache over its budget holds a value");
            dropped.push(self.entries.remove(oldest).name);
        }
        Ok(dropped)
    }

    /// Holds `value` under `name`, counted as `bytes`, in place of any value held under that
    /// name, after making room for it as [`Cache::make_room`] does; gives the value and the
    /// names dropped. Refuses a value over the whole budget, holding nothing new.
    pub fn insert(
        &mut self,
        name: &str,
        value: T,
        bytes: u64,
    ) -> Result<(Arc<T>, Vec<String>), TooLarge> {
        self.remove(name);
        let dropped = self.make_room(bytes)?;
        self.clock += 1;
        let value = Arc::new(value);
        self.entries.push(Entry {
            name: name.to_string(),
            value: Arc::clone(&value),
            bytes,
            used: self.clock,
        });
        Ok((value, dropped))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_least_recently_used_value_is_dropped_first() {
        let mut cache = Cache::new(100);
        assert!(cache.insert("a", 'a', 40).unwrap().1.is_empty());
        assert!(cache.insert("b", 'b', 40).unwrap().1.is_empty());
        assert_eq!(cache.held(), 80);
        // Using `a` makes `b` the least recently used.
        assert_eq!(cache.get("a").as_deref(), Some(&'a'));
        assert_eq!(cache.insert("c", 'c', 40).unwrap().1, ["b"]);
        assert_eq!(cache.get("b"), None);
        assert_eq!(cache.held(), 80);
        // A large value drops as many as it needs, oldest first.
        assert_eq!(cache.insert("d", 'd', 90).unwrap().1, ["a", "c"]);
        assert_eq!(cache.held(), 90);
        assert_eq!(cache.get("d").as_deref(), Some(&'d'));
    }

    #[test]
    fn a_value_over_the_budget_is_refused_and_drops_nothing() {
        let mut cache = Cache::new(100);
        cache.insert("a", 1, 60).unwrap();
        assert_eq!(
            cache.make_room(101),
            Err(TooLarge {
                bytes: 101,
                budget: 100
            })
        );
        assert_eq!(cache.held(), 60);
        assert!(cache.insert("b", 2, 101).is_err());
        assert_eq!(cache.get("a").as_deref(), Some(&1));
        assert_eq!(cache.get("b"), None);
        // A value that fits the budget exactly is held alone.
        assert_eq!(cache.insert("c", 3, 100).unwrap().1, ["a"]);
    }

    #[test]
    fn a_value_held_again_replaces_the_old_one() {
        let mut cache = Cache::new(100);
        cache.insert("a", 1, 30).unwrap();
        let (held, dropped) = cache.insert("a", 2, 90).unwrap();
        assert!(dropped.is_empty(), "the old value is not counted");
        assert_eq!(*held, 2);
        assert_eq!(cache.held(), 90);
        // A value handed out outlives its place in the cache.
        cache.clear();
        assert_eq!(*held, 2);
        assert_eq!(cache.held(), 0);
        assert_eq!(cache.budget(), 100);
    }
}

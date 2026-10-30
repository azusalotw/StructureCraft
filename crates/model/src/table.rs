//! Generational tables: dense rows addressed by ids that go stale when their row is removed.
//!
//! Rows sit in one `Vec` (no per-element allocation) and a removed slot is reused, so iterating
//! touches contiguous memory and an id is two `u32`s. A slot's generation is odd while it holds a
//! row and even while it is free; removing bumps it, so an old id never reaches a new row.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// The id of a row in a [`Table<T>`].
pub struct Id<T> {
    index: u32,
    generation: u32,
    kind: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation, kind: PhantomData }
    }

    pub const fn index(self) -> u32 {
        self.index
    }

    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// An id that names no row (generation 0 is never live), for the fields of free slots.
impl<T> Default for Id<T> {
    fn default() -> Self {
        Self::new(u32::MAX, 0)
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for Id<T> {}
impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}
impl<T> Eq for Id<T> {}
impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (u64::from(self.generation) << 32 | u64::from(self.index)).hash(state);
    }
}
impl<T> PartialOrd for Id<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<T> Ord for Id<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.index, self.generation).cmp(&(other.index, other.generation))
    }
}
impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.index, self.generation)
    }
}

/// Why a table refused a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotError {
    /// The id names no live row.
    Missing(u32),
    /// The slot is taken or was reused since the id was handed out.
    Occupied(u32),
    /// The table already holds `u32::MAX` slots.
    Full,
}

#[derive(Clone)]
pub struct Table<T> {
    rows: Vec<T>,
    generations: Vec<u32>,
    free: Vec<u32>,
    live: usize,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self { rows: Vec::new(), generations: Vec::new(), free: Vec::new(), live: 0 }
    }
}

impl<T: fmt::Debug> fmt::Debug for Table<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<T: PartialEq> PartialEq for Table<T> {
    fn eq(&self, other: &Self) -> bool {
        self.live == other.live && self.iter().all(|(id, row)| other.get(id) == Some(row))
    }
}

const fn is_live(generation: u32) -> bool {
    generation & 1 == 1
}

impl<T> Table<T> {
    /// Live rows.
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// Slots, live or free: ids index below this.
    pub fn capacity(&self) -> usize {
        self.rows.len()
    }

    pub fn reserve(&mut self, additional: usize) {
        let grow = additional.saturating_sub(self.free.len());
        self.rows.reserve(grow);
        self.generations.reserve(grow);
    }

    pub fn contains(&self, id: Id<T>) -> bool {
        self.generations.get(id.index as usize) == Some(&id.generation) && is_live(id.generation)
    }

    pub fn get(&self, id: Id<T>) -> Option<&T> {
        if self.contains(id) { self.rows.get(id.index as usize) } else { None }
    }

    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        if self.contains(id) { self.rows.get_mut(id.index as usize) } else { None }
    }

    /// The live id at a slot index, if any.
    pub fn id_at(&self, index: u32) -> Option<Id<T>> {
        self.generations.get(index as usize).filter(|g| is_live(**g)).map(|g| Id::new(index, *g))
    }

    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)> + '_ {
        self.generations.iter().zip(&self.rows).enumerate().filter(|(_, (g, _))| is_live(**g)).map(|(i, (g, row))| (Id::new(i as u32, *g), row))
    }

    pub fn ids(&self) -> impl Iterator<Item = Id<T>> + '_ {
        self.iter().map(|(id, _)| id)
    }

    /// Every slot's generation and row, live or free, in index order (for files).
    pub fn slots(&self) -> impl Iterator<Item = (u32, &T)> + '_ {
        self.generations.iter().copied().zip(&self.rows)
    }
}

impl<T: Default> Table<T> {
    pub fn insert(&mut self, row: T) -> Result<Id<T>, SlotError> {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                let index = u32::try_from(self.rows.len()).map_err(|_| SlotError::Full)?;
                if index == u32::MAX {
                    return Err(SlotError::Full);
                }
                self.rows.push(T::default());
                self.generations.push(0);
                index
            }
        };
        let i = index as usize;
        let (Some(slot), Some(generation)) = (self.rows.get_mut(i), self.generations.get_mut(i)) else {
            return Err(SlotError::Full);
        };
        *slot = row;
        *generation += 1;
        self.live += 1;
        Ok(Id::new(index, *generation))
    }

    pub fn remove(&mut self, id: Id<T>) -> Result<T, SlotError> {
        if !self.contains(id) {
            return Err(SlotError::Missing(id.index));
        }
        let i = id.index as usize;
        let (Some(slot), Some(generation)) = (self.rows.get_mut(i), self.generations.get_mut(i)) else {
            return Err(SlotError::Missing(id.index));
        };
        *generation += 1;
        self.live -= 1;
        // A slot whose generation would wrap is retired, never reused.
        if *generation < u32::MAX - 1 {
            self.free.push(id.index);
        }
        Ok(std::mem::take(slot))
    }

    /// Whether [`restore`](Self::restore) would accept `id`: its slot is free, and either
    /// freed exactly once since `id` was handed out or never used.
    pub fn can_restore(&self, id: Id<T>) -> bool {
        if !is_live(id.generation) {
            return false;
        }
        match self.generations.get(id.index as usize) {
            Some(g) => *g == 0 || Some(*g) == id.generation.checked_add(1),
            None => true,
        }
    }

    /// Put a row back under the id it had (undo of a remove, or loading a file).
    pub fn restore(&mut self, id: Id<T>, row: T) -> Result<(), SlotError> {
        if !self.can_restore(id) {
            return Err(SlotError::Occupied(id.index));
        }
        let i = id.index as usize;
        if i >= self.rows.len() {
            if id.index == u32::MAX {
                return Err(SlotError::Full);
            }
            let first_new = self.rows.len() as u32;
            self.rows.resize_with(i + 1, T::default);
            self.generations.resize(i + 1, 0);
            self.free.extend(first_new..id.index);
        } else if let Some(at) = self.free.iter().rposition(|f| *f == id.index) {
            self.free.swap_remove(at);
        }
        let (Some(slot), Some(generation)) = (self.rows.get_mut(i), self.generations.get_mut(i)) else {
            return Err(SlotError::Full);
        };
        *slot = row;
        *generation = id.generation;
        self.live += 1;
        Ok(())
    }

    /// A table from its slots as [`slots`](Self::slots) lists them.
    pub fn from_slots(slots: impl IntoIterator<Item = (u32, T)>) -> Self {
        let mut table = Self::default();
        for (index, (generation, row)) in slots.into_iter().enumerate() {
            let Ok(index) = u32::try_from(index) else { break };
            table.generations.push(generation);
            table.rows.push(if is_live(generation) { row } else { T::default() });
            if is_live(generation) {
                table.live += 1;
            } else if generation < u32::MAX - 1 {
                table.free.push(index);
            }
        }
        // Reuse low slots first, as a table built by inserts would.
        table.free.reverse();
        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_ids_never_reach_new_rows() {
        let mut t = Table::default();
        let a = t.insert(1).unwrap();
        assert_eq!(t.remove(a), Ok(1));
        let b = t.insert(2).unwrap();
        assert_eq!(a.index(), b.index());
        assert_eq!(t.get(a), None);
        assert_eq!(t.get(b), Some(&2));
        assert_eq!(t.remove(a), Err(SlotError::Missing(a.index())));
    }

    #[test]
    fn restore_brings_a_row_back_under_its_id() {
        let mut t = Table::default();
        let ids: Vec<_> = (0..5).map(|i| t.insert(i).unwrap()).collect();
        assert_eq!(t.remove(ids[1]), Ok(1));
        assert_eq!(t.remove(ids[3]), Ok(3));
        t.restore(ids[1], 1).unwrap();
        assert_eq!(t.get(ids[1]), Some(&1));
        assert_eq!(t.restore(ids[1], 1), Err(SlotError::Occupied(1)));
        // The other free slot is still reused.
        let c = t.insert(9).unwrap();
        assert_eq!(c.index(), 3);
        assert_eq!(t.len(), 5);
    }

    #[test]
    fn slots_round_trip() {
        let mut t = Table::default();
        let ids: Vec<_> = (0..4).map(|i| t.insert(i).unwrap()).collect();
        t.remove(ids[2]).unwrap();
        let copy = Table::from_slots(t.slots().map(|(g, r)| (g, *r)));
        assert_eq!(copy, t);
        assert_eq!(copy.len(), 3);
        let mut copy = copy;
        assert_eq!(copy.insert(7).unwrap().index(), 2);
    }
}

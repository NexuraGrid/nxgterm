//! Image storage with a memory budget, plus the placements on screen.

use std::collections::{BTreeMap, HashMap};

use super::{Image, Placement};
use crate::size::CellPixels;

/// Decoded pixel bytes kept at most; the oldest images go first.
pub const DEFAULT_BUDGET: usize = 256 * 1024 * 1024;
/// Images kept at most, whatever their size.
pub const MAX_IMAGES: usize = 4096;
/// Placements kept at most; the oldest go first.
pub const MAX_PLACEMENTS: usize = 4096;

/// Images by key plus the placements that show them.
#[derive(Debug)]
pub struct ImageStore {
    /// Keys grow monotonically, so iteration order is age order.
    images: BTreeMap<u64, Image>,
    /// Client id to key.
    ids: HashMap<u32, u64>,
    /// In creation order; later ones draw on top within a z level.
    placements: Vec<Placement>,
    bytes: usize,
    budget: usize,
    next_key: u64,
    /// Ids handed out for images the client did not name, counting down
    /// from the top of the range to stay clear of client ids.
    next_auto_id: u32,
}

impl Default for ImageStore {
    fn default() -> Self {
        Self::new(DEFAULT_BUDGET)
    }
}

impl ImageStore {
    /// An empty store holding at most `budget` bytes of pixels.
    pub fn new(budget: usize) -> Self {
        Self {
            images: BTreeMap::new(),
            ids: HashMap::new(),
            placements: Vec::new(),
            bytes: 0,
            budget,
            next_key: 1,
            next_auto_id: u32::MAX,
        }
    }

    /// Pixel bytes currently stored.
    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn len(&self) -> usize {
        self.images.len()
    }

    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// The image with `key`, if still stored.
    pub fn image(&self, key: u64) -> Option<&Image> {
        self.images.get(&key)
    }

    /// Every stored image, oldest first.
    pub fn images(&self) -> impl Iterator<Item = &Image> {
        self.images.values()
    }

    /// The image the client calls `id`.
    pub fn by_id(&self, id: u32) -> Option<&Image> {
        self.ids.get(&id).and_then(|key| self.images.get(key))
    }

    /// The newest image transmitted with number `number`.
    pub fn by_number(&self, number: u32) -> Option<&Image> {
        if number == 0 {
            return None;
        }
        self.images.values().rev().find(|i| i.number == number)
    }

    /// Placements in creation order.
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }

    /// An id no stored image uses, for images the client did not name.
    pub fn unused_id(&mut self) -> u32 {
        loop {
            let id = self.next_auto_id;
            self.next_auto_id = self.next_auto_id.checked_sub(1).unwrap_or(u32::MAX);
            if id != 0 && !self.ids.contains_key(&id) {
                return id;
            }
        }
    }

    /// Stores an image under client `id`, replacing (and unplacing) any
    /// previous one with that id, then evicts the oldest images until the
    /// budget holds. Returns the new key, or `None` when the image alone
    /// exceeds the budget.
    pub fn insert(
        &mut self,
        id: u32,
        number: u32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Option<u64> {
        if let Some(&old) = self.ids.get(&id) {
            self.remove_image(old);
        }
        if rgba.len() > self.budget {
            return None;
        }
        while self.bytes + rgba.len() > self.budget || self.images.len() >= MAX_IMAGES {
            let Some(&oldest) = self.images.keys().next() else {
                break;
            };
            self.remove_image(oldest);
        }
        let key = self.next_key;
        self.next_key += 1;
        self.bytes += rgba.len();
        self.ids.insert(id, key);
        self.images.insert(
            key,
            Image {
                key,
                id,
                number,
                width,
                height,
                rgba,
            },
        );
        Some(key)
    }

    /// Frees an image and its placements.
    pub fn remove_image(&mut self, key: u64) {
        if let Some(image) = self.images.remove(&key) {
            self.bytes -= image.byte_len();
            if self.ids.get(&image.id) == Some(&key) {
                self.ids.remove(&image.id);
            }
        }
        self.placements.retain(|p| p.image != key);
    }

    /// Adds a placement. A non-zero placement id replaces the image's
    /// placement with the same id. Placements of unknown images are
    /// ignored.
    pub fn place(&mut self, placement: Placement) {
        if !self.images.contains_key(&placement.image) {
            return;
        }
        if placement.id != 0 {
            self.placements
                .retain(|p| !(p.image == placement.image && p.id == placement.id));
        }
        if self.placements.len() >= MAX_PLACEMENTS {
            self.placements.remove(0);
        }
        self.placements.push(placement);
    }

    /// Removes the placements matching `pred`. With `free`, images that
    /// lost a placement and have none left are freed too.
    pub fn remove_placements(&mut self, free: bool, mut pred: impl FnMut(&Placement) -> bool) {
        let mut touched = Vec::new();
        self.placements.retain(|p| {
            let hit = pred(p);
            if hit {
                touched.push(p.image);
            }
            !hit
        });
        if free {
            for key in touched {
                if !self.placements.iter().any(|p| p.image == key) {
                    self.remove_image(key);
                }
            }
        }
    }

    /// Removes every placement, keeping the images (screen clear).
    pub fn clear_placements(&mut self) {
        self.placements.clear();
    }

    /// Moves placements up `lines` rows as the grid scrolls, dropping the
    /// ones that leave the screen entirely.
    pub fn scroll_up(&mut self, lines: u32, cell: CellPixels) {
        let lines = i32::try_from(lines).unwrap_or(i32::MAX);
        self.placements.retain_mut(|p| {
            p.row = p.row.saturating_sub(lines);
            let (_, rows) = p.span(cell);
            i64::from(p.row) + i64::from(rows) > 0
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::SrcRect;

    const CELL: CellPixels = CellPixels {
        width: 10,
        height: 10,
    };

    fn pixels(w: u32, h: u32) -> Vec<u8> {
        vec![255; (w * h * 4) as usize]
    }

    fn at(image: u64, id: u32, row: i32, col: u32) -> Placement {
        Placement {
            image,
            id,
            row,
            col,
            offset_x: 0,
            offset_y: 0,
            src: SrcRect {
                x: 0,
                y: 0,
                width: 20,
                height: 20,
            },
            cols: 0,
            rows: 0,
            z: 0,
        }
    }

    #[test]
    fn inserts_and_finds_by_id_and_number() {
        let mut store = ImageStore::default();
        let key = store.insert(7, 3, 2, 2, pixels(2, 2)).unwrap();
        assert_eq!(store.by_id(7).unwrap().key, key);
        assert_eq!(store.by_number(3).unwrap().id, 7);
        assert!(store.by_number(0).is_none());
        assert_eq!(store.bytes(), 16);
    }

    #[test]
    fn retransmit_replaces_image_and_drops_its_placements() {
        let mut store = ImageStore::default();
        let old = store.insert(1, 0, 2, 2, pixels(2, 2)).unwrap();
        store.place(at(old, 0, 0, 0));
        let new = store.insert(1, 0, 1, 1, pixels(1, 1)).unwrap();
        assert_ne!(old, new, "keys are never reused");
        assert!(store.image(old).is_none());
        assert!(store.placements().is_empty());
        assert_eq!(store.bytes(), 4);
    }

    #[test]
    fn evicts_oldest_images_beyond_the_budget() {
        let mut store = ImageStore::new(40);
        let a = store.insert(1, 0, 2, 2, pixels(2, 2)).unwrap();
        let b = store.insert(2, 0, 2, 2, pixels(2, 2)).unwrap();
        store.place(at(a, 0, 0, 0));
        let c = store.insert(3, 0, 2, 2, pixels(2, 2)).unwrap();
        assert!(store.image(a).is_none(), "oldest evicted");
        assert!(store.image(b).is_some() && store.image(c).is_some());
        assert!(store.placements().is_empty(), "evicted image is unplaced");
        assert!(store.bytes() <= 40);
        assert_eq!(store.insert(4, 0, 4, 4, pixels(4, 4)), None, "too big");
    }

    #[test]
    fn placement_id_replaces_and_anonymous_ones_accumulate() {
        let mut store = ImageStore::default();
        let key = store.insert(1, 0, 2, 2, pixels(2, 2)).unwrap();
        store.place(at(key, 5, 0, 0));
        store.place(at(key, 5, 1, 1));
        store.place(at(key, 0, 2, 2));
        store.place(at(key, 0, 3, 3));
        store.place(at(999, 0, 0, 0));
        assert_eq!(store.placements().len(), 3);
        assert_eq!(store.placements()[0].row, 1);
    }

    #[test]
    fn remove_placements_frees_only_unreferenced_images_when_asked() {
        let mut store = ImageStore::default();
        let a = store.insert(1, 0, 2, 2, pixels(2, 2)).unwrap();
        let b = store.insert(2, 0, 2, 2, pixels(2, 2)).unwrap();
        store.place(at(a, 0, 0, 0));
        store.place(at(a, 0, 5, 0));
        store.place(at(b, 0, 0, 0));
        store.remove_placements(true, |p| p.row == 0);
        assert!(store.image(a).is_some(), "a still has a placement");
        assert!(store.image(b).is_none());
        store.remove_placements(false, |_| true);
        assert!(store.image(a).is_some(), "lowercase keeps the data");
    }

    #[test]
    fn scroll_moves_placements_and_drops_those_off_screen() {
        let mut store = ImageStore::default();
        let key = store.insert(1, 0, 2, 2, pixels(2, 2)).unwrap();
        // 20px tall placements span two rows.
        store.place(at(key, 1, 0, 0));
        store.place(at(key, 2, 3, 0));
        store.scroll_up(1, CELL);
        let rows: Vec<i32> = store.placements().iter().map(|p| p.row).collect();
        assert_eq!(rows, [-1, 2], "partly visible placement stays");
        store.scroll_up(1, CELL);
        assert_eq!(store.placements().len(), 1);
        assert_eq!(store.placements()[0].row, 1);
    }

    #[test]
    fn caps_placements_dropping_the_oldest() {
        let mut store = ImageStore::default();
        let key = store.insert(1, 0, 2, 2, pixels(2, 2)).unwrap();
        for row in 0..=MAX_PLACEMENTS as i32 {
            store.place(at(key, 0, row, 0));
        }
        assert_eq!(store.placements().len(), MAX_PLACEMENTS);
        assert_eq!(store.placements()[0].row, 1);
    }

    #[test]
    fn unused_ids_skip_taken_ones() {
        let mut store = ImageStore::default();
        store.insert(u32::MAX, 0, 1, 1, pixels(1, 1));
        assert_eq!(store.unused_id(), u32::MAX - 1);
        assert_eq!(store.unused_id(), u32::MAX - 2);
    }

    #[test]
    fn clear_placements_keeps_images() {
        let mut store = ImageStore::default();
        let key = store.insert(1, 0, 1, 1, pixels(1, 1)).unwrap();
        store.place(at(key, 0, 0, 0));
        store.clear_placements();
        assert!(store.placements().is_empty() && store.image(key).is_some());
    }
}

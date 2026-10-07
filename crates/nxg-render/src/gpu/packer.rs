//! Shelf (row) packer for the glyph atlas.

/// Packs rectangles left to right in rows ("shelves") as tall as their
/// tallest entry. Never frees single entries; [`ShelfPacker::clear`] resets.
#[derive(Debug, Clone)]
pub struct ShelfPacker {
    width: u32,
    height: u32,
    /// Next free x on the current shelf.
    x: u32,
    /// Top of the current shelf.
    y: u32,
    /// Height of the current shelf so far.
    shelf: u32,
}

impl ShelfPacker {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            x: 0,
            y: 0,
            shelf: 0,
        }
    }

    /// Reserves a `w x h` area and returns its top-left corner, or `None`
    /// when the atlas is full.
    pub fn alloc(&mut self, w: u32, h: u32) -> Option<[u32; 2]> {
        if w == 0 || h == 0 {
            return Some([0, 0]);
        }
        if w > self.width {
            return None;
        }
        if self.x + w > self.width {
            // Start a new shelf below the current one.
            self.y += self.shelf;
            self.x = 0;
            self.shelf = 0;
        }
        if self.y + h > self.height {
            return None;
        }
        let corner = [self.x, self.y];
        self.x += w;
        self.shelf = self.shelf.max(h);
        Some(corner)
    }

    /// Forgets every allocation.
    pub fn clear(&mut self) {
        *self = Self::new(self.width, self.height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_left_to_right_on_one_shelf() {
        let mut packer = ShelfPacker::new(10, 10);
        assert_eq!(packer.alloc(3, 2), Some([0, 0]));
        assert_eq!(packer.alloc(4, 5), Some([3, 0]));
        assert_eq!(packer.alloc(3, 1), Some([7, 0]));
    }

    #[test]
    fn opens_a_new_shelf_below_the_tallest_entry() {
        let mut packer = ShelfPacker::new(10, 10);
        packer.alloc(6, 2);
        packer.alloc(3, 4);
        assert_eq!(packer.alloc(2, 2), Some([0, 4]));
    }

    #[test]
    fn reports_full_when_nothing_fits() {
        let mut packer = ShelfPacker::new(4, 4);
        assert_eq!(packer.alloc(5, 1), None, "wider than the atlas");
        assert_eq!(packer.alloc(4, 3), Some([0, 0]));
        assert_eq!(packer.alloc(1, 2), None, "no room for a new shelf");
        assert_eq!(packer.alloc(1, 1), Some([0, 3]));
    }

    #[test]
    fn clear_starts_over() {
        let mut packer = ShelfPacker::new(4, 4);
        packer.alloc(4, 4);
        assert_eq!(packer.alloc(1, 1), None);
        packer.clear();
        assert_eq!(packer.alloc(1, 1), Some([0, 0]));
    }

    #[test]
    fn zero_sized_requests_take_no_space() {
        let mut packer = ShelfPacker::new(4, 4);
        assert_eq!(packer.alloc(0, 0), Some([0, 0]));
        assert_eq!(packer.alloc(4, 4), Some([0, 0]));
    }
}

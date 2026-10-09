//! The panes of a tab: a binary split tree with cell-aligned geometry.
//!
//! Generic over what a pane holds, so the logic is tested without spawning
//! shells or touching the renderer. Each pane gets a [`PaneId`] chosen by
//! the app and never reused, so events from a pane's background threads
//! that arrive after it closed are recognized and dropped.

/// Smallest width, in cells, a split may leave a pane.
pub const MIN_COLS: u16 = 4;
/// Smallest height, in cells, a split may leave a pane.
pub const MIN_ROWS: u16 = 2;

/// Identifies a pane for its whole life, unlike its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PaneId(u64);

impl PaneId {
    /// Wraps a raw id. The app owns the counter so ids stay unique
    /// across every tab.
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }
}

/// A direction on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Left,
    Right,
    Up,
    Down,
}

/// How a split lays out its two children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Side by side.
    Right,
    /// One above the other.
    Down,
}

/// A rectangle of whole cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    pub col: u16,
    pub row: u16,
    pub cols: u16,
    pub rows: u16,
}

/// Why a split did not happen.
#[derive(Debug, PartialEq, Eq)]
pub enum SplitError<E> {
    /// A half would fall under [`MIN_COLS`] x [`MIN_ROWS`].
    TooSmall,
    /// The leaf factory failed.
    Make(E),
}

/// Where a divider sits in the tree: `false` = first child, `true` =
/// second, from the root. Any structural change invalidates it.
pub type DividerPath = Vec<bool>;

/// The reserved line between the two halves of a split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Divider {
    pub rect: CellRect,
    pub axis: Axis,
    pub path: DividerPath,
}

/// Splits `len` cells in two around a 1-cell divider, returning the
/// bounds `(total, lo, hi)` the first half may take. When the area is
/// smaller than both minimums the floor relaxes to 1 cell so geometry
/// never panics (the frame clips whatever overflows).
fn bounds(len: u16, min: u16) -> (u16, u16, u16) {
    let total = len.saturating_sub(1);
    let min = if total >= min.saturating_mul(2) {
        min
    } else {
        1
    };
    let hi = total.saturating_sub(min);
    (total, min.min(hi), hi)
}

fn min_of(axis: Axis) -> u16 {
    match axis {
        Axis::Right => MIN_COLS,
        Axis::Down => MIN_ROWS,
    }
}

fn len_of(rect: CellRect, axis: Axis) -> u16 {
    match axis {
        Axis::Right => rect.cols,
        Axis::Down => rect.rows,
    }
}

/// Cells given to the first half of a split over `len` cells.
fn first_len(len: u16, ratio: u16, min: u16) -> u16 {
    let (total, lo, hi) = bounds(len, min);
    let a = (u32::from(total) * u32::from(ratio) + 500) / 1000;
    u16::try_from(a).unwrap_or(u16::MAX).clamp(lo, hi)
}

/// The first half, the divider and the second half of `rect`.
fn cut(rect: CellRect, axis: Axis, ratio: u16) -> (CellRect, CellRect, CellRect) {
    let a = first_len(len_of(rect, axis), ratio, min_of(axis));
    let b = len_of(rect, axis).saturating_sub(1).saturating_sub(a);
    match axis {
        Axis::Right => {
            let col = rect.col.saturating_add(a);
            (
                CellRect { cols: a, ..rect },
                CellRect {
                    col,
                    cols: 1,
                    ..rect
                },
                CellRect {
                    col: col.saturating_add(1),
                    cols: b,
                    ..rect
                },
            )
        }
        Axis::Down => {
            let row = rect.row.saturating_add(a);
            (
                CellRect { rows: a, ..rect },
                CellRect {
                    row,
                    rows: 1,
                    ..rect
                },
                CellRect {
                    row: row.saturating_add(1),
                    rows: b,
                    ..rect
                },
            )
        }
    }
}

#[derive(Debug)]
enum Node<T> {
    Leaf(PaneId, T),
    Split {
        axis: Axis,
        /// Permille of the space (minus the divider) given to `a`.
        ratio: u16,
        a: Box<Node<T>>,
        b: Box<Node<T>>,
    },
}

impl<T> Node<T> {
    fn leaves<'a>(&'a self, out: &mut Vec<(PaneId, &'a T)>) {
        match self {
            Node::Leaf(id, leaf) => out.push((*id, leaf)),
            Node::Split { a, b, .. } => {
                a.leaves(out);
                b.leaves(out);
            }
        }
    }

    fn find(&self, target: PaneId) -> Option<&T> {
        match self {
            Node::Leaf(id, leaf) => (*id == target).then_some(leaf),
            Node::Split { a, b, .. } => a.find(target).or_else(|| b.find(target)),
        }
    }

    fn find_mut(&mut self, target: PaneId) -> Option<&mut T> {
        match self {
            Node::Leaf(id, leaf) => (*id == target).then_some(leaf),
            Node::Split { a, b, .. } => a.find_mut(target).or_else(|| b.find_mut(target)),
        }
    }

    /// Lays the subtree out inside `rect`, in tree order.
    fn lay_out(
        &self,
        rect: CellRect,
        path: &mut DividerPath,
        panes: &mut Vec<(PaneId, CellRect)>,
        dividers: &mut Vec<Divider>,
    ) {
        match self {
            Node::Leaf(id, _) => panes.push((*id, rect)),
            Node::Split { axis, ratio, a, b } => {
                let (first, divider, second) = cut(rect, *axis, *ratio);
                path.push(false);
                a.lay_out(first, path, panes, dividers);
                path.pop();
                dividers.push(Divider {
                    rect: divider,
                    axis: *axis,
                    path: path.clone(),
                });
                path.push(true);
                b.lay_out(second, path, panes, dividers);
                path.pop();
            }
        }
    }

    /// Replaces the leaf `target` with a split of it and `new`.
    fn split_leaf(self, target: PaneId, axis: Axis, new: &mut Option<Node<T>>) -> Node<T> {
        match self {
            Node::Leaf(id, _) if id == target => match new.take() {
                Some(new) => Node::Split {
                    axis,
                    ratio: 500,
                    a: Box::new(self),
                    b: Box::new(new),
                },
                None => self,
            },
            Node::Leaf(..) => self,
            Node::Split {
                axis: own,
                ratio,
                a,
                b,
            } => Node::Split {
                axis: own,
                ratio,
                a: Box::new(a.split_leaf(target, axis, new)),
                b: Box::new(b.split_leaf(target, axis, new)),
            },
        }
    }
}

/// A binary split tree of panes with one focused pane and, optionally,
/// one zoomed to fill the whole area.
#[derive(Debug)]
pub struct Panes<T> {
    /// Only `None` while an edit rebuilds the tree.
    root: Option<Node<T>>,
    focus: PaneId,
    zoomed: Option<PaneId>,
}

impl<T> Panes<T> {
    /// A tree of one pane, which has focus.
    pub fn new(id: PaneId, leaf: T) -> Self {
        Self {
            root: Some(Node::Leaf(id, leaf)),
            focus: id,
            zoomed: None,
        }
    }

    fn root(&self) -> &Node<T> {
        self.root.as_ref().expect("the tree always has a root")
    }

    pub fn get(&self, id: PaneId) -> Option<&T> {
        self.root().find(id)
    }

    pub fn get_mut(&mut self, id: PaneId) -> Option<&mut T> {
        self.root.as_mut()?.find_mut(id)
    }

    pub fn contains(&self, id: PaneId) -> bool {
        self.get(id).is_some()
    }

    /// The focused pane.
    pub fn focused(&self) -> (PaneId, &T) {
        let leaf = self.get(self.focus).expect("focus is always a leaf");
        (self.focus, leaf)
    }

    /// Every pane in tree order (left/top first).
    pub fn iter(&self) -> impl Iterator<Item = (PaneId, &T)> {
        let mut out = Vec::new();
        self.root().leaves(&mut out);
        out.into_iter()
    }

    pub fn len(&self) -> usize {
        self.iter().count()
    }

    /// The pane filling the area, if any.
    pub fn zoomed(&self) -> Option<PaneId> {
        self.zoomed
    }

    /// Zooms the focused pane, or restores the layout when zoomed. A
    /// single pane has nothing to zoom.
    pub fn toggle_zoom(&mut self) {
        self.zoomed = match self.zoomed {
            Some(_) => None,
            None if self.len() > 1 => Some(self.focus),
            None => None,
        };
    }

    fn lay_out(&self, area: (u16, u16)) -> (Vec<(PaneId, CellRect)>, Vec<Divider>) {
        let (mut panes, mut dividers) = (Vec::new(), Vec::new());
        let full = CellRect {
            col: 0,
            row: 0,
            cols: area.0,
            rows: area.1,
        };
        self.root()
            .lay_out(full, &mut Vec::new(), &mut panes, &mut dividers);
        (panes, dividers)
    }

    /// Where each pane sits in an `area` of (cols, rows), in tree order.
    /// While zoomed, only the zoomed pane, filling the area.
    pub fn rects(&self, area: (u16, u16)) -> Vec<(PaneId, CellRect)> {
        match self.zoomed {
            Some(id) => vec![(
                id,
                CellRect {
                    col: 0,
                    row: 0,
                    cols: area.0,
                    rows: area.1,
                },
            )],
            None => self.lay_out(area).0,
        }
    }

    /// The divider lines between panes (none while zoomed).
    pub fn dividers(&self, area: (u16, u16)) -> Vec<Divider> {
        match self.zoomed {
            Some(_) => Vec::new(),
            None => self.lay_out(area).1,
        }
    }

    /// Splits the focused pane. `make` gets the new pane's rect (so its
    /// pty can start at the right size) and builds the leaf; when it
    /// fails, or the halves would be too small, nothing changes. A
    /// successful split ends zoom and focuses the new pane.
    pub fn split_with<E>(
        &mut self,
        axis: Axis,
        id: PaneId,
        area: (u16, u16),
        make: impl FnOnce(CellRect) -> Result<T, E>,
    ) -> Result<(), SplitError<E>> {
        debug_assert!(!self.contains(id), "pane ids are never reused");
        let (_, rect) = self
            .lay_out(area)
            .0
            .into_iter()
            .find(|(pane, _)| *pane == self.focus)
            .expect("focus is always a leaf");
        let (total, ..) = bounds(len_of(rect, axis), min_of(axis));
        if total < min_of(axis).saturating_mul(2) {
            return Err(SplitError::TooSmall);
        }
        let (_, _, second) = cut(rect, axis, 500);
        let leaf = make(second).map_err(SplitError::Make)?;
        let root = self.root.take().expect("the tree always has a root");
        self.root = Some(root.split_leaf(self.focus, axis, &mut Some(Node::Leaf(id, leaf))));
        self.focus = id;
        self.zoomed = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> PaneId {
        PaneId::new(n)
    }

    fn split(
        panes: &mut Panes<u32>,
        axis: Axis,
        n: u64,
        area: (u16, u16),
    ) -> Result<(), SplitError<()>> {
        panes.split_with(axis, id(n), area, |_| Ok(n as u32))
    }

    fn rect(col: u16, row: u16, cols: u16, rows: u16) -> CellRect {
        CellRect {
            col,
            row,
            cols,
            rows,
        }
    }

    fn rect_of(panes: &Panes<u32>, area: (u16, u16), n: u64) -> CellRect {
        panes
            .rects(area)
            .into_iter()
            .find(|(pane, _)| *pane == id(n))
            .map(|(_, rect)| rect)
            .expect("pane has a rect")
    }

    /// Panes in a 100x30 area: 1, split right -> 2.
    fn two() -> Panes<u32> {
        let mut panes = Panes::new(id(1), 1);
        split(&mut panes, Axis::Right, 2, (100, 30)).unwrap();
        panes
    }

    // 1.1 types and accessors

    #[test]
    fn a_new_tree_has_one_focused_pane() {
        let panes = Panes::new(id(7), 70);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes.focused(), (id(7), &70));
        assert!(panes.contains(id(7)));
        assert_eq!(panes.zoomed(), None);
    }

    #[test]
    fn unknown_ids_are_none() {
        let mut panes = Panes::new(id(1), 10);
        assert!(!panes.contains(id(9)));
        assert_eq!(panes.get(id(9)), None);
        assert_eq!(panes.get_mut(id(9)), None);
    }

    #[test]
    fn get_mut_changes_the_leaf() {
        let mut panes = two();
        *panes.get_mut(id(2)).unwrap() = 99;
        assert_eq!(panes.get(id(2)), Some(&99));
        assert_eq!(panes.get(id(1)), Some(&1));
    }

    #[test]
    fn iter_follows_tree_order() {
        let mut panes = two();
        split(&mut panes, Axis::Down, 3, (100, 30)).unwrap();
        let order: Vec<_> = panes.iter().map(|(pane, leaf)| (pane, *leaf)).collect();
        assert_eq!(order, vec![(id(1), 1), (id(2), 2), (id(3), 3)]);
    }

    #[test]
    fn ids_compare_by_value() {
        assert_eq!(id(3), PaneId::new(3));
        assert_ne!(id(3), id(4));
    }

    // 1.2 rects

    /// Every cell of the area belongs to exactly one pane or divider.
    fn assert_tiles(panes: &Panes<u32>, area: (u16, u16)) {
        let cells = |r: &CellRect| u32::from(r.cols) * u32::from(r.rows);
        let panes_cells: u32 = panes.rects(area).iter().map(|(_, r)| cells(r)).sum();
        let divider_cells: u32 = panes.dividers(area).iter().map(|d| cells(&d.rect)).sum();
        assert_eq!(
            panes_cells + divider_cells,
            u32::from(area.0) * u32::from(area.1)
        );
        for (_, r) in panes.rects(area) {
            assert!(r.col + r.cols <= area.0 && r.row + r.rows <= area.1);
        }
    }

    #[test]
    fn one_pane_fills_the_area() {
        let panes = Panes::new(id(1), 1);
        assert_eq!(panes.rects((80, 24)), vec![(id(1), rect(0, 0, 80, 24))]);
    }

    #[test]
    fn two_panes_plus_divider_equal_the_width() {
        let panes = two();
        let (a, b) = (rect_of(&panes, (100, 30), 1), rect_of(&panes, (100, 30), 2));
        assert_eq!((a.cols, b.cols), (50, 49));
        assert_eq!(a.cols + 1 + b.cols, 100);
        assert_eq!(b.col, a.col + a.cols + 1);
        assert_eq!((a.rows, b.rows), (30, 30));
    }

    #[test]
    fn nested_layouts_tile_the_area() {
        let mut panes = two();
        split(&mut panes, Axis::Down, 3, (100, 30)).unwrap();
        for area in [(100, 30), (101, 31), (57, 13)] {
            assert_tiles(&panes, area);
        }
    }

    #[test]
    fn tiny_areas_never_panic() {
        let mut panes = two();
        split(&mut panes, Axis::Down, 3, (100, 30)).unwrap();
        split(&mut panes, Axis::Right, 4, (100, 30)).unwrap();
        for area in [
            (0, 0),
            (1, 1),
            (2, 1),
            (3, 2),
            (5, 5),
            (7, 3),
            (u16::MAX, 1),
        ] {
            assert_eq!(panes.rects(area).len(), 4);
        }
    }

    #[test]
    fn minimums_relax_below_the_summed_minimums() {
        let panes = two();
        let (a, b) = (rect_of(&panes, (6, 4), 1), rect_of(&panes, (6, 4), 2));
        assert_eq!((a.cols, b.cols), (3, 2));
        assert!(a.cols >= 1 && b.cols >= 1);
    }

    // 1.3 split_with

    #[test]
    fn split_right_puts_the_new_pane_beside_and_focuses_it() {
        let mut panes = Panes::new(id(1), 1);
        let mut given = None;
        panes
            .split_with(Axis::Right, id(2), (100, 30), |r| {
                given = Some(r);
                Ok::<_, ()>(2)
            })
            .unwrap();
        assert_eq!(given, Some(rect(51, 0, 49, 30)));
        assert_eq!(
            panes.rects((100, 30)),
            vec![(id(1), rect(0, 0, 50, 30)), (id(2), rect(51, 0, 49, 30))]
        );
        assert_eq!(panes.focused(), (id(2), &2));
    }

    #[test]
    fn split_down_puts_the_new_pane_below() {
        let mut panes = Panes::new(id(1), 1);
        split(&mut panes, Axis::Down, 2, (100, 30)).unwrap();
        assert_eq!(rect_of(&panes, (100, 30), 1), rect(0, 0, 100, 15));
        assert_eq!(rect_of(&panes, (100, 30), 2), rect(0, 16, 100, 14));
    }

    #[test]
    fn split_splits_the_focused_pane_only() {
        let mut panes = two();
        split(&mut panes, Axis::Down, 3, (100, 30)).unwrap();
        assert_eq!(rect_of(&panes, (100, 30), 1), rect(0, 0, 50, 30));
        assert_eq!(rect_of(&panes, (100, 30), 2), rect(51, 0, 49, 15));
        assert_eq!(rect_of(&panes, (100, 30), 3), rect(51, 16, 49, 14));
    }

    #[test]
    fn split_is_refused_when_a_half_would_be_too_small() {
        let mut calls = 0;
        let mut panes = Panes::new(id(1), 1);
        let result = panes.split_with(Axis::Right, id(2), (8, 30), |_| {
            calls += 1;
            Ok::<_, ()>(2)
        });
        assert_eq!(result, Err(SplitError::TooSmall));
        let result = panes.split_with(Axis::Down, id(2), (80, 4), |_| {
            calls += 1;
            Ok::<_, ()>(2)
        });
        assert_eq!(result, Err(SplitError::TooSmall));
        assert_eq!(calls, 0);
        assert_eq!(panes.len(), 1);
        assert_eq!(panes.focused(), (id(1), &1));
    }

    #[test]
    fn the_smallest_splittable_area_leaves_the_minimums() {
        let mut panes = Panes::new(id(1), 1);
        split(&mut panes, Axis::Right, 2, (MIN_COLS * 2 + 1, 30)).unwrap();
        let area = (MIN_COLS * 2 + 1, 30);
        assert_eq!(rect_of(&panes, area, 1).cols, MIN_COLS);
        assert_eq!(rect_of(&panes, area, 2).cols, MIN_COLS);
        let mut panes = Panes::new(id(1), 1);
        split(&mut panes, Axis::Down, 2, (80, MIN_ROWS * 2 + 1)).unwrap();
        let area = (80, MIN_ROWS * 2 + 1);
        assert_eq!(rect_of(&panes, area, 1).rows, MIN_ROWS);
        assert_eq!(rect_of(&panes, area, 2).rows, MIN_ROWS);
    }

    #[test]
    fn a_failing_factory_leaves_the_tree_unchanged() {
        let mut panes = two();
        let before = panes.rects((100, 30));
        let result = panes.split_with(Axis::Down, id(3), (100, 30), |_| Err("boom"));
        assert_eq!(result, Err(SplitError::Make("boom")));
        assert_eq!(panes.rects((100, 30)), before);
        assert_eq!(panes.len(), 2);
        assert!(!panes.contains(id(3)));
        assert_eq!(panes.focused(), (id(2), &2));
    }

    #[test]
    fn a_split_ends_zoom_and_a_refused_one_keeps_it() {
        let mut panes = two();
        panes.toggle_zoom();
        assert_eq!(panes.zoomed(), Some(id(2)));
        let refused = panes.split_with(Axis::Down, id(3), (100, 3), |_| Ok::<_, ()>(3));
        assert_eq!(refused, Err(SplitError::TooSmall));
        assert_eq!(panes.zoomed(), Some(id(2)));
        split(&mut panes, Axis::Down, 3, (100, 30)).unwrap();
        assert_eq!(panes.zoomed(), None);
    }
}

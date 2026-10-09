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

impl CellRect {
    /// Whether the cell (`col`, `row`) is inside.
    pub fn contains(&self, col: u16, row: u16) -> bool {
        let inside = |at: u16, start: u16, len: u16| {
            at >= start && u32::from(at) < u32::from(start) + u32::from(len)
        };
        inside(col, self.col, self.cols) && inside(row, self.row, self.rows)
    }
}

/// Why a split did not happen.
#[derive(Debug, PartialEq, Eq)]
pub enum SplitError<E> {
    /// A half would fall under [`MIN_COLS`] x [`MIN_ROWS`].
    TooSmall,
    /// The leaf factory failed.
    Make(E),
}

/// What closing a pane did.
#[derive(Debug, PartialEq, Eq)]
pub enum Closed<T> {
    /// It was the only pane; the tree is unchanged and the caller drops
    /// the whole tab.
    Last,
    /// The pane left the tree and its sibling took the space.
    Removed(T),
    /// No such pane (already closed): nothing changed.
    Unknown,
}

/// What lies under a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hit {
    Pane(PaneId),
    Divider(DividerPath),
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

/// Cells shared by the spans `[a0, a0 + a_len)` and `[b0, b0 + b_len)`.
fn overlap(a0: u16, a_len: u16, b0: u16, b_len: u16) -> u32 {
    let start = u32::from(a0).max(u32::from(b0));
    let end = (u32::from(a0) + u32::from(a_len)).min(u32::from(b0) + u32::from(b_len));
    end.saturating_sub(start)
}

/// How much of `from`'s `dir` edge `to` faces across a 1-cell divider
/// (`None` when it is not adjacent that way).
fn facing_overlap(from: CellRect, to: CellRect, dir: Dir) -> Option<u32> {
    let (near_from, near_to) = match dir {
        Dir::Right => (
            u32::from(from.col) + u32::from(from.cols) + 1,
            u32::from(to.col),
        ),
        Dir::Left => (
            u32::from(from.col),
            u32::from(to.col) + u32::from(to.cols) + 1,
        ),
        Dir::Down => (
            u32::from(from.row) + u32::from(from.rows) + 1,
            u32::from(to.row),
        ),
        Dir::Up => (
            u32::from(from.row),
            u32::from(to.row) + u32::from(to.rows) + 1,
        ),
    };
    if near_from != near_to {
        return None;
    }
    let shared = match dir {
        Dir::Left | Dir::Right => overlap(from.row, from.rows, to.row, to.rows),
        Dir::Up | Dir::Down => overlap(from.col, from.cols, to.col, to.cols),
    };
    (shared > 0).then_some(shared)
}

/// The ratio (permille) that gives the first half `first` of the cells
/// left once the divider of a `len`-cell split is taken out.
fn ratio_for(len: u16, first: u16) -> u16 {
    let total = u32::from(len.saturating_sub(1));
    if total == 0 {
        return 500;
    }
    let ratio = (u32::from(first) * 1000 + total / 2) / total;
    u16::try_from(ratio).unwrap_or(1000).min(1000)
}

/// What [`Node::remove`] found.
struct Removal<T> {
    leaf: Option<T>,
    nearest: Option<PaneId>,
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

    fn leaf_count(&self) -> u32 {
        match self {
            Node::Leaf(..) => 1,
            Node::Split { a, b, .. } => a.leaf_count() + b.leaf_count(),
        }
    }

    /// Directions from this node down to the leaf `target`.
    fn path_to(&self, target: PaneId) -> Option<DividerPath> {
        match self {
            Node::Leaf(id, _) => (*id == target).then(Vec::new),
            Node::Split { a, b, .. } => a
                .path_to(target)
                .map(|mut path| {
                    path.insert(0, false);
                    path
                })
                .or_else(|| {
                    b.path_to(target).map(|mut path| {
                        path.insert(0, true);
                        path
                    })
                }),
        }
    }

    fn equalize(&mut self) {
        if let Node::Split { ratio, a, b, .. } = self {
            let (first, second) = (a.leaf_count(), b.leaf_count());
            *ratio = u16::try_from((first * 1000 + (first + second) / 2) / (first + second))
                .unwrap_or(500);
            a.equalize();
            b.equalize();
        }
    }

    /// The leaf of this subtree closest to a sibling that sat first
    /// (`closed_first`) or second along `axis`.
    fn nearest_leaf(&self, axis: Axis, closed_first: bool) -> PaneId {
        match self {
            Node::Leaf(id, _) => *id,
            Node::Split {
                axis: own, a, b, ..
            } => {
                if *own == axis && !closed_first {
                    b.nearest_leaf(axis, closed_first)
                } else {
                    a.nearest_leaf(axis, closed_first)
                }
            }
        }
    }

    /// Takes the leaf `target` out; its sibling subtree replaces the split.
    fn remove(self, target: PaneId, out: &mut Removal<T>) -> Node<T> {
        let Node::Split { axis, ratio, a, b } = self else {
            return self;
        };
        let (a, b) = (*a, *b);
        let a = match a.take_leaf(target) {
            Ok(leaf) => {
                out.leaf = Some(leaf);
                out.nearest = Some(b.nearest_leaf(axis, true));
                return b;
            }
            Err(a) => a,
        };
        let b = match b.take_leaf(target) {
            Ok(leaf) => {
                out.leaf = Some(leaf);
                out.nearest = Some(a.nearest_leaf(axis, false));
                return a;
            }
            Err(b) => b,
        };
        Node::Split {
            axis,
            ratio,
            a: Box::new(a.remove(target, out)),
            b: Box::new(b.remove(target, out)),
        }
    }

    /// The payload when this node is the leaf `target`, itself otherwise.
    fn take_leaf(self, target: PaneId) -> Result<T, Node<T>> {
        match self {
            Node::Leaf(id, leaf) if id == target => Ok(leaf),
            other => Err(other),
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

    /// Removes `id` (shell exit or action); the sibling subtree takes the
    /// space. If the pane had focus, it moves to the sibling's leaf
    /// nearest the closed one. Closing the zoomed pane ends zoom.
    pub fn close(&mut self, id: PaneId) -> Closed<T> {
        if !self.contains(id) {
            return Closed::Unknown;
        }
        if self.len() == 1 {
            return Closed::Last;
        }
        let root = self.root.take().expect("the tree always has a root");
        let mut removal = Removal {
            leaf: None,
            nearest: None,
        };
        self.root = Some(root.remove(id, &mut removal));
        if self.focus == id {
            self.focus = removal.nearest.expect("a sibling took the space");
        }
        if self.zoomed == Some(id) {
            self.zoomed = None;
        }
        Closed::Removed(removal.leaf.expect("the pane was found"))
    }

    /// Focuses `id`; returns whether it exists. Changing focus ends zoom.
    pub fn set_focus(&mut self, id: PaneId) -> bool {
        if !self.contains(id) {
            return false;
        }
        if id != self.focus {
            self.focus = id;
            self.zoomed = None;
        }
        true
    }

    /// Moves focus to the pane adjacent to the focused one in `dir`: the
    /// one whose edge faces it across the divider with the largest
    /// overlap, first in tree order on a tie. Returns false (and changes
    /// nothing) when there is none.
    pub fn focus_dir(&mut self, dir: Dir, area: (u16, u16)) -> bool {
        let panes = self.lay_out(area).0;
        let Some(&(_, from)) = panes.iter().find(|(id, _)| *id == self.focus) else {
            return false;
        };
        let mut best: Option<(PaneId, u32)> = None;
        for &(id, to) in &panes {
            let Some(overlap) = facing_overlap(from, to, dir) else {
                continue;
            };
            if best.is_none_or(|(_, most)| overlap > most) {
                best = Some((id, overlap));
            }
        }
        match best {
            Some((id, _)) => self.set_focus(id),
            None => false,
        }
    }

    /// Moves the innermost divider above the focused pane whose axis
    /// matches `dir` by `cells` toward `dir`, keeping both sides at the
    /// minimum size. Returns whether it moved; a moved divider ends zoom.
    pub fn resize(&mut self, dir: Dir, cells: u16, area: (u16, u16)) -> bool {
        let wanted = match dir {
            Dir::Left | Dir::Right => Axis::Right,
            Dir::Up | Dir::Down => Axis::Down,
        };
        let grow = matches!(dir, Dir::Right | Dir::Down);
        let Some(path) = self.root().path_to(self.focus) else {
            return false;
        };
        for depth in (0..path.len()).rev() {
            let Some((ratio, axis, rect)) = self.split_at_mut(&path[..depth], area) else {
                continue;
            };
            if axis != wanted {
                continue;
            }
            let len = len_of(rect, axis);
            let (_, lo, hi) = bounds(len, min_of(axis));
            let now = first_len(len, *ratio, min_of(axis));
            let to = if grow {
                now.saturating_add(cells)
            } else {
                now.saturating_sub(cells)
            };
            let to = to.clamp(lo, hi);
            if to == now {
                return false;
            }
            *ratio = ratio_for(len, to);
            self.zoomed = None;
            return true;
        }
        false
    }

    /// Puts the divider at `divider` under the pointer at cell `pos`
    /// (a column for side-by-side splits, a row for stacked ones),
    /// clamped to the minimums. A path that no longer names a split is
    /// ignored.
    pub fn drag(&mut self, divider: &DividerPath, pos: u16, area: (u16, u16)) {
        let Some((ratio, axis, rect)) = self.split_at_mut(divider, area) else {
            return;
        };
        let (len, origin) = match axis {
            Axis::Right => (rect.cols, rect.col),
            Axis::Down => (rect.rows, rect.row),
        };
        let (_, lo, hi) = bounds(len, min_of(axis));
        *ratio = ratio_for(len, pos.saturating_sub(origin).clamp(lo, hi));
    }

    /// Gives every pane the same share: each split's ratio follows its
    /// sides' leaf counts.
    pub fn equalize(&mut self) {
        if let Some(root) = self.root.as_mut() {
            root.equalize();
        }
    }

    /// The split at `path` from the root: its ratio, axis and rect.
    fn split_at_mut(
        &mut self,
        path: &[bool],
        area: (u16, u16),
    ) -> Option<(&mut u16, Axis, CellRect)> {
        let mut rect = CellRect {
            col: 0,
            row: 0,
            cols: area.0,
            rows: area.1,
        };
        let mut node = self.root.as_mut()?;
        for &side in path {
            let Node::Split { axis, ratio, a, b } = node else {
                return None;
            };
            let (first, _, second) = cut(rect, *axis, *ratio);
            (node, rect) = if side {
                (&mut **b, second)
            } else {
                (&mut **a, first)
            };
        }
        match node {
            Node::Split { axis, ratio, .. } => Some((ratio, *axis, rect)),
            Node::Leaf(..) => None,
        }
    }

    /// What lies under the cell (`col`, `row`) of an `area`: a pane or a
    /// divider (none outside the area).
    pub fn hit(&self, area: (u16, u16), col: u16, row: u16) -> Option<Hit> {
        if let Some((id, _)) = self
            .rects(area)
            .into_iter()
            .find(|(_, rect)| rect.contains(col, row))
        {
            return Some(Hit::Pane(id));
        }
        self.dividers(area)
            .into_iter()
            .find(|divider| divider.rect.contains(col, row))
            .map(|divider| Hit::Divider(divider.path))
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

    // 1.4 close

    /// 1 | (2 / 3), with 3 focused.
    fn three() -> Panes<u32> {
        let mut panes = two();
        split(&mut panes, Axis::Down, 3, (100, 30)).unwrap();
        panes
    }

    #[test]
    fn closing_a_pane_gives_its_space_to_the_sibling() {
        let mut panes = two();
        assert_eq!(panes.close(id(2)), Closed::Removed(2));
        assert_eq!(panes.rects((100, 30)), vec![(id(1), rect(0, 0, 100, 30))]);
        assert_eq!(panes.focused(), (id(1), &1));
        assert!(!panes.contains(id(2)));
    }

    #[test]
    fn closing_an_unfocused_pane_keeps_focus() {
        let mut panes = three();
        assert_eq!(panes.close(id(1)), Closed::Removed(1));
        assert_eq!(panes.focused(), (id(3), &3));
        assert_eq!(panes.len(), 2);
        assert_tiles(&panes, (100, 30));
    }

    #[test]
    fn closing_the_last_pane_reports_it_and_changes_nothing() {
        let mut panes = Panes::new(id(1), 1);
        assert_eq!(panes.close(id(1)), Closed::Last);
        assert_eq!(panes.len(), 1);
        assert!(panes.contains(id(1)));
    }

    #[test]
    fn closing_an_unknown_pane_changes_nothing() {
        let mut panes = two();
        assert_eq!(panes.close(id(9)), Closed::Unknown);
        assert_eq!(panes.len(), 2);
        assert_eq!(panes.close(id(2)), Closed::Removed(2));
        assert_eq!(panes.close(id(2)), Closed::Unknown);
    }

    #[test]
    fn focus_moves_to_the_leaf_nearest_the_closed_pane() {
        // 1 | (2 / 3): closing 1 leaves 2 / 3, nearest the left edge is 2.
        let mut panes = three();
        assert!(panes.set_focus(id(1)));
        assert_eq!(panes.close(id(1)), Closed::Removed(1));
        assert_eq!(panes.focused().0, id(2));

        // (1 | 3) | 2: closing 2 leaves 1 | 3, nearest is 3 (beside it).
        let mut panes = two();
        assert!(panes.set_focus(id(1)));
        split(&mut panes, Axis::Right, 3, (100, 30)).unwrap();
        assert!(panes.set_focus(id(2)));
        assert_eq!(panes.close(id(2)), Closed::Removed(2));
        assert_eq!(panes.focused().0, id(3));
    }

    #[test]
    fn closing_the_zoomed_pane_ends_zoom() {
        let mut panes = two();
        panes.toggle_zoom();
        assert_eq!(panes.close(id(2)), Closed::Removed(2));
        assert_eq!(panes.zoomed(), None);
        assert_eq!(panes.rects((100, 30)).len(), 1);
    }

    #[test]
    fn closing_another_pane_keeps_zoom() {
        let mut panes = three();
        panes.toggle_zoom();
        assert_eq!(panes.close(id(1)), Closed::Removed(1));
        assert_eq!(panes.zoomed(), Some(id(3)));
    }

    // 1.5 focus

    #[test]
    fn set_focus_selects_known_panes_only() {
        let mut panes = two();
        assert!(panes.set_focus(id(1)));
        assert_eq!(panes.focused().0, id(1));
        assert!(!panes.set_focus(id(9)));
        assert_eq!(panes.focused().0, id(1));
    }

    #[test]
    fn changing_focus_ends_zoom_but_keeping_it_does_not() {
        let mut panes = two();
        panes.toggle_zoom();
        assert!(panes.set_focus(id(2)));
        assert_eq!(panes.zoomed(), Some(id(2)));
        assert!(panes.set_focus(id(1)));
        assert_eq!(panes.zoomed(), None);
    }

    #[test]
    fn focus_moves_to_the_adjacent_pane() {
        let mut panes = two();
        assert!(panes.set_focus(id(1)));
        assert!(panes.focus_dir(Dir::Right, (100, 30)));
        assert_eq!(panes.focused().0, id(2));
        assert!(panes.focus_dir(Dir::Left, (100, 30)));
        assert_eq!(panes.focused().0, id(1));
    }

    #[test]
    fn focus_does_nothing_without_a_neighbour() {
        let mut panes = two();
        assert!(panes.set_focus(id(1)));
        for dir in [Dir::Left, Dir::Up, Dir::Down] {
            assert!(!panes.focus_dir(dir, (100, 30)));
        }
        assert_eq!(panes.focused().0, id(1));
        assert!(!Panes::new(id(1), 1).focus_dir(Dir::Right, (100, 30)));
    }

    #[test]
    fn focus_prefers_the_largest_overlap_then_tree_order() {
        // 1 | (2 / 3): from 1 going right, 2 (15 rows) beats 3 (14).
        let mut panes = three();
        assert!(panes.set_focus(id(1)));
        assert!(panes.focus_dir(Dir::Right, (100, 30)));
        assert_eq!(panes.focused().0, id(2));
        // With 31 rows both overlap 15: tree order picks 2.
        assert!(panes.set_focus(id(1)));
        assert!(panes.focus_dir(Dir::Right, (100, 31)));
        assert_eq!(panes.focused().0, id(2));
    }

    #[test]
    fn focus_picks_the_largest_neighbour_on_asymmetric_nests() {
        // Left column: (1 / 4) over 3, so 3 is tall and last in tree order.
        let mut panes = Panes::new(id(1), 1);
        split(&mut panes, Axis::Right, 2, (100, 30)).unwrap();
        assert!(panes.set_focus(id(1)));
        split(&mut panes, Axis::Down, 3, (100, 30)).unwrap();
        assert!(panes.set_focus(id(1)));
        split(&mut panes, Axis::Down, 4, (100, 30)).unwrap();
        assert_eq!(rect_of(&panes, (100, 30), 1).rows, 7);
        assert_eq!(rect_of(&panes, (100, 30), 4).rows, 7);
        assert_eq!(rect_of(&panes, (100, 30), 3).rows, 14);
        assert!(panes.set_focus(id(2)));
        assert!(panes.focus_dir(Dir::Left, (100, 30)));
        assert_eq!(panes.focused().0, id(3));
    }

    #[test]
    fn vertical_focus_crosses_the_divider_row() {
        let mut panes = three();
        assert!(panes.focus_dir(Dir::Up, (100, 30)));
        assert_eq!(panes.focused().0, id(2));
        assert!(panes.focus_dir(Dir::Down, (100, 30)));
        assert_eq!(panes.focused().0, id(3));
        assert!(panes.focus_dir(Dir::Left, (100, 30)));
        assert_eq!(panes.focused().0, id(1));
    }

    #[test]
    fn moving_focus_ends_zoom_and_failing_keeps_it() {
        let mut panes = two();
        assert!(panes.set_focus(id(1)));
        panes.toggle_zoom();
        assert!(!panes.focus_dir(Dir::Left, (100, 30)));
        assert_eq!(panes.zoomed(), Some(id(1)));
        assert!(panes.focus_dir(Dir::Right, (100, 30)));
        assert_eq!(panes.zoomed(), None);
        assert_eq!(panes.focused().0, id(2));
    }

    // 1.6 resize, drag, equalize

    #[test]
    fn resize_moves_the_divider_toward_the_direction() {
        let mut panes = two();
        assert!(panes.resize(Dir::Right, 2, (100, 30)));
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, 52);
        assert_eq!(rect_of(&panes, (100, 30), 2), rect(53, 0, 47, 30));
        assert!(panes.resize(Dir::Left, 5, (100, 30)));
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, 47);
        assert_tiles(&panes, (100, 30));
    }

    #[test]
    fn resize_needs_a_divider_on_that_axis() {
        let mut panes = two();
        let before = panes.rects((100, 30));
        assert!(!panes.resize(Dir::Up, 1, (100, 30)));
        assert!(!panes.resize(Dir::Down, 1, (100, 30)));
        assert!(!Panes::new(id(1), 1).resize(Dir::Right, 1, (100, 30)));
        assert_eq!(panes.rects((100, 30)), before);
    }

    #[test]
    fn resize_keeps_both_panes_at_the_minimum() {
        let mut panes = two();
        assert!(panes.resize(Dir::Right, 1000, (100, 30)));
        assert_eq!(rect_of(&panes, (100, 30), 2).cols, MIN_COLS);
        assert!(!panes.resize(Dir::Right, 1, (100, 30)));
        assert!(panes.resize(Dir::Left, 1000, (100, 30)));
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, MIN_COLS);
        assert!(!panes.resize(Dir::Left, 1, (100, 30)));
        assert_tiles(&panes, (100, 30));
    }

    #[test]
    fn resize_uses_the_innermost_divider_of_the_axis() {
        // 1 | (2 / 3), focus 3.
        let mut panes = three();
        assert!(panes.resize(Dir::Up, 3, (100, 30)));
        assert_eq!(rect_of(&panes, (100, 30), 2).rows, 12);
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, 50);
        // No Down divider is outside; Right reaches the root one.
        assert!(panes.resize(Dir::Right, 2, (100, 30)));
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, 52);
        assert_eq!(rect_of(&panes, (100, 30), 2).rows, 12);
    }

    #[test]
    fn resize_ends_zoom() {
        let mut panes = two();
        panes.toggle_zoom();
        assert!(panes.resize(Dir::Left, 1, (100, 30)));
        assert_eq!(panes.zoomed(), None);
    }

    #[test]
    fn drag_places_the_divider_at_the_pointer() {
        let mut panes = two();
        let path = panes.dividers((100, 30))[0].path.clone();
        panes.drag(&path, 52, (100, 30));
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, 52);
        assert_eq!(rect_of(&panes, (100, 30), 2), rect(53, 0, 47, 30));
        assert_eq!(panes.dividers((100, 30))[0].rect.col, 52);
        assert_tiles(&panes, (100, 30));
    }

    #[test]
    fn drag_clamps_to_the_minimums() {
        let mut panes = two();
        let path = panes.dividers((100, 30))[0].path.clone();
        panes.drag(&path, 0, (100, 30));
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, MIN_COLS);
        panes.drag(&path, 500, (100, 30));
        assert_eq!(rect_of(&panes, (100, 30), 2).cols, MIN_COLS);
    }

    #[test]
    fn drag_is_relative_to_the_split_it_moves() {
        // 1 | (2 | 3): the inner divider starts at col 51 + 25.
        let mut panes = two();
        split(&mut panes, Axis::Right, 3, (100, 30)).unwrap();
        let inner = panes
            .dividers((100, 30))
            .into_iter()
            .find(|d| d.path == vec![true])
            .unwrap();
        panes.drag(&inner.path, 80, (100, 30));
        assert_eq!(rect_of(&panes, (100, 30), 2), rect(51, 0, 29, 30));
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, 50);
        assert_tiles(&panes, (100, 30));
    }

    #[test]
    fn dragging_a_stale_path_changes_nothing() {
        let mut panes = two();
        let before = panes.rects((100, 30));
        panes.drag(&vec![true, false], 60, (100, 30));
        panes.drag(&vec![false], 60, (100, 30));
        assert_eq!(panes.rects((100, 30)), before);
    }

    #[test]
    fn equalize_weights_splits_by_leaf_count() {
        // 1 | (2 / 3): the left leaf gets a third of the width.
        let mut panes = three();
        panes.resize(Dir::Right, 20, (100, 30));
        panes.resize(Dir::Up, 5, (100, 30));
        panes.equalize();
        assert_eq!(rect_of(&panes, (100, 30), 1).cols, 33);
        let (r2, r3) = (rect_of(&panes, (100, 30), 2), rect_of(&panes, (100, 30), 3));
        assert!(r2.rows.abs_diff(r3.rows) <= 1);
        assert_tiles(&panes, (100, 30));
    }

    #[test]
    fn equalize_makes_a_row_of_panes_even() {
        let mut panes = two();
        split(&mut panes, Axis::Right, 3, (100, 30)).unwrap();
        split(&mut panes, Axis::Right, 4, (100, 30)).unwrap();
        panes.resize(Dir::Left, 10, (100, 30));
        panes.equalize();
        let widths: Vec<u16> = panes.rects((100, 30)).iter().map(|(_, r)| r.cols).collect();
        assert_eq!(widths.iter().sum::<u16>() + 3, 100);
        let (min, max) = (widths.iter().min().unwrap(), widths.iter().max().unwrap());
        assert!(max - min <= 1, "{widths:?}");
    }

    #[test]
    fn equalize_keeps_zoom() {
        let mut panes = two();
        panes.toggle_zoom();
        panes.equalize();
        assert_eq!(panes.zoomed(), Some(id(2)));
    }

    // 1.7 zoom, dividers, hit

    #[test]
    fn a_single_pane_cannot_zoom() {
        let mut panes = Panes::new(id(1), 1);
        panes.toggle_zoom();
        assert_eq!(panes.zoomed(), None);
    }

    #[test]
    fn zoom_fills_the_area_and_unzoom_restores_the_sizes() {
        let mut panes = two();
        assert!(panes.set_focus(id(1)));
        let before = panes.rects((100, 30));
        panes.toggle_zoom();
        assert_eq!(panes.zoomed(), Some(id(1)));
        assert_eq!(panes.rects((100, 30)), vec![(id(1), rect(0, 0, 100, 30))]);
        assert!(panes.dividers((100, 30)).is_empty());
        panes.toggle_zoom();
        assert_eq!(panes.rects((100, 30)), before);
        assert_eq!(panes.dividers((100, 30)).len(), 1);
    }

    #[test]
    fn zoom_keeps_hidden_panes_in_the_tree() {
        let mut panes = two();
        panes.toggle_zoom();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes.get(id(1)), Some(&1));
    }

    #[test]
    fn dividers_sit_between_the_halves() {
        let panes = three();
        let dividers = panes.dividers((100, 30));
        assert_eq!(dividers.len(), 2);
        assert!(dividers.contains(&Divider {
            rect: rect(50, 0, 1, 30),
            axis: Axis::Right,
            path: vec![],
        }));
        assert!(dividers.contains(&Divider {
            rect: rect(51, 15, 49, 1),
            axis: Axis::Down,
            path: vec![true],
        }));
    }

    #[test]
    fn hit_finds_panes_and_dividers() {
        let panes = three();
        let area = (100, 30);
        assert_eq!(panes.hit(area, 0, 0), Some(Hit::Pane(id(1))));
        assert_eq!(panes.hit(area, 49, 29), Some(Hit::Pane(id(1))));
        assert_eq!(panes.hit(area, 51, 0), Some(Hit::Pane(id(2))));
        assert_eq!(panes.hit(area, 99, 14), Some(Hit::Pane(id(2))));
        assert_eq!(panes.hit(area, 51, 16), Some(Hit::Pane(id(3))));
        assert_eq!(panes.hit(area, 50, 10), Some(Hit::Divider(vec![])));
        assert_eq!(panes.hit(area, 70, 15), Some(Hit::Divider(vec![true])));
    }

    #[test]
    fn hit_outside_the_area_is_none() {
        let panes = three();
        assert_eq!(panes.hit((100, 30), 100, 0), None);
        assert_eq!(panes.hit((100, 30), 0, 30), None);
        assert_eq!(Panes::new(id(1), 1).hit((80, 24), 80, 5), None);
    }

    #[test]
    fn hit_while_zoomed_sees_only_the_zoomed_pane() {
        let mut panes = two();
        panes.toggle_zoom();
        assert_eq!(panes.hit((100, 30), 10, 10), Some(Hit::Pane(id(2))));
        assert_eq!(panes.hit((100, 30), 50, 10), Some(Hit::Pane(id(2))));
    }
}

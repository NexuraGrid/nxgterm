//! A tab's panes and how events find them.
//!
//! Generic over what a pane holds, so routing is tested without spawning
//! shells. Pane ids are global (every tab draws from one counter) and never
//! reused, so output from the threads of a pane that already closed is
//! recognized and dropped.

use nxg_core::TermSize;
use nxg_render::Layout;

use crate::bindings::Action;
use crate::mouse::cell_at;
use crate::panes::{Axis, CellRect, Closed, Dir, DividerPath, Hit, PaneId, Panes, SplitError};
use crate::tabs::Tabs;

/// Hands out pane ids, each once.
#[derive(Debug, Default)]
pub struct PaneIds {
    next: u64,
}

impl PaneIds {
    pub fn next(&mut self) -> PaneId {
        let id = PaneId::new(self.next);
        self.next += 1;
        id
    }
}

/// One tab: a tree of panes, one of them focused.
#[derive(Debug)]
pub struct Tab<T> {
    pub panes: Panes<T>,
}

impl<T> Tab<T> {
    /// A tab with a single pane.
    pub fn new(id: PaneId, pane: T) -> Self {
        Self {
            panes: Panes::new(id, pane),
        }
    }

    /// The pane that takes the keys.
    pub fn focused(&self) -> &T {
        self.panes.focused().1
    }

    pub fn focused_mut(&mut self) -> &mut T {
        let (id, _) = self.panes.focused();
        self.panes.get_mut(id).expect("focus is always a leaf")
    }

    /// Runs `f` on every pane.
    pub fn each_mut(&mut self, mut f: impl FnMut(&mut T)) {
        let ids: Vec<PaneId> = self.panes.iter().map(|(id, _)| id).collect();
        for id in ids {
            if let Some(pane) = self.panes.get_mut(id) {
                f(pane);
            }
        }
    }
}

/// A pane of the visible layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub id: PaneId,
    pub rect: CellRect,
    pub focused: bool,
}

/// The panes drawn (and sized) in a content area of `area`: all of them,
/// or only the zoomed one.
pub fn placements<T>(tab: &Tab<T>, area: TermSize) -> Vec<Placement> {
    let focus = tab.panes.focused().0;
    tab.panes
        .rects((area.cols(), area.rows()))
        .into_iter()
        .map(|(id, rect)| Placement {
            id,
            rect,
            focused: id == focus,
        })
        .collect()
}

/// How far a pane fades: not at all with focus, `inactive` without.
pub fn dim_of(focused: bool, inactive: f32) -> f32 {
    if focused { 0.0 } else { inactive }
}

/// The focused pane of the active tab: where input, paste and the
/// pointer go. Free functions (not a method of the session) so callers
/// keep borrowing the other fields of the session.
pub fn focused<T>(tabs: &Tabs<Tab<T>>) -> Option<&T> {
    tabs.active().map(Tab::focused)
}

pub fn focused_mut<T>(tabs: &mut Tabs<Tab<T>>) -> Option<&mut T> {
    tabs.active_mut().map(Tab::focused_mut)
}

/// Position of the tab that holds pane `id`.
fn tab_of<T>(tabs: &Tabs<Tab<T>>, id: PaneId) -> Option<usize> {
    tabs.iter().position(|tab| tab.panes.contains(id))
}

/// The pane `id`, wherever it is; `None` once it closed.
pub fn pane_mut<T>(tabs: &mut Tabs<Tab<T>>, id: PaneId) -> Option<&mut T> {
    tabs.iter_mut().find_map(|tab| tab.panes.get_mut(id))
}

/// Whether output of pane `id` changes what is on screen: its tab is the
/// active one and zoom does not hide it. `false` for a closed pane.
pub fn shows<T>(tabs: &Tabs<Tab<T>>, id: PaneId) -> bool {
    tab_of(tabs, id) == Some(tabs.active_index())
        && tabs
            .active()
            .is_some_and(|tab| tab.panes.zoomed().is_none_or(|zoomed| zoomed == id))
}

/// What the exit of a pane did.
#[derive(Debug, PartialEq, Eq)]
pub enum Exit {
    /// No such pane (already closed): nothing changed.
    Stale,
    /// The pane left its tab, which keeps the others.
    Pane,
    /// It was the last pane: the tab closed with it.
    Tab,
}

/// Closes pane `id` after its child exited. The caller exits the app when
/// no tab is left.
pub fn exit<T>(tabs: &mut Tabs<Tab<T>>, id: PaneId) -> Exit {
    let Some(index) = tab_of(tabs, id) else {
        return Exit::Stale;
    };
    let Some(tab) = tabs.iter_mut().nth(index) else {
        return Exit::Stale;
    };
    match tab.panes.close(id) {
        Closed::Removed(_) => Exit::Pane,
        Closed::Last => {
            tabs.close(index);
            Exit::Tab
        }
        Closed::Unknown => Exit::Stale,
    }
}

/// What a pane action does to the active tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneCommand {
    Split(Axis),
    Focus(Dir),
    Resize(Dir),
    Close,
    Zoom,
    Equalize,
}

/// The pane command `action` stands for.
pub fn command(action: Action) -> Option<PaneCommand> {
    Some(match action {
        Action::SplitRight => PaneCommand::Split(Axis::Right),
        Action::SplitDown => PaneCommand::Split(Axis::Down),
        Action::FocusPaneLeft => PaneCommand::Focus(Dir::Left),
        Action::FocusPaneRight => PaneCommand::Focus(Dir::Right),
        Action::FocusPaneUp => PaneCommand::Focus(Dir::Up),
        Action::FocusPaneDown => PaneCommand::Focus(Dir::Down),
        Action::ResizePaneLeft => PaneCommand::Resize(Dir::Left),
        Action::ResizePaneRight => PaneCommand::Resize(Dir::Right),
        Action::ResizePaneUp => PaneCommand::Resize(Dir::Up),
        Action::ResizePaneDown => PaneCommand::Resize(Dir::Down),
        Action::ClosePane => PaneCommand::Close,
        Action::ZoomPane => PaneCommand::Zoom,
        Action::EqualizePanes => PaneCommand::Equalize,
        _ => return None,
    })
}

/// Cells a divider moves per resize key press: two columns (cells are
/// about twice as tall as wide) or one row.
pub fn resize_step(dir: Dir) -> u16 {
    match dir {
        Dir::Left | Dir::Right => 2,
        Dir::Up | Dir::Down => 1,
    }
}

/// Splits the focused pane of the active tab (`None` without a tab); see
/// [`Panes::split_with`].
pub fn split_active<T, E>(
    tabs: &mut Tabs<Tab<T>>,
    axis: Axis,
    id: PaneId,
    area: TermSize,
    make: impl FnOnce(CellRect) -> Result<T, E>,
) -> Option<Result<(), SplitError<E>>> {
    let tab = tabs.active_mut()?;
    Some(
        tab.panes
            .split_with(axis, id, (area.cols(), area.rows()), make),
    )
}

/// Moves focus to the neighbour of the active tab in `dir`.
pub fn focus_active<T>(tabs: &mut Tabs<Tab<T>>, dir: Dir, area: TermSize) -> bool {
    let area = (area.cols(), area.rows());
    tabs.active_mut()
        .is_some_and(|tab| tab.panes.focus_dir(dir, area))
}

/// Moves the divider next to the focused pane of the active tab in `dir`.
pub fn resize_active<T>(tabs: &mut Tabs<Tab<T>>, dir: Dir, area: TermSize) -> bool {
    let area = (area.cols(), area.rows());
    tabs.active_mut()
        .is_some_and(|tab| tab.panes.resize(dir, resize_step(dir), area))
}

/// Zooms the focused pane of the active tab, or restores the layout;
/// `false` when there is nothing to zoom (no tab, or a single pane).
pub fn zoom_active<T>(tabs: &mut Tabs<Tab<T>>) -> bool {
    let Some(tab) = tabs.active_mut() else {
        return false;
    };
    let before = tab.panes.zoomed();
    tab.panes.toggle_zoom();
    tab.panes.zoomed() != before
}

/// Gives the panes of the active tab equal shares; whether any moved.
pub fn equalize_active<T>(tabs: &mut Tabs<Tab<T>>, area: TermSize) -> bool {
    let area = (area.cols(), area.rows());
    let Some(tab) = tabs.active_mut() else {
        return false;
    };
    let before = tab.panes.rects(area);
    tab.panes.equalize();
    tab.panes.rects(area) != before
}

/// What lies under the pointer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Under {
    Pane(PaneId),
    /// A divider, by its path in the tree, and the way it runs.
    Divider(DividerPath, Axis),
    Nothing,
}

/// What `tab` shows at window pixel `x`, `y`. Pixels outside the cells
/// (the padding) count as the nearest cell.
pub fn under<T>(tab: &Tab<T>, layout: Layout, area: TermSize, x: f64, y: f64) -> Under {
    let (col, row) = cell_at(layout, area, x, y);
    let area = (area.cols(), area.rows());
    match tab.panes.hit(area, col, row) {
        Some(Hit::Pane(id)) => Under::Pane(id),
        Some(Hit::Divider(path)) => match divider_axis(tab, area, &path) {
            Some(axis) => Under::Divider(path, axis),
            None => Under::Nothing,
        },
        None => Under::Nothing,
    }
}

fn divider_axis<T>(tab: &Tab<T>, area: (u16, u16), path: &DividerPath) -> Option<Axis> {
    let dividers = tab.panes.dividers(area);
    dividers.iter().find(|d| d.path == *path).map(|d| d.axis)
}

/// Where pane `id` sits in the window and how many cells it has, to map
/// pointer pixels to its own cells with [`cell_at`]. `None` for a pane that
/// is closed or hidden by zoom.
pub fn frame<T>(
    tab: &Tab<T>,
    layout: Layout,
    area: TermSize,
    id: PaneId,
) -> Option<(Layout, TermSize)> {
    let rects = tab.panes.rects((area.cols(), area.rows()));
    let (_, rect) = rects.into_iter().find(|(pane, _)| *pane == id)?;
    let size = TermSize::new(rect.cols, rect.rows).ok()?;
    Some((layout.at(rect.col, rect.row), size))
}

/// Gives focus to pane `id` of the active tab; whether it moved.
pub fn focus_pane<T>(tabs: &mut Tabs<Tab<T>>, id: PaneId) -> bool {
    let Some(tab) = tabs.active_mut() else {
        return false;
    };
    tab.panes.focused().0 != id && tab.panes.set_focus(id)
}

/// Moves `divider` of the active tab under the pointer at window pixel
/// `x`, `y`; whether any pane changed size.
pub fn drag_divider<T>(
    tabs: &mut Tabs<Tab<T>>,
    layout: Layout,
    area: TermSize,
    divider: &DividerPath,
    x: f64,
    y: f64,
) -> bool {
    let cells = (area.cols(), area.rows());
    let Some(tab) = tabs.active_mut() else {
        return false;
    };
    let Some(axis) = divider_axis(tab, cells, divider) else {
        return false;
    };
    let (col, row) = cell_at(layout, area, x, y);
    let before = tab.panes.rects(cells);
    let pos = match axis {
        Axis::Right => col,
        Axis::Down => row,
    };
    tab.panes.drag(divider, pos, cells);
    tab.panes.rects(cells) != before
}

/// Closes the focused pane of the active tab like an exit of its child
/// would; the caller exits the app when no tab is left.
pub fn close_focused<T>(tabs: &mut Tabs<Tab<T>>) -> Exit {
    match tabs.active().map(|tab| tab.panes.focused().0) {
        Some(id) => exit(tabs, id),
        None => Exit::Stale,
    }
}

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use nxg_render::CellSize;

    use super::*;
    use crate::panes::{Axis, Dir, SplitError};

    const AREA: (u16, u16) = (100, 30);

    fn size() -> TermSize {
        TermSize::new(AREA.0, AREA.1).unwrap()
    }

    /// Opens a one-pane tab holding `value`.
    fn open(tabs: &mut Tabs<Tab<u32>>, ids: &mut PaneIds, value: u32) -> PaneId {
        let id = ids.next();
        tabs.add_with(|_| Ok::<_, Infallible>(Tab::new(id, value)))
            .unwrap();
        id
    }

    /// Splits the focused pane of the active tab; the new pane holds `value`.
    fn split(tabs: &mut Tabs<Tab<u32>>, ids: &mut PaneIds, value: u32) -> PaneId {
        let id = ids.next();
        let tab = tabs.active_mut().unwrap();
        tab.panes
            .split_with(Axis::Right, id, AREA, |_| Ok::<_, Infallible>(value))
            .unwrap();
        id
    }

    #[test]
    fn pane_ids_are_never_repeated() {
        let mut ids = PaneIds::default();
        let first = ids.next();
        let second = ids.next();
        let third = ids.next();
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
    }

    #[test]
    fn the_focused_pane_is_the_one_with_focus() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        open(&mut tabs, &mut ids, 1);
        split(&mut tabs, &mut ids, 2);
        let tab = tabs.active_mut().unwrap();
        assert_eq!(*tab.focused(), 2, "a split focuses the new pane");
        *tab.focused_mut() = 9;
        assert_eq!(*tab.focused(), 9);
        let first = tab.panes.iter().next().unwrap();
        assert_eq!(*first.1, 1, "the other pane is untouched");
    }

    #[test]
    fn one_pane_fills_the_content_area_and_has_focus() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let id = open(&mut tabs, &mut ids, 1);
        let placed = placements(tabs.active().unwrap(), size());
        assert_eq!(
            placed,
            vec![Placement {
                id,
                rect: CellRect {
                    col: 0,
                    row: 0,
                    cols: 100,
                    rows: 30
                },
                focused: true,
            }]
        );
    }

    #[test]
    fn placements_mark_only_the_focused_pane_and_follow_zoom() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let first = open(&mut tabs, &mut ids, 1);
        let second = split(&mut tabs, &mut ids, 2);
        let placed = placements(tabs.active().unwrap(), size());
        assert_eq!(placed.len(), 2);
        assert_eq!(placed[0].id, first);
        assert!(!placed[0].focused);
        assert_eq!(placed[1].id, second);
        assert!(placed[1].focused);
        assert_eq!(placed[0].rect.cols + 1 + placed[1].rect.cols, 100);

        tabs.active_mut().unwrap().panes.toggle_zoom();
        let zoomed = placements(tabs.active().unwrap(), size());
        assert_eq!(zoomed.len(), 1);
        assert_eq!(zoomed[0].id, second);
        assert_eq!(zoomed[0].rect.cols, 100);
    }

    #[test]
    fn pane_mut_finds_a_pane_in_any_tab_and_drops_unknown_ids() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let a = open(&mut tabs, &mut ids, 10);
        let b = open(&mut tabs, &mut ids, 20);
        *pane_mut(&mut tabs, a).unwrap() += 1;
        *pane_mut(&mut tabs, b).unwrap() += 2;
        assert_eq!(*pane_mut(&mut tabs, a).unwrap(), 11);
        assert_eq!(*pane_mut(&mut tabs, b).unwrap(), 22);
        assert!(pane_mut(&mut tabs, ids.next()).is_none());
    }

    #[test]
    fn output_shows_only_for_a_visible_pane_of_the_active_tab() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let background = open(&mut tabs, &mut ids, 1);
        let active = open(&mut tabs, &mut ids, 2);
        assert!(shows(&tabs, active));
        assert!(!shows(&tabs, background), "another tab is not drawn");
        assert!(!shows(&tabs, ids.next()), "an unknown pane is dropped");
        tabs.select(0);
        assert!(shows(&tabs, background));
        assert!(!shows(&tabs, active));
    }

    #[test]
    fn output_of_a_pane_hidden_by_zoom_does_not_show() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let first = open(&mut tabs, &mut ids, 1);
        let second = split(&mut tabs, &mut ids, 2);
        assert!(shows(&tabs, first));
        assert!(shows(&tabs, second));
        tabs.active_mut().unwrap().panes.toggle_zoom();
        assert!(!shows(&tabs, first));
        assert!(shows(&tabs, second));
    }

    #[test]
    fn the_exit_of_the_last_pane_closes_its_tab() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let first = open(&mut tabs, &mut ids, 1);
        let second = open(&mut tabs, &mut ids, 2);
        assert_eq!(exit(&mut tabs, second), Exit::Tab);
        assert_eq!(tabs.len(), 1);
        assert!(!tabs.is_empty(), "another tab is left");
        assert_eq!(exit(&mut tabs, first), Exit::Tab);
        assert!(tabs.is_empty(), "the last tab closed: the app exits");
    }

    #[test]
    fn the_exit_of_one_of_two_panes_keeps_the_tab() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let first = open(&mut tabs, &mut ids, 1);
        let second = split(&mut tabs, &mut ids, 2);
        assert_eq!(exit(&mut tabs, second), Exit::Pane);
        assert_eq!(tabs.len(), 1);
        let tab = tabs.active().unwrap();
        assert_eq!(tab.panes.len(), 1);
        assert_eq!(tab.panes.focused().0, first, "focus moves to the sibling");
    }

    #[test]
    fn the_exit_of_a_stale_pane_changes_nothing() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let only = open(&mut tabs, &mut ids, 1);
        let gone = ids.next();
        assert_eq!(exit(&mut tabs, gone), Exit::Stale);
        assert_eq!(tabs.len(), 1);
        assert_eq!(exit(&mut tabs, only), Exit::Tab);
        assert_eq!(
            exit(&mut tabs, only),
            Exit::Stale,
            "a second exit is dropped"
        );
    }
    #[test]
    fn the_active_tab_gives_its_focused_pane() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        assert!(focused(&tabs).is_none(), "no tab, no pane");
        assert!(focused_mut(&mut tabs).is_none());
        open(&mut tabs, &mut ids, 1);
        open(&mut tabs, &mut ids, 2);
        split(&mut tabs, &mut ids, 3);
        assert_eq!(focused(&tabs), Some(&3));
        *focused_mut(&mut tabs).unwrap() = 4;
        assert_eq!(focused(&tabs), Some(&4));
        tabs.select(0);
        assert_eq!(focused(&tabs), Some(&1), "each tab has its own focus");
    }

    #[test]
    fn each_mut_visits_every_pane_of_the_tab() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        open(&mut tabs, &mut ids, 1);
        split(&mut tabs, &mut ids, 2);
        split(&mut tabs, &mut ids, 3);
        let tab = tabs.active_mut().unwrap();
        tab.each_mut(|value| *value *= 10);
        let values: Vec<u32> = tab.panes.iter().map(|(_, value)| *value).collect();
        assert_eq!(values.len(), 3);
        assert!(values.contains(&10) && values.contains(&20) && values.contains(&30));
    }

    #[test]
    fn actions_map_to_pane_commands() {
        use nxg_config::keybindings::Action;
        let map = |a| command(a);
        assert_eq!(
            map(Action::SplitRight),
            Some(PaneCommand::Split(Axis::Right))
        );
        assert_eq!(map(Action::SplitDown), Some(PaneCommand::Split(Axis::Down)));
        assert_eq!(
            map(Action::FocusPaneLeft),
            Some(PaneCommand::Focus(Dir::Left))
        );
        assert_eq!(
            map(Action::FocusPaneDown),
            Some(PaneCommand::Focus(Dir::Down))
        );
        assert_eq!(
            map(Action::ResizePaneRight),
            Some(PaneCommand::Resize(Dir::Right))
        );
        assert_eq!(
            map(Action::ResizePaneUp),
            Some(PaneCommand::Resize(Dir::Up))
        );
        assert_eq!(map(Action::ClosePane), Some(PaneCommand::Close));
        assert_eq!(map(Action::NewTab), None, "not a pane action");
        assert_eq!(map(Action::ZoomPane), Some(PaneCommand::Zoom));
        assert_eq!(map(Action::EqualizePanes), Some(PaneCommand::Equalize));
    }

    #[test]
    fn zoom_fills_the_area_and_the_layout_returns_with_the_same_sizes() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        open(&mut tabs, &mut ids, 1);
        split(&mut tabs, &mut ids, 2);
        let before = placements(tabs.active().unwrap(), size());
        assert!(zoom_active(&mut tabs));
        let zoomed = placements(tabs.active().unwrap(), size());
        assert_eq!(zoomed.len(), 1);
        assert_eq!(zoomed[0].rect.cols, 100);
        assert!(zoom_active(&mut tabs), "the second toggle restores");
        assert_eq!(placements(tabs.active().unwrap(), size()), before);
    }

    #[test]
    fn a_single_pane_has_nothing_to_zoom() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        open(&mut tabs, &mut ids, 1);
        assert!(!zoom_active(&mut tabs));
        assert!(!zoom_active(&mut Tabs::<Tab<u32>>::new()), "no tab");
    }

    #[test]
    fn equalize_undoes_a_resize_and_reports_whether_anything_moved() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        open(&mut tabs, &mut ids, 1);
        split(&mut tabs, &mut ids, 2);
        let equal = placements(tabs.active().unwrap(), size());
        assert!(!equalize_active(&mut tabs, size()), "already equal");
        assert!(resize_active(&mut tabs, Dir::Right, size()));
        assert_ne!(placements(tabs.active().unwrap(), size()), equal);
        assert!(equalize_active(&mut tabs, size()));
        assert_eq!(placements(tabs.active().unwrap(), size()), equal);
    }

    #[test]
    fn resize_moves_two_columns_or_one_row() {
        assert_eq!(resize_step(Dir::Left), 2);
        assert_eq!(resize_step(Dir::Right), 2);
        assert_eq!(resize_step(Dir::Up), 1);
        assert_eq!(resize_step(Dir::Down), 1);
    }

    #[test]
    fn a_split_adds_and_focuses_a_pane_of_the_active_tab_only() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        open(&mut tabs, &mut ids, 1);
        open(&mut tabs, &mut ids, 2);
        let id = ids.next();
        let rect = std::cell::Cell::new(None);
        let done = split_active(&mut tabs, Axis::Right, id, size(), |r| {
            rect.set(Some(r));
            Ok::<_, Infallible>(3)
        });
        assert!(matches!(done, Some(Ok(()))));
        let tab = tabs.active().unwrap();
        assert_eq!(tab.panes.len(), 2);
        assert_eq!(tab.panes.focused().0, id);
        assert_eq!(
            rect.get().unwrap().rows,
            30,
            "the factory sees the new rect"
        );
        tabs.select(0);
        assert_eq!(tabs.active().unwrap().panes.len(), 1, "other tab untouched");
    }

    #[test]
    fn a_refused_or_failed_split_changes_nothing() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let first = open(&mut tabs, &mut ids, 1);
        let tiny = TermSize::new(5, 30).unwrap();
        let id = ids.next();
        let done = split_active(&mut tabs, Axis::Right, id, tiny, |_| Ok::<_, Infallible>(2));
        assert!(matches!(done, Some(Err(SplitError::TooSmall))));
        let done = split_active(&mut tabs, Axis::Down, id, size(), |_| Err("no shell"));
        assert!(matches!(done, Some(Err(SplitError::Make("no shell")))));
        let tab = tabs.active().unwrap();
        assert_eq!(tab.panes.len(), 1);
        assert_eq!(tab.panes.focused().0, first);
        let mut none: Tabs<Tab<u32>> = Tabs::new();
        let done = split_active(&mut none, Axis::Right, id, size(), |_| {
            Ok::<_, Infallible>(1)
        });
        assert!(done.is_none(), "no tab, no split");
    }

    #[test]
    fn focus_and_resize_act_on_the_active_tab() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let first = open(&mut tabs, &mut ids, 1);
        let second = split(&mut tabs, &mut ids, 2);
        assert!(!focus_active(&mut tabs, Dir::Right, size()), "no neighbour");
        assert!(focus_active(&mut tabs, Dir::Left, size()));
        assert_eq!(tabs.active().unwrap().panes.focused().0, first);
        let width = |tabs: &Tabs<Tab<u32>>| placements(tabs.active().unwrap(), size())[0].rect.cols;
        let before = width(&tabs);
        assert!(resize_active(&mut tabs, Dir::Right, size()));
        assert_eq!(width(&tabs), before + 2);
        assert!(
            !resize_active(&mut tabs, Dir::Down, size()),
            "no horizontal divider"
        );
        assert!(focus_active(&mut tabs, Dir::Right, size()));
        assert_eq!(tabs.active().unwrap().panes.focused().0, second);
        let mut none: Tabs<Tab<u32>> = Tabs::new();
        assert!(!focus_active(&mut none, Dir::Left, size()));
        assert!(!resize_active(&mut none, Dir::Left, size()));
    }

    #[test]
    fn closing_the_focused_pane_follows_the_exit_rules() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let first = open(&mut tabs, &mut ids, 1);
        open(&mut tabs, &mut ids, 2);
        split(&mut tabs, &mut ids, 3);
        assert_eq!(close_focused(&mut tabs), Exit::Pane);
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            close_focused(&mut tabs),
            Exit::Tab,
            "last pane closes the tab"
        );
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.active().unwrap().panes.focused().0, first);
        assert_eq!(close_focused(&mut tabs), Exit::Tab);
        assert!(tabs.is_empty(), "last tab: the app exits");
        assert_eq!(close_focused(&mut tabs), Exit::Stale);
    }

    #[test]
    fn only_panes_without_focus_dim() {
        assert_eq!(dim_of(true, 0.25), 0.0);
        assert_eq!(dim_of(false, 0.25), 0.25);
        assert_eq!(dim_of(false, 0.0), 0.0, "0 disables dimming");
    }

    fn px_layout() -> Layout {
        Layout {
            cell: CellSize {
                width: 10,
                height: 20,
            },
            padding: 0,
            left: 0,
            top: 0,
        }
    }

    /// The window pixel at the middle of cell (`col`, `row`).
    fn at(col: u16, row: u16) -> (f64, f64) {
        (f64::from(col) * 10.0 + 5.0, f64::from(row) * 20.0 + 10.0)
    }

    fn under_at(tabs: &Tabs<Tab<u32>>, col: u16, row: u16) -> Under {
        let (x, y) = at(col, row);
        under(tabs.active().unwrap(), px_layout(), size(), x, y)
    }

    #[test]
    fn the_pointer_is_over_a_pane_a_divider_or_the_pane_that_zoom_left() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let a = open(&mut tabs, &mut ids, 1);
        let b = split(&mut tabs, &mut ids, 2);
        // A takes columns 0-49, the divider is column 50, B takes 51-99.
        assert_eq!(under_at(&tabs, 10, 5), Under::Pane(a));
        assert_eq!(under_at(&tabs, 80, 5), Under::Pane(b));
        assert_eq!(under_at(&tabs, 50, 5), Under::Divider(vec![], Axis::Right));
        tabs.active_mut().unwrap().panes.toggle_zoom();
        assert_eq!(under_at(&tabs, 10, 5), Under::Pane(b), "B fills the area");
    }

    #[test]
    fn a_press_focuses_the_pane_under_it_once() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let a = open(&mut tabs, &mut ids, 1);
        let b = split(&mut tabs, &mut ids, 2);
        assert!(focus_active(&mut tabs, Dir::Left, size()));
        assert_eq!(*tabs.active().unwrap().focused(), 1);
        assert!(focus_pane(&mut tabs, b), "A had focus");
        assert_eq!(*tabs.active().unwrap().focused(), 2);
        assert!(!focus_pane(&mut tabs, b), "already focused");
        assert!(focus_pane(&mut tabs, a));
        assert!(!focus_pane(&mut tabs, ids.next()), "unknown pane");
    }

    #[test]
    fn pane_cells_are_relative_to_the_pane_and_clamped_to_it() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        let a = open(&mut tabs, &mut ids, 1);
        let b = split(&mut tabs, &mut ids, 2);
        let tab = tabs.active().unwrap();
        let cell = |id, col, row| {
            let (pane_layout, pane_size) = frame(tab, px_layout(), size(), id).unwrap();
            let (x, y) = at(col, row);
            crate::mouse::cell_at(pane_layout, pane_size, x, y)
        };
        assert_eq!(cell(b, 80, 5), (29, 5), "column 51 is B's column 0");
        assert_eq!(cell(a, 10, 5), (10, 5));
        assert_eq!(cell(a, 80, 5), (49, 5), "a drag from A stops at its edge");
        assert_eq!(cell(b, 10, 5), (0, 5), "and from B at its own");
        assert!(frame(tab, px_layout(), size(), ids.next()).is_none());
    }

    #[test]
    fn dragging_a_divider_two_cells_right_widens_the_first_pane() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        open(&mut tabs, &mut ids, 1);
        split(&mut tabs, &mut ids, 2);
        let cols = |tabs: &Tabs<Tab<u32>>| -> Vec<u16> {
            placements(tabs.active().unwrap(), size())
                .iter()
                .map(|p| p.rect.cols)
                .collect()
        };
        assert_eq!(cols(&tabs), [50, 49]);
        let drag = |tabs: &mut Tabs<Tab<u32>>, col, row| {
            let (x, y) = at(col, row);
            drag_divider(tabs, px_layout(), size(), &Vec::new(), x, y)
        };
        assert!(drag(&mut tabs, 52, 5));
        assert_eq!(cols(&tabs), [52, 47]);
        assert!(!drag(&mut tabs, 52, 9), "same column: nothing moved");
        assert!(drag(&mut tabs, 99, 5));
        assert_eq!(cols(&tabs), [95, 4], "the second pane keeps its minimum");
        assert!(!drag(&mut Tabs::<Tab<u32>>::new(), 52, 5), "no tab");
    }

    #[test]
    fn a_stacked_divider_follows_the_row_of_the_pointer() {
        let (mut tabs, mut ids) = (Tabs::new(), PaneIds::default());
        open(&mut tabs, &mut ids, 1);
        let id = ids.next();
        let tab = tabs.active_mut().unwrap();
        tab.panes
            .split_with(Axis::Down, id, AREA, |_| Ok::<_, Infallible>(2))
            .unwrap();
        // 30 rows: A takes 0-14, the divider is row 15.
        assert_eq!(under_at(&tabs, 5, 15), Under::Divider(vec![], Axis::Down));
        let (x, y) = at(5, 10);
        assert!(drag_divider(
            &mut tabs,
            px_layout(),
            size(),
            &Vec::new(),
            x,
            y
        ));
        let rows: Vec<u16> = placements(tabs.active().unwrap(), size())
            .iter()
            .map(|p| p.rect.rows)
            .collect();
        assert_eq!(rows, [10, 19]);
    }
}

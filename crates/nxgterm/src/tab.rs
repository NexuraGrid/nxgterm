//! A tab's panes and how events find them.
//!
//! Generic over what a pane holds, so routing is tested without spawning
//! shells. Pane ids are global (every tab draws from one counter) and never
//! reused, so output from the threads of a pane that already closed is
//! recognized and dropped.

use nxg_core::TermSize;

use crate::panes::{CellRect, Closed, PaneId, Panes};
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

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use super::*;
    use crate::panes::Axis;

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
}

//! The open tabs: their order, which one is active and where new ones go.
//!
//! Generic over what a tab holds, so the logic is tested without spawning
//! shells. Each tab gets a [`TabId`] that is never reused, so events from
//! a tab's background threads that arrive after it closed are recognized
//! and dropped.

/// Identifies a tab for its whole life, unlike its position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(u64);

/// Tabs in display order with one active (none while empty).
#[derive(Debug)]
pub struct Tabs<T> {
    tabs: Vec<(TabId, T)>,
    active: usize,
    next_id: u64,
}

impl<T> Default for Tabs<T> {
    fn default() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
        }
    }
}

impl<T> Tabs<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a tab right after the active one (first when empty) and
    /// activates it. `make` builds the tab knowing its id; when it fails
    /// nothing changes.
    pub fn add_with<E>(&mut self, make: impl FnOnce(TabId) -> Result<T, E>) -> Result<TabId, E> {
        let id = TabId(self.next_id);
        let tab = make(id)?;
        self.next_id += 1;
        let at = if self.tabs.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.tabs.insert(at, (id, tab));
        self.active = at;
        Ok(id)
    }

    /// Closes the tab at `index`, returning it. Closing the active tab
    /// activates its right neighbour, or the left one when it was last.
    pub fn close(&mut self, index: usize) -> Option<T> {
        if index >= self.tabs.len() {
            return None;
        }
        let (_, tab) = self.tabs.remove(index);
        if index < self.active || self.active >= self.tabs.len() {
            self.active = self.active.saturating_sub(1);
        }
        Some(tab)
    }

    /// Position of the tab `id`, if it is still open.
    pub fn index_of(&self, id: TabId) -> Option<usize> {
        self.tabs.iter().position(|(tab, _)| *tab == id)
    }

    pub fn get_mut(&mut self, id: TabId) -> Option<&mut T> {
        let index = self.index_of(id)?;
        Some(&mut self.tabs[index].1)
    }

    /// Position of the active tab (0 while empty).
    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> Option<&T> {
        self.tabs.get(self.active).map(|(_, tab)| tab)
    }

    pub fn active_mut(&mut self) -> Option<&mut T> {
        self.tabs.get_mut(self.active).map(|(_, tab)| tab)
    }

    /// Activates the tab at `index`; returns whether it exists.
    pub fn select(&mut self, index: usize) -> bool {
        let exists = index < self.tabs.len();
        if exists {
            self.active = index;
        }
        exists
    }

    /// Activates the next tab, wrapping to the first.
    pub fn next(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + 1) % self.tabs.len();
        }
    }

    /// Activates the previous tab, wrapping to the last.
    pub fn previous(&mut self) {
        if !self.tabs.is_empty() {
            self.active = (self.active + self.tabs.len() - 1) % self.tabs.len();
        }
    }

    /// Activates tab `n`, counting from 1; past the last tab it activates
    /// the last one (as Alt+9 does in most terminals and browsers).
    pub fn goto(&mut self, n: u8) {
        if !self.tabs.is_empty() {
            let index = usize::from(n.max(1)) - 1;
            self.active = index.min(self.tabs.len() - 1);
        }
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// The tabs in display order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.tabs.iter().map(|(_, tab)| tab)
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.tabs.iter_mut().map(|(_, tab)| tab)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::convert::Infallible;

    fn add(tabs: &mut Tabs<&'static str>, name: &'static str) -> TabId {
        tabs.add_with(|_| Ok::<_, Infallible>(name)).unwrap()
    }

    fn tabs(names: &[&'static str]) -> Tabs<&'static str> {
        let mut tabs = Tabs::new();
        for name in names {
            add(&mut tabs, name);
        }
        tabs
    }

    fn order(tabs: &Tabs<&'static str>) -> Vec<&'static str> {
        tabs.iter().copied().collect()
    }

    #[test]
    fn starts_empty_with_nothing_active() {
        let tabs: Tabs<()> = Tabs::new();
        assert!(tabs.is_empty());
        assert_eq!(tabs.active(), None);
    }

    #[test]
    fn new_tabs_open_after_the_active_one_and_become_active() {
        let mut tabs = tabs(&["a", "b", "c"]);
        assert_eq!(order(&tabs), ["a", "b", "c"]);
        assert_eq!(tabs.active(), Some(&"c"));
        tabs.goto(1);
        add(&mut tabs, "d");
        assert_eq!(order(&tabs), ["a", "d", "b", "c"]);
        assert_eq!((tabs.active_index(), tabs.active()), (1, Some(&"d")));
    }

    #[test]
    fn a_failed_add_changes_nothing() {
        let mut tabs = tabs(&["a"]);
        let result = tabs.add_with(|_| Err::<&str, _>("no pty"));
        assert_eq!(result, Err("no pty"));
        assert_eq!(order(&tabs), ["a"]);
        assert_eq!(tabs.active(), Some(&"a"));
    }

    #[test]
    fn ids_are_unique_and_never_reused() {
        let mut tabs = tabs(&[]);
        let a = add(&mut tabs, "a");
        let b = add(&mut tabs, "b");
        assert_ne!(a, b);
        tabs.close(1);
        let c = add(&mut tabs, "c");
        assert_ne!(b, c);
        assert_eq!(tabs.index_of(b), None, "closed tabs are gone");
        assert_eq!(tabs.index_of(c), Some(1));
        *tabs.get_mut(a).unwrap() = "A";
        assert_eq!(order(&tabs), ["A", "c"]);
        assert!(tabs.get_mut(b).is_none());
    }

    #[test]
    fn the_id_is_known_while_making_the_tab() {
        let mut tabs = Tabs::new();
        let id = tabs.add_with(Ok::<_, Infallible>).unwrap();
        assert_eq!(tabs.active(), Some(&id));
    }

    #[test]
    fn closing_the_active_tab_activates_the_right_neighbour() {
        let mut tabs = tabs(&["a", "b", "c"]);
        tabs.goto(2);
        assert_eq!(tabs.close(1), Some("b"));
        assert_eq!(tabs.active(), Some(&"c"));
    }

    #[test]
    fn closing_the_last_active_tab_activates_the_left_neighbour() {
        let mut tabs = tabs(&["a", "b", "c"]);
        assert_eq!(tabs.close(2), Some("c"));
        assert_eq!(tabs.active(), Some(&"b"));
    }

    #[test]
    fn closing_another_tab_keeps_the_active_one() {
        let mut tabs = tabs(&["a", "b", "c"]);
        tabs.goto(2);
        tabs.close(0);
        assert_eq!(tabs.active(), Some(&"b"));
        tabs.close(1);
        assert_eq!(tabs.active(), Some(&"b"));
    }

    #[test]
    fn closing_everything_leaves_it_empty() {
        let mut tabs = tabs(&["a"]);
        assert_eq!(tabs.close(0), Some("a"));
        assert!(tabs.is_empty());
        assert_eq!(tabs.active(), None);
        assert_eq!(tabs.close(0), None);
        add(&mut tabs, "b");
        assert_eq!(tabs.active(), Some(&"b"));
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let mut tabs = tabs(&["a", "b", "c"]);
        tabs.next();
        assert_eq!(tabs.active(), Some(&"a"));
        tabs.previous();
        assert_eq!(tabs.active(), Some(&"c"));
        tabs.previous();
        assert_eq!(tabs.active(), Some(&"b"));
        let mut empty: Tabs<()> = Tabs::new();
        empty.next();
        empty.previous();
        assert_eq!(empty.active(), None);
    }

    #[test]
    fn goto_counts_from_one_and_stops_at_the_last_tab() {
        let mut tabs = tabs(&["a", "b", "c"]);
        tabs.goto(1);
        assert_eq!(tabs.active(), Some(&"a"));
        tabs.goto(2);
        assert_eq!(tabs.active(), Some(&"b"));
        tabs.goto(9);
        assert_eq!(tabs.active(), Some(&"c"));
    }

    #[test]
    fn select_ignores_missing_tabs() {
        let mut tabs = tabs(&["a", "b"]);
        assert!(!tabs.select(2));
        assert_eq!(tabs.active(), Some(&"b"));
        assert!(tabs.select(0));
        assert_eq!(tabs.active(), Some(&"a"));
    }
}

//! Ordered backend selection with automatic fallback.
//!
//! Used wherever a preferred backend may be unavailable at runtime:
//! ConPTY -> winpty on Windows, GPU -> CPU rendering, and so on.

/// Deferred backend constructor.
pub type Init<'a, T, E> = Box<dyn FnOnce() -> Result<T, E> + 'a>;

/// A backend that failed to initialize, kept for diagnostics.
#[derive(Debug)]
pub struct Attempt<E> {
    pub name: &'static str,
    pub error: E,
}

/// The first backend that initialized, plus the ones skipped before it.
#[derive(Debug)]
pub struct Selected<T, E> {
    pub name: &'static str,
    pub backend: T,
    pub skipped: Vec<Attempt<E>>,
}

/// Tries each candidate in order and returns the first that succeeds.
///
/// Candidates after the selected one are never constructed. When all fail,
/// every attempt is returned in order.
pub fn first_available<'a, T, E>(
    candidates: Vec<(&'static str, Init<'a, T, E>)>,
) -> Result<Selected<T, E>, Vec<Attempt<E>>> {
    let mut skipped = Vec::new();
    for (name, init) in candidates {
        match init() {
            Ok(backend) => {
                return Ok(Selected {
                    name,
                    backend,
                    skipped,
                });
            }
            Err(error) => skipped.push(Attempt { name, error }),
        }
    }
    Err(skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn ok(value: u8) -> Init<'static, u8, &'static str> {
        Box::new(move || Ok(value))
    }

    fn err(msg: &'static str) -> Init<'static, u8, &'static str> {
        Box::new(move || Err(msg))
    }

    #[test]
    fn picks_first_candidate_when_it_succeeds() {
        let selected = first_available(vec![("gpu", ok(1)), ("cpu", ok(2))]).unwrap();
        assert_eq!((selected.name, selected.backend), ("gpu", 1));
        assert!(selected.skipped.is_empty());
    }

    #[test]
    fn falls_back_and_records_skipped_attempts() {
        let selected = first_available(vec![("gpu", err("no adapter")), ("cpu", ok(2))]).unwrap();
        assert_eq!((selected.name, selected.backend), ("cpu", 2));
        assert_eq!(selected.skipped.len(), 1);
        assert_eq!(selected.skipped[0].name, "gpu");
        assert_eq!(selected.skipped[0].error, "no adapter");
    }

    #[test]
    fn does_not_construct_candidates_after_success() {
        let constructed = Cell::new(false);
        let later: Init<'_, u8, &str> = Box::new(|| {
            constructed.set(true);
            Ok(9)
        });
        first_available(vec![("conpty", ok(1)), ("winpty", later)]).unwrap();
        assert!(!constructed.get());
    }

    #[test]
    fn returns_all_attempts_in_order_when_every_candidate_fails() {
        let attempts =
            first_available(vec![("conpty", err("missing")), ("winpty", err("blocked"))])
                .unwrap_err();
        let names: Vec<_> = attempts.iter().map(|a| a.name).collect();
        assert_eq!(names, ["conpty", "winpty"]);
    }

    #[test]
    fn empty_candidate_list_fails_without_attempts() {
        let attempts = first_available::<u8, &str>(vec![]).unwrap_err();
        assert!(attempts.is_empty());
    }
}

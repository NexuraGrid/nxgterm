//! The environment handed to a winpty child.

use std::ffi::OsString;

/// `vars` with `overrides` applied.
///
/// Windows variable names are case-insensitive, so an override replaces
/// every variable with the same name in any case. The result is sorted by
/// upper-cased name, as `CreateProcess` expects of an environment block.
#[cfg_attr(not(all(windows, target_arch = "x86_64")), allow(dead_code))]
pub(crate) fn child_env(
    vars: impl IntoIterator<Item = (OsString, OsString)>,
    overrides: &[(&str, &str)],
) -> Vec<(OsString, OsString)> {
    let key = |name: &OsString| name.to_string_lossy().to_uppercase();
    let mut env: Vec<(OsString, OsString)> = vars
        .into_iter()
        .filter(|(name, _)| {
            let name = key(name);
            !overrides.iter().any(|(o, _)| o.to_uppercase() == name)
        })
        .chain(overrides.iter().map(|&(k, v)| (k.into(), v.into())))
        .collect();
    env.sort_by_cached_key(|(name, _)| key(name));
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
        pairs
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v)))
            .collect()
    }

    #[test]
    fn overrides_replace_variables_in_any_case() {
        let env = child_env(
            vars(&[("Path", "C:\\bin"), ("term", "dumb")]),
            &[("TERM", "xterm-256color")],
        );
        assert_eq!(
            env,
            vars(&[("Path", "C:\\bin"), ("TERM", "xterm-256color")])
        );
    }

    #[test]
    fn result_is_sorted_case_insensitively() {
        let env = child_env(vars(&[("b", "2"), ("C", "3"), ("A", "1")]), &[]);
        assert_eq!(env, vars(&[("A", "1"), ("b", "2"), ("C", "3")]));
    }

    #[test]
    fn overrides_are_added_when_absent() {
        let env = child_env(
            vars(&[]),
            &[("TERM_PROGRAM", "nxgterm"), ("TERM", "xterm-256color")],
        );
        assert_eq!(
            env,
            vars(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "nxgterm")])
        );
    }
}

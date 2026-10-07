//! Renderer selection from the config and the `NXGTERM_RENDERER`
//! environment variable.

use nxg_config::Backend;

/// Environment variable that overrides the configured renderer.
pub const ENV_VAR: &str = "NXGTERM_RENDERER";

/// Renderer candidates, in the order they are tried.
///
/// `env` (`auto`, `gpu` or `cpu`, any case, surrounding spaces ignored)
/// wins over `configured`; other values are ignored. `gpu` or `cpu` forces
/// that renderer alone; `auto` tries the GPU then the CPU.
pub fn renderer_order(env: Option<&str>, configured: Backend) -> &'static [&'static str] {
    let from_env = match env.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        Some("auto") => Some(Backend::Auto),
        Some("gpu") => Some(Backend::Gpu),
        Some("cpu") => Some(Backend::Cpu),
        _ => None,
    };
    match from_env.unwrap_or(configured) {
        Backend::Auto => &["gpu", "cpu"],
        Backend::Gpu => &["gpu"],
        Backend::Cpu => &["cpu"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AUTO: Backend = Backend::Auto;

    #[test]
    fn unset_follows_the_config() {
        assert_eq!(renderer_order(None, AUTO), ["gpu", "cpu"]);
        assert_eq!(renderer_order(None, Backend::Gpu), ["gpu"]);
        assert_eq!(renderer_order(None, Backend::Cpu), ["cpu"]);
    }

    #[test]
    fn env_forces_a_single_renderer() {
        assert_eq!(renderer_order(Some("gpu"), AUTO), ["gpu"]);
        assert_eq!(renderer_order(Some("cpu"), AUTO), ["cpu"]);
        assert_eq!(renderer_order(Some(" CPU "), Backend::Gpu), ["cpu"]);
    }

    #[test]
    fn env_auto_overrides_a_forced_config() {
        assert_eq!(renderer_order(Some("auto"), Backend::Cpu), ["gpu", "cpu"]);
    }

    #[test]
    fn unknown_env_values_fall_back_to_the_config() {
        assert_eq!(renderer_order(Some(""), AUTO), ["gpu", "cpu"]);
        assert_eq!(renderer_order(Some("vulkan"), AUTO), ["gpu", "cpu"]);
        assert_eq!(renderer_order(Some("vulkan"), Backend::Cpu), ["cpu"]);
    }
}

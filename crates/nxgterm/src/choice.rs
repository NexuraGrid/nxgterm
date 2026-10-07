//! Renderer selection from the `NXGTERM_RENDERER` environment variable.

/// Environment variable that forces a renderer.
pub const ENV_VAR: &str = "NXGTERM_RENDERER";

/// Renderer candidates, in the order they are tried.
///
/// `gpu` or `cpu` (any case, surrounding spaces ignored) forces that
/// renderer alone; anything else, or unset, tries the GPU then the CPU.
pub fn renderer_order(value: Option<&str>) -> &'static [&'static str] {
    let value = value.map(|v| v.trim().to_ascii_lowercase());
    match value.as_deref() {
        Some("gpu") => &["gpu"],
        Some("cpu") => &["cpu"],
        _ => &["gpu", "cpu"],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_tries_gpu_then_cpu() {
        assert_eq!(renderer_order(None), ["gpu", "cpu"]);
    }

    #[test]
    fn forces_a_single_renderer() {
        assert_eq!(renderer_order(Some("gpu")), ["gpu"]);
        assert_eq!(renderer_order(Some("cpu")), ["cpu"]);
        assert_eq!(renderer_order(Some(" CPU ")), ["cpu"]);
    }

    #[test]
    fn unknown_values_mean_auto() {
        assert_eq!(renderer_order(Some("")), ["gpu", "cpu"]);
        assert_eq!(renderer_order(Some("vulkan")), ["gpu", "cpu"]);
        assert_eq!(renderer_order(Some("auto")), ["gpu", "cpu"]);
    }
}

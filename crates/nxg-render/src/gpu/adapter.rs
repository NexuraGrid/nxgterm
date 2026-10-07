//! Adapter ranking.
//!
//! Software adapters (WARP, llvmpipe, SwiftShader) report
//! [`DeviceType::Cpu`]. They are rejected for the window renderer because
//! our own CPU renderer is faster and lighter than emulating a GPU.

use wgpu::DeviceType;

/// Picks the best candidate: integrated, then discrete (low power first),
/// then virtual, then unknown. Software adapters count only when
/// `allow_software` is set. Ties keep the input order.
pub fn pick<T>(candidates: Vec<(T, DeviceType)>, allow_software: bool) -> Option<T> {
    let rank = |device_type: DeviceType| match device_type {
        DeviceType::IntegratedGpu => Some(0),
        DeviceType::DiscreteGpu => Some(1),
        DeviceType::VirtualGpu => Some(2),
        DeviceType::Other => Some(3),
        DeviceType::Cpu => allow_software.then_some(4),
    };
    candidates
        .into_iter()
        .filter_map(|(candidate, device_type)| Some((rank(device_type)?, candidate)))
        .min_by_key(|&(rank, _)| rank)
        .map(|(_, candidate)| candidate)
}

#[cfg(test)]
mod tests {
    use super::*;
    use DeviceType::*;

    #[test]
    fn prefers_integrated_then_discrete() {
        assert_eq!(
            pick(vec![("d", DiscreteGpu), ("i", IntegratedGpu)], false),
            Some("i")
        );
        assert_eq!(
            pick(vec![("v", VirtualGpu), ("d", DiscreteGpu)], false),
            Some("d")
        );
        assert_eq!(
            pick(vec![("o", Other), ("v", VirtualGpu)], false),
            Some("v")
        );
    }

    #[test]
    fn keeps_input_order_on_ties() {
        assert_eq!(pick(vec![("a", Other), ("b", Other)], false), Some("a"));
    }

    #[test]
    fn rejects_software_adapters_unless_allowed() {
        assert_eq!(pick(vec![("warp", Cpu)], false), None);
        assert_eq!(pick(vec![("warp", Cpu)], true), Some("warp"));
        assert_eq!(pick(vec![("warp", Cpu), ("o", Other)], true), Some("o"));
        assert_eq!(pick::<&str>(vec![], true), None);
    }
}

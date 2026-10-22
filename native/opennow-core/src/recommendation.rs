//! Pure profile recommendation for low-latency, low-power streaming.
//! No I/O, no device probes — just deterministic math over capabilities
//! supplied by the Qt shell. Unit-testable with `cargo test` in CI.

use serde_json::{json, Value};

/// Machine tier reported by the Qt shell (capability gathering).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Low,
    Medium,
    High,
}

/// Hardware/decode capability of the current machine, as reported by the shell.
#[derive(Clone, Debug, PartialEq)]
pub struct MachineCapability {
    pub tier: Tier,
    pub hardware_decode_hevc: bool,
    pub hardware_decode_av1: bool,
    pub max_decode_width: u32,
    pub max_decode_height: u32,
    pub max_decode_fps: u32,
    pub supports_10bit: bool,
}

/// Network capability measured by the shell. Downlink is the congestion axis.
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkCapability {
    pub downlink_mbps: f64,   // 0.0 = unknown
    pub uplink_mbps: f64,     // 0.0 = unknown
    pub region_ping_ms: f64,  // 0.0 = unknown
    pub loss_percent: f64,    // 0.0 = unknown
}

/// Output of the recommendation engine: settings to apply and human-readable reasons.
#[derive(Clone, Debug, PartialEq)]
pub struct Recommendation {
    pub settings: Vec<(String, Value)>,
    pub reasons: Vec<String>,
}

const BITRATE_FLOOR_MBPS: u32 = 3;
const BITRATE_CEILING_MBPS: u32 = 50;
const DOWNLINK_HEADROOM: f64 = 0.65;

fn tier_bitrate_cap(tier: Tier) -> u32 {
    match tier {
        Tier::Low => 12,
        Tier::Medium => 25,
        Tier::High => 50,
    }
}

fn tier_resolution(tier: Tier) -> &'static str {
    match tier {
        Tier::Low => "1280x720",
        Tier::Medium => "1920x1080",
        Tier::High => "2560x1440",
    }
}

fn resolution_min_downlink(resolution: &str) -> f64 {
    match resolution {
        "2560x1440" => 25.0,
        "1920x1080" => 15.0,
        _ => 8.0,
    }
}

fn parse_width_height(s: &str) -> (u32, u32) {
    let mut parts = s.split('x');
    let w = parts.next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    let h = parts.next().and_then(|p| p.parse::<u32>().ok()).unwrap_or(0);
    (w, h)
}

/// Recommend a stream profile deterministically from machine + network capability.
pub fn recommend_profile(machine: &MachineCapability, network: &NetworkCapability) -> Recommendation {
    let mut settings: Vec<(String, Value)> = Vec::new();
    let mut reasons: Vec<String> = Vec::new();

    // 1. Downlink: an unknown measurement must not shrink the profile.
    let downlink = if network.downlink_mbps > 0.0 {
        network.downlink_mbps
    } else {
        f64::INFINITY
    };

    // 2. Bitrate: tier cap, capped at 65% of measured downlink, floored at 3 Mbps.
    let mut bitrate = tier_bitrate_cap(machine.tier);
    if downlink.is_finite() {
        let capped = (downlink * DOWNLINK_HEADROOM).floor() as u32;
        bitrate = bitrate.min(capped);
    }
    bitrate = bitrate.max(BITRATE_FLOOR_MBPS).min(BITRATE_CEILING_MBPS);

    settings.push(("maxBitrateMbps".to_string(), json!(bitrate)));
    reasons.push(format!(
        "bitrate {} Mbps: tier cap {} Mbps, {}",
        bitrate,
        tier_bitrate_cap(machine.tier),
        if downlink.is_finite() { "downlink headroom 65%" } else { "downlink unknown, tier cap kept" }
    ));

    // 3. Resolution: start at tier cap, step down the ladder while decode/downlink constrain.
    let ladder = ["2560x1440", "1920x1080", "1280x720"];
    let mut resolution = tier_resolution(machine.tier);
    let mut tier_idx = ladder.iter().position(|&r| r == resolution).unwrap_or(2);
    while tier_idx + 1 < ladder.len() {
        let next = ladder[tier_idx + 1];
        let blocked_by_decode = machine.max_decode_width > 0 && machine.max_decode_height > 0
            && (parse_width_height(next).0 > machine.max_decode_width
                || parse_width_height(next).1 > machine.max_decode_height);
        let blocked_by_downlink = downlink.is_finite() && downlink < resolution_min_downlink(next);
        if blocked_by_decode || blocked_by_downlink {
            resolution = next;
            tier_idx += 1;
        } else {
            break;
        }
    }

    settings.push(("resolution".to_string(), json!(resolution)));
    reasons.push(format!(
        "resolution {}: tier {} fits decode ceiling and downlink headroom",
        resolution, machine.tier
    ));

    // 4. FPS: 60 when decode supports it and downlink is wide enough, else 30.
    let fps = if machine.max_decode_fps >= 60 && (downlink.is_infinite() || downlink >= 8.0) {
        60
    } else {
        30
    };
    settings.push(("fps".to_string(), json!(fps)));
    reasons.push(format!(
        "fps {}: decode {} fps, downlink {} mbps",
        fps,
        machine.max_decode_fps,
        if downlink.is_finite() { downlink.to_string() } else { "unknown".into() }
    ));

    // 5. Codec: h264 for low tier; h265 when hardware HEVC decode confirmed; av1 never in this version.
    let codec = if machine.tier == Tier::Low || !machine.hardware_decode_hevc {
        "h264"
    } else {
        "h265"
    };
    settings.push(("codec".to_string(), json!(codec)));
    settings.push(("fallbackCodec".to_string(), json!("h264")));
    reasons.push(format!(
        "codec {}: hardware HEVC {}, fallback h264",
        codec, machine.hardware_decode_hevc
    ));

    // 6. Color: 10-bit only when supported, codec is not h264, and downlink is wide.
    let color_quality = if machine.supports_10bit && codec != "h264" && (downlink.is_infinite() || downlink >= 25.0) {
        "10bit_420"
    } else {
        "8bit_420"
    };
    settings.push(("colorQuality".to_string(), json!(color_quality)));
    reasons.push(format!(
        "color {}: 10-bit {}, downlink {} mbps",
        color_quality,
        if machine.supports_10bit { "supported" } else { "unsupported" },
        if downlink.is_finite() { downlink.to_string() } else { "unknown".into() }
    ));

    // 7. Fixed policy values.
    settings.push(("enableHdr".to_string(), json!(false)));
    settings.push(("upscaling".to_string(), json!("off")));
    settings.push(("saveBandwidth".to_string(), json!(true)));
    reasons.push("HDR off, upscaling off, saveBandwidth on (prefer-FPS): reduces decoder load and gives the server headroom to adapt".to_string());

    Recommendation { settings, reasons }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(rec: &Recommendation, key: &str) -> Value {
        rec.settings
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or(json!(null))
    }

    fn high_machine() -> MachineCapability {
        MachineCapability {
            tier: Tier::High,
            hardware_decode_hevc: true,
            hardware_decode_av1: false,
            max_decode_width: 3840,
            max_decode_height: 2160,
            max_decode_fps: 120,
            supports_10bit: true,
        }
    }

    fn low_machine() -> MachineCapability {
        MachineCapability {
            tier: Tier::Low,
            hardware_decode_hevc: false,
            hardware_decode_av1: false,
            max_decode_width: 1920,
            max_decode_height: 1080,
            max_decode_fps: 60,
            supports_10bit: false,
        }
    }

    fn machine_without_hevc() -> MachineCapability {
        MachineCapability {
            tier: Tier::Medium,
            hardware_decode_hevc: false,
            hardware_decode_av1: false,
            max_decode_width: 1920,
            max_decode_height: 1080,
            max_decode_fps: 60,
            supports_10bit: false,
        }
    }

    fn machine_with_hevc() -> MachineCapability {
        MachineCapability {
            tier: Tier::Medium,
            hardware_decode_hevc: true,
            hardware_decode_av1: false,
            max_decode_width: 1920,
            max_decode_height: 1080,
            max_decode_fps: 60,
            supports_10bit: false,
        }
    }

    fn network(downlink: f64, uplink: f64) -> NetworkCapability {
        NetworkCapability {
            downlink_mbps: downlink,
            uplink_mbps: uplink,
            region_ping_ms: 0.0,
            loss_percent: 0.0,
        }
    }

    #[test]
    fn weak_machine_and_weak_link_get_the_floor_profile() {
        let machine = low_machine();
        let network = network(10.0, 3.0);
        let rec = recommend_profile(&machine, &network);
        assert_eq!(value(&rec, "resolution"), json!("1280x720"));
        assert_eq!(value(&rec, "fps"), json!(30));
        assert_eq!(value(&rec, "codec"), json!("h264"));
        assert_eq!(value(&rec, "colorQuality"), json!("8bit_420"));
        assert_eq!(value(&rec, "enableHdr"), json!(false));
        assert_eq!(value(&rec, "upscaling"), json!("off"));
        assert_eq!(value(&rec, "saveBandwidth"), json!(true));
        assert_eq!(value(&rec, "maxBitrateMbps"), json!(6));
    }

    #[test]
    fn bitrate_is_capped_below_the_downlink_to_avoid_bufferbloat() {
        let rec = recommend_profile(&high_machine(), &network(100.0, 50.0));
        assert_eq!(value(&rec, "maxBitrateMbps"), json!(50));
    }

    #[test]
    fn unknown_downlink_falls_back_to_the_tier_cap_not_zero() {
        let rec = recommend_profile(&low_machine(), &network(0.0, 0.0));
        assert_eq!(value(&rec, "maxBitrateMbps"), json!(12));
    }

    #[test]
    fn hardware_hevc_is_required_before_selecting_h265() {
        let rec = recommend_profile(&machine_without_hevc(), &network(50.0, 20.0));
        assert_eq!(value(&rec, "codec"), json!("h264"));
        let rec = recommend_profile(&machine_with_hevc(), &network(50.0, 20.0));
        assert_eq!(value(&rec, "codec"), json!("h265"));
    }

    #[test]
    fn ten_bit_requires_decoder_support_and_a_wide_link() {
        let mut machine = machine_with_hevc();
        machine.supports_10bit = true;
        let rec = recommend_profile(&machine, &network(50.0, 20.0));
        assert_eq!(value(&rec, "colorQuality"), json!("10bit_420"));
        let rec = recommend_profile(&machine, &network(12.0, 5.0));
        assert_eq!(value(&rec, "colorQuality"), json!("8bit_420"));
    }

    #[test]
    fn every_recommendation_carries_a_reason_per_setting() {
        let rec = recommend_profile(&high_machine(), &network(100.0, 50.0));
        assert!(!rec.reasons.is_empty());
        assert!(rec.reasons.len() >= rec.settings.len());
    }
}

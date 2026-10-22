# OpenNOW Low-Latency Governor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the OpenNOW client hold a stable, low ping on weak links and weak PCs by (a) capping the video bitrate below the measured downlink, (b) picking a stream profile the machine can actually decode, (c) making recovery timing immune to RTT spikes, and (d) shipping a one-click "Recommended" button that applies all of it per machine.

**Architecture:** The decision logic is a pure Rust module in the existing `opennow-core` crate (`recommendation.rs`), so it is unit-testable with `cargo test` in CI and never runs on the user's weak PC. The existing settings store (`SettingsStore`), region ping (`network::ping_regions`), SDP builder (`nvst_rtsp.rs`), and RTT owner (`nvst.rs`) are reused rather than duplicated. The Qt layer only gathers capability inputs, calls one new core command, and renders the result.

**Tech Stack:** Rust (core + streamer crates), Qt 6 / QML (desktop shell), JSON protocol between QML and the bundled Rust core (`docs/core-protocol.md`), GitHub Actions CI (`qt-checks`, `qt-build`).

**Spec:** This document. (Commit a copy to the repo as `docs/superpowers/specs/2026-10-22-low-latency-governor-design.md` in Task 7.)

## Global Constraints

- Fork: `UPGRADER-RANDOM-PROJECTS/OpenNOW` (fork of `OpenCloudGaming/OpenNOW`, default branch `main`). Work on branch `feat/low-latency-governor`; **no pull requests** — push the branch to the fork and fast-forward `main` when the branch is green.
- Repo `AGENTS.md` priorities, in order: performance, reliability, predictable behavior under load and during failures.
- No local build of the desktop app. All Rust/QML verification happens in CI (`cargo fmt --check`, `cargo clippy`, `cargo test`, QML syntax checks, `qt-checks`/`qt-build` workflows).
- Rust edition/style follows the existing crates: pure functions, `#[cfg(test)] mod tests` inside the same file, no new top-level crates.
- New user-visible strings go through `qsTr(...)` in QML and are added to the locale files the repo already maintains (`locales/`).
- Never raise quality automatically. Degradation is one-way per session and always visible in the UI.
- Existing setting key names and allowed values are fixed (do not rename): `codec` ∈ `auto|av1|h264|h265`, `fallbackCodec` ∈ `auto|h264|h265`, `colorQuality` ∈ `8bit_420|10bit_420|8bit_444|10bit_444`, `resolution` = `"WxH"`, `fps` = int 30..360, `maxBitrateMbps` = int, `saveBandwidth` = bool, `enableHdr` = bool, `upscaling` = `"off"`, `region` = string ≤ 256 chars.
- The video stream flows **server → client**, so the congestion that produces ping spikes and frame drops is on the **downlink**. Caps and measurements in this plan are downlink-based. Uplink only matters for input/ACK traffic and is not capped.

## Review Focus

- Downlink measurement returns 0 or fails (offline, blocked endpoint): the recommendation must still return a valid profile (bitrate floor 3 Mbps, resolution 1280x720) and never write an invalid value into the settings store.
- Region ping returns no usable region (all offline): the recommendation must keep the user's current `region` untouched instead of clearing it.
- The machine reports no hardware HEVC/AV1 decode: the recommendation must not select `h265`/`av1`, and must fall back to `h264`.
- The user manually changed a stream setting after applying a recommendation: the next "Recommended" press must show a diff of what will change, not silently overwrite.
- A session is live while the recommendation is applied: settings must only affect the next ANNOUNCE; the running session must not be disturbed or invalidated.

---

### Task 1: Branch and local clone of the fork

**Files:**
- Modify: none (git only)

**Interfaces:**
- Consumes: none
- Produces: local clone at `C:\Users\mr_ali7685\Documents\Default Project\OpenNOW` with branch `feat/low-latency-governor` tracking the fork's `main`.

- [ ] **Step 1: Clone the fork (shallow, single branch)**

Run:
```bash
git clone --depth 1 --single-branch --branch main https://github.com/UPGRADER-RANDOM-PROJECTS/OpenNOW.git OpenNOW
```
Expected: clone succeeds into `OpenNOW/`, `main` checked out.

- [ ] **Step 2: Create the working branch**

Run:
```bash
git -C OpenNOW checkout -b feat/low-latency-governor
```
Expected: `Switched to a new branch 'feat/low-latency-governor'`.

- [ ] **Step 3: Verify the baseline test command works before any change**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-core/Cargo.toml
```
Expected: existing core tests pass. If `cargo` is unavailable on this machine, record that and rely on CI for all test runs (do not install toolchains on the weak PC).

- [ ] **Step 4: Commit nothing yet; leave the tree clean**

Run:
```bash
git -C OpenNOW status --short
```
Expected: no output.

---

### Task 2: Pure recommendation engine in the core crate

**Files:**
- Create: `native/opennow-core/src/recommendation.rs`
- Modify: `native/opennow-core/src/main.rs` (module declaration `mod recommendation;` only — no dispatch yet)

**Interfaces:**
- Consumes: none (pure module, no I/O)
- Produces:
```rust
// native/opennow-core/src/recommendation.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier { Low, Medium, High }

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

#[derive(Clone, Debug, PartialEq)]
pub struct NetworkCapability {
    pub downlink_mbps: f64,   // 0.0 = unknown
    pub uplink_mbps: f64,     // 0.0 = unknown
    pub region_ping_ms: f64,  // 0.0 = unknown
    pub loss_percent: f64,    // 0.0 = unknown
}

#[derive(Clone, Debug, PartialEq)]
pub struct Recommendation {
    pub settings: Vec<(String, serde_json::Value)>,
    pub reasons: Vec<String>,
}

pub fn recommend_profile(
    machine: &MachineCapability,
    network: &NetworkCapability,
) -> Recommendation
```
`settings` contains exactly these keys, in this order: `resolution`, `fps`, `codec`, `fallbackCodec`, `colorQuality`, `enableHdr`, `upscaling`, `saveBandwidth`, `maxBitrateMbps`.

- [ ] **Step 1: Write the failing unit tests**

In `native/opennow-core/src/recommendation.rs` add `#[cfg(test)] mod tests` with these cases and exact expectations:

```rust
#[test]
fn weak_machine_and_weak_link_get_the_floor_profile() {
    let machine = MachineCapability { tier: Tier::Low, hardware_decode_hevc: false,
        hardware_decode_av1: false, max_decode_width: 1920, max_decode_height: 1080,
        max_decode_fps: 60, supports_10bit: false };
    let network = NetworkCapability { downlink_mbps: 6.0, uplink_mbps: 2.0,
        region_ping_ms: 180.0, loss_percent: 4.0 };
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
    // downlink 100 Mbps must not produce a 100 Mbps stream
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
```
Helper `fn value(rec: &Recommendation, key: &str) -> serde_json::Value` in the test module reads `rec.settings`.

- [ ] **Step 2: Run the tests to verify they fail**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-core/Cargo.toml recommendation
```
Expected: compile error — `recommend_profile` not defined (the tests fail for the right reason).

- [ ] **Step 3: Implement `recommendation.rs`**

Implementation, exactly as specified:

```rust
const BITRATE_FLOOR_MBPS: u32 = 3;
const BITRATE_CEILING_MBPS: u32 = 50;
const DOWNLINK_HEADROOM: f64 = 0.65;

fn tier_bitrate_cap(tier: Tier) -> u32 { match tier { Tier::Low => 12, Tier::Medium => 25, Tier::High => 50 } }
fn tier_resolution(tier: Tier) -> &'static str { match tier { Tier::Low => "1280x720", Tier::Medium => "1920x1080", Tier::High => "2560x1440" } }
fn resolution_min_downlink(resolution: &str) -> f64 { match resolution { "2560x1440" => 25.0, "1920x1080" => 15.0, _ => 8.0 } }
```

`recommend_profile` computes:
1. `downlink = if network.downlink_mbps > 0.0 { network.downlink_mbps } else { f64::INFINITY }` — an unknown measurement must not shrink the profile; the tier cap still applies.
2. `bitrate = min(tier_bitrate_cap(tier), floor(downlink * 0.65))`, floored at `BITRATE_FLOOR_MBPS`. With unknown downlink the result is the tier cap.
3. `resolution`: start from `tier_resolution(tier)`; step down the fixed ladder `["2560x1440", "1920x1080", "1280x720"]` while (a) the ladder entry is above the tier's resolution, (b) `max_decode_width/height` cannot hold it, or (c) `downlink < resolution_min_downlink(entry)`. Never below `1280x720`.
4. `fps`: `60` when `max_decode_fps >= 60 && downlink >= 8.0`, else `30`.
5. `codec`: `h264` for `Tier::Low`; `h265` when `hardware_decode_hevc`; else `h264`. `av1` is never selected in this version. `fallbackCodec` is always `h264`.
6. `colorQuality`: `10bit_420` when `supports_10bit && codec != "h264" && downlink >= 25.0`; else `8bit_420`.
7. `enableHdr` = `false`, `upscaling` = `"off"`, `saveBandwidth` = `true` (maps to `dynamicStreamingMode=1`, prefer-FPS).
8. `reasons`: one English sentence per setting, e.g. `"bitrate 6 Mbps: downlink measured 6 Mbps, capped at 65% to avoid bufferbloat"`. Reasons are diagnostic text; the UI localizes its own copy.

- [ ] **Step 4: Register the module**

In `native/opennow-core/src/main.rs`, add `mod recommendation;` next to the other module declarations (`mod network;`, `mod settings;`).

- [ ] **Step 5: Run the tests to verify they pass**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-core/Cargo.toml recommendation
```
Expected: 6 tests pass.

- [ ] **Step 6: Commit**

```bash
git -C OpenNOW add native/opennow-core/src/recommendation.rs native/opennow-core/src/main.rs
git -C OpenNOW commit -m "feat(core): add pure low-latency profile recommendation engine"
```

---

### Task 3: `settings.recommend` core command

**Files:**
- Modify: `native/opennow-core/src/main.rs` (dispatch table, next to `"network.regions.ping" => network::ping_regions(params)`)
- Modify: `native/opennow-core/src/recommendation.rs` (add the command entry point)
- Test: `native/opennow-core/src/recommendation.rs` (unit test for the entry point)

**Interfaces:**
- Consumes: `recommend_profile` (Task 2), `SettingsStore::all()` / `SettingsStore::set` (existing, `native/opennow-core/src/settings.rs`)
- Produces: core command `settings.recommend` with request payload
```json
{"machine": {"tier":"low","hardwareDecodeHevc":false,"hardwareDecodeAv1":false,
  "maxDecodeWidth":1920,"maxDecodeHeight":1080,"maxDecodeFps":60,"supports10bit":false},
 "network": {"downlinkMbps":8.5,"uplinkMbps":3.0,"regionPingMs":120.0,"lossPercent":1.5}}
```
and response payload
```json
{"settings": {"resolution":"1280x720","fps":30,"codec":"h264","fallbackCodec":"h264",
  "colorQuality":"8bit_420","enableHdr":false,"upscaling":"off","saveBandwidth":true,
  "maxBitrateMbps":5},
 "reasons": ["..."]}
```
The command **computes and returns**; it does not write. Writing is a separate `settings.set` call issued by the client, so the UI can show a diff first.

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn recommend_command_parses_capabilities_and_returns_settings_without_writing() {
    let params = json!({
        "machine": {"tier":"medium","hardwareDecodeHevc":true,"hardwareDecodeAv1":false,
            "maxDecodeWidth":1920,"maxDecodeHeight":1080,"maxDecodeFps":60,"supports10bit":false},
        "network": {"downlinkMbps":20.0,"uplinkMbps":5.0,"regionPingMs":80.0,"lossPercent":1.0}
    });
    let response = recommend(&params).unwrap();
    assert_eq!(response["settings"]["resolution"], json!("1920x1080"));
    assert_eq!(response["settings"]["codec"], json!("h265"));
    assert_eq!(response["settings"]["maxBitrateMbps"], json!(13));
    assert!(response["reasons"].as_array().unwrap().len() >= 8);
}

#[test]
fn recommend_command_rejects_unknown_tier() {
    let params = json!({"machine": {"tier":"ultra"}, "network": {}});
    assert!(recommend(&params).is_err());
}
```
`recommend` lives in `recommendation.rs` and is `pub(crate) fn recommend(params: &serde_json::Value) -> Result<serde_json::Value, String>`.

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-core/Cargo.toml recommendation
```
Expected: compile error — `recommend` not defined.

- [ ] **Step 3: Implement `recommend`**

Parse `params["machine"]` and `params["network"]` into `MachineCapability` / `NetworkCapability` (missing fields default to the conservative values: `tier` defaults to `Low`, unknown booleans `false`, zero dimensions mean "unknown" and skip the decode-size constraint). Unknown `tier` string → `Err("unknown tier: <value>")`. Call `recommend_profile` and return `json!({"settings": map, "reasons": [...]})`.

- [ ] **Step 4: Wire the dispatch**

In `native/opennow-core/src/main.rs`, next to the `"network.regions.ping"` arm, add:
```rust
"settings.recommend" => recommendation::recommend(params),
```
and add `"settings.recommend"` to the `capabilities` array in the hello payload (line ~435 of `main.rs`).

- [ ] **Step 5: Run the tests to verify they pass**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-core/Cargo.toml recommendation
```
Expected: all recommendation tests pass.

- [ ] **Step 6: Commit**

```bash
git -C OpenNOW add native/opennow-core/src/main.rs native/opennow-core/src/recommendation.rs
git -C OpenNOW commit -m "feat(core): add settings.recommend command"
```

---

### Task 4: Enable the server-side dynamic rate control in the ANNOUNCE

**Files:**
- Modify: `native/opennow-core/src/settings.rs` (new setting key + default)
- Modify: `native/opennow-streamer/crates/opennow-streamer-core/src/nvst_rtsp.rs` (SDP builder)
- Test: `native/opennow-streamer/crates/opennow-streamer-core/src/nvst_rtsp.rs` (existing inline tests)

**Interfaces:**
- Consumes: existing settings store (`SettingsStore::set("dynamicRateControl", json!(true))`), existing SDP builder
- Produces: new setting `dynamicRateControl` (bool, default `true`); SDP line `a=x-nv-vqos[0].drc.enable:1` when enabled.

- [ ] **Step 1: Write the failing test**

In `nvst_rtsp.rs` add:
```rust
#[test]
fn dynamic_rate_control_is_enabled_by_default() {
    let sdp = build_test_sdp_with_settings(json!({"maxBitrateMbps": 12}));
    assert!(sdp.contains("a=x-nv-vqos[0].drc.enable:1\r\n"));
}

#[test]
fn dynamic_rate_control_can_be_disabled() {
    let sdp = build_test_sdp_with_settings(json!({"maxBitrateMbps": 12, "dynamicRateControl": false}));
    assert!(sdp.contains("a=x-nv-vqos[0].drc.enable:0\r\n"));
}
```
Use the same helper the neighbouring SDP tests in this file already use (the tests around line 3049-3160 build an SDP from a settings JSON). If the helper takes the full settings object, pass the extra key through it.

- [ ] **Step 2: Run the tests to verify they fail**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-streamer/Cargo.toml dynamic_rate_control
```
Expected: FAIL — the SDP still contains `drc.enable:0`.

- [ ] **Step 3: Implement**

In `nvst_rtsp.rs`, replace the hardcoded line
`"a=x-nv-vqos[0].drc.enable:0".to_owned(),`
with a computed value read from the settings object the builder already receives:
```rust
let drc_enabled = settings.get("dynamicRateControl").and_then(serde_json::Value::as_bool).unwrap_or(true);
```
and emit `format!("a=x-nv-vqos[0].drc.enable:{drc}")` with `drc` = `1`/`0`.

In `native/opennow-core/src/settings.rs`, add `"dynamicRateControl": true` to the defaults map (near `"saveBandwidth": false`) and allow the key in the validated key list so `SettingsStore::set` accepts it.

- [ ] **Step 4: Fix the existing test that asserts the old value**

The existing test at `nvst_rtsp.rs:3088` asserts `a=x-nv-vqos[0].drc.enable:0`. Update it to assert `1` (the new default) and add a second assertion path for the disabled case (covered by the new test in Step 1).

- [ ] **Step 5: Run the tests to verify they pass**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-streamer/Cargo.toml
```
Expected: all streamer-core tests pass.

- [ ] **Step 6: Commit**

```bash
git -C OpenNOW add native/opennow-core/src/settings.rs native/opennow-streamer/crates/opennow-streamer-core/src/nvst_rtsp.rs
git -C OpenNOW commit -m "feat(streamer): enable dynamic rate control in the RTSP announce"
```

---

### Task 5: Make recovery timing immune to RTT spikes

**Files:**
- Modify: `native/opennow-streamer/crates/opennow-streamer-transport/src/nvst.rs`
- Test: same file (inline `#[cfg(test)] mod tests`)

**Interfaces:**
- Consumes: existing ping owners (`ice_ping`, `video_ping`, `bundle_ping`: `Mutex<Option<(Instant, Duration)>>`), `take_nack(now, retry_rtt)`, `ping_ms(now)`
- Produces:
```rust
const PING_WINDOW_SAMPLES: usize = 8;
const PING_WINDOW_TTL: Duration = Duration::from_secs(5);

fn stable_rtt(&self, now: Instant) -> Option<Duration>   // min of the fresh samples in the window
pub fn stable_ping_ms(&self, now: Instant) -> Option<f64> // stable_rtt in milliseconds
```
`ping_ms` keeps reporting the **current** sample so the on-screen ping still spikes honestly; only the recovery timing paths switch to `stable_rtt`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn a_ping_spike_does_not_stretch_the_nack_retry_interval() {
    // baseline 20 ms, then a 200 ms spike
    let transport = test_transport_with_ping(Duration::from_millis(20));
    let baseline = transport.stable_rtt(Instant::now());
    transport.publish_ping(false, Instant::now(), Duration::from_millis(200));
    let after_spike = transport.stable_rtt(Instant::now());
    assert_eq!(baseline, after_spike);
    assert_eq!(transport.ping_ms(Instant::now()), Some(200.0));
}

#[test]
fn stable_rtt_falls_back_to_the_current_sample_when_the_window_is_empty() {
    let transport = test_transport_with_ping(Duration::from_millis(35));
    assert_eq!(transport.stable_rtt(Instant::now()), Some(Duration::from_millis(35)));
}

#[test]
fn expired_samples_leave_the_window() {
    let transport = test_transport_with_ping(Duration::from_millis(35));
    let later = Instant::now() + PING_WINDOW_TTL + Duration::from_secs(1);
    assert_eq!(transport.stable_rtt(later), None);
}
```
`test_transport_with_ping` is a test helper that constructs the transport with a published ping sample; if the type cannot be constructed cheaply in tests, drive the existing `publish_ping` path on a transport built by the neighbouring ping tests (`nvst_ping_tests.rs` shows the established pattern).

- [ ] **Step 2: Run the tests to verify they fail**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-streamer/Cargo.toml stable_rtt
```
Expected: compile error — `stable_rtt` not defined.

- [ ] **Step 3: Implement the ping window**

Replace the three `Mutex<Option<(Instant, Duration)>>` ping holders with a small struct that keeps the latest sample plus a bounded window:

```rust
struct PingWindow {
    latest: Option<(Instant, Duration)>,
    samples: VecDeque<(Instant, Duration)>, // newest last, capped at PING_WINDOW_SAMPLES
}
```
`publish_ping` and `update_ice_ping` push into it and evict samples older than `PING_WINDOW_TTL` and beyond `PING_WINDOW_SAMPLES`. `ping(now)` keeps its current semantics (first fresh sample in the existing priority order: ICE, video, bundle). `stable_rtt(now)` returns the **minimum** duration among the fresh samples of all three windows (min-filtering is the standard RTT-spike estimator: it tracks the path's floor, not its peaks).

- [ ] **Step 4: Use the stable RTT in the recovery paths**

In `take_nack(now, retry_rtt)`, the caller passes the RTT used for `retry_interval = rtt + NACK_RETRY_INTERVAL` and `attempts = clamp(52ms / rtt, 1, 3)`. Change the call site to pass `stable_rtt(now)` instead of the current sample. The IDR backstop (`IDR_RETRY_INTERVAL`, the 250 ms backstop) must keep using wall-clock deadlines and must not change.

- [ ] **Step 5: Run the tests to verify they pass**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-streamer/Cargo.toml
```
Expected: all transport tests pass, including the existing NACK and ping tests.

- [ ] **Step 6: Commit**

```bash
git -C OpenNOW add native/opennow-streamer/crates/opennow-streamer-transport/src/nvst.rs
git -C OpenNOW commit -m "fix(transport): use a filtered RTT for NACK recovery timing"
```

---

### Task 6: "Recommended" button in the Qt settings UI

**Files:**
- Modify: `opennow-qt/qml/desktop/settings/pages/DesktopSettingsStreamPage.qml`
- Modify: `opennow-qt/qml/desktop/settings/pages/DesktopSettingsNetworkPage.qml` (link to the same action)
- Modify: `opennow-qt/qml/state/ShellStore.qml` (new action + capability gathering)
- Modify: locale files under `locales/` for any new `qsTr` strings
- Test: `opennow-qt/tests/` acceptance test following the existing pattern (e.g. `RegionChoicesAcceptance.qml`)

**Interfaces:**
- Consumes: core command `settings.recommend` (Task 3), `ShellStore.setSetting(key, value, source)` (existing), `ShellStore.pingRegions()` / `refreshRegions()` (existing), `network.regions.ping` results
- Produces: `ShellStore.recommendProfile()` action that (1) gathers machine + network capability, (2) calls `settings.recommend`, (3) stores the proposal in `recommendedProfileProposal`, and (4) applies it with `applyRecommendedProfile()` only after the user confirms.

- [ ] **Step 1: Write the failing acceptance test**

Create `opennow-qt/tests/RecommendedProfileAcceptance.qml` following the shape of `RegionChoicesAcceptance.qml`: it asserts that a "Recommended" button exists with `objectName: "recommendedProfileButton"`, that pressing it when no measurement is available leaves the current settings unchanged, and that applying a proposal writes exactly the keys `resolution`, `fps`, `codec`, `fallbackCodec`, `colorQuality`, `enableHdr`, `upscaling`, `saveBandwidth`, `maxBitrateMbps`.

- [ ] **Step 2: Run the test to verify it fails**

Run the QML acceptance test target the repo uses for `opennow-qt/tests/` (see `opennow-qt/README.md` "CI checks" for the exact command; if it needs a desktop session, run it in CI and rely on the red CI run as the failing check).
Expected: FAIL — the button does not exist.

- [ ] **Step 3: Implement the capability gathering**

In `ShellStore.qml` add `function recommendProfile()`:
1. `refreshRegions()` + `pingRegions()` to obtain per-region latency; pick the lowest measured ping (keep the current region if no region measured).
2. Machine capability: CPU core count and physical memory from the existing native layer (the runtime already exposes platform info for other features — reuse it). Decoder capability: use the last decoder report the streamer emitted when it probed the adapters (`d3d11-decode` / `nvst-handoff` telemetry already name the selected backend); when no report exists, assume no hardware HEVC/AV1 and a 1920x1080@60 ceiling (conservative).
3. Network capability: downlink measured by an active download probe against a configurable endpoint stored in settings (`linkProbeUrl`, empty by default → downlink unknown → the engine falls back to tier caps); loss percent from the streamer's loss counters when a session has run.
4. Call the core command `settings.recommend` with the payload from Task 3 and store the response in `recommendedProfileProposal`.

- [ ] **Step 4: Implement the UI**

In `DesktopSettingsStreamPage.qml` add a "Recommended" button (`objectName: "recommendedProfileButton"`) next to the stream quality controls. On press: run `recommendProfile()`, then show a summary card listing each proposed change with its reason (`resolution 2560x1440 → 1280x720`, `bitrate 75 → 6 Mbps`, …). A second button ("Apply") writes the values through `setSetting(key, value, "recommended")`; a "Cancel" button discards the proposal. Nothing is written before the user confirms.

- [ ] **Step 5: Localize the new strings**

Add every new `qsTr` string to the locale files under `locales/` following the existing pattern.

- [ ] **Step 6: Run the checks**

Run in CI (QML syntax check + Qt acceptance): push the branch and let `qt-checks` run; locally only run `cargo` tests (the desktop app is not built on this machine).

- [ ] **Step 7: Commit**

```bash
git -C OpenNOW add opennow-qt/qml opennow-qt/tests locales
git -C OpenNOW commit -m "feat(qt): add Recommended profile button to stream settings"
```

---

### Task 7: Lag report and one-way live degradation

**Files:**
- Modify: `native/opennow-streamer/crates/opennow-streamer-core/src/queue_drops.rs` (per-stage aggregation)
- Modify: `opennow-qt/qml/desktop/settings/pages/DesktopSettingsStatsPage.qml` (report rows)
- Modify: `opennow-qt/qml/desktop/stream/DesktopStreamStats.qml` (live ping + stable ping + drops)
- Modify: `native/opennow-core/src/settings.rs` (new setting `lastSessionLocalStall`, default `false`)
- Test: `native/opennow-streamer/crates/opennow-streamer-core/src/queue_drops.rs` (inline tests)

**Interfaces:**
- Consumes: existing `queue-dropped` events per media source (`video`, `d3d11-decode`, `present`, `audio-output`, …) from `queue_drops.rs`; `ping_ms` and the new `stable_ping_ms` (Task 5)
- Produces: a per-session aggregate `local_stall: bool` exposed to the UI, and a "Lag report" block in the stats page listing: current ping, stable ping, per-stage drop counts, negotiated resolution/fps/codec/bitrate, and the applied `dynamicRateControl` / `dynamicStreamingMode` policy.

- [ ] **Step 1: Write the failing test**

In `queue_drops.rs` add:
```rust
#[test]
fn decode_and_present_drops_mark_a_local_stall() {
    let mut reports = QueueDropReports::new();
    reports.record("d3d11-decode", 4);
    reports.record("present", 2);
    assert!(reports.local_stall());
}

#[test]
fn network_only_drops_do_not_mark_a_local_stall() {
    let mut reports = QueueDropReports::new();
    reports.record("video", 40);
    assert!(!reports.local_stall());
}
```
`local_stall()` returns true when the total of `d3d11-decode`, `present`, `video-decode`, `video-presentation` drops in the current session is ≥ 3.

- [ ] **Step 2: Run the test to verify it fails**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-streamer/Cargo.toml local_stall
```
Expected: compile error — `local_stall` not defined.

- [ ] **Step 3: Implement the aggregation**

Add per-source counters to `QueueDropReports` (it currently only keeps a pending total per media source) and a `local_stall()` accessor over the local-decode sources listed above.

- [ ] **Step 4: Surface the report in the UI**

In `DesktopSettingsStatsPage.qml` add a "Lag report" section reading the values the runtime already streams (ping, drops per stage, negotiated profile) and in `DesktopStreamStats.qml` show `ping` (current) next to `stable ping` so a spike is visibly distinguishable from the path floor.

- [ ] **Step 5: Persist the stall flag**

At session end the core writes `lastSessionLocalStall: true` into the settings store when the session produced a local stall. The next "Recommended" press then tightens the profile by one tier step (e.g. 1080p → 720p) and adds the reason `"previous session dropped frames in the local decode path"`. Never the reverse: the flag is cleared only by the user resetting the profile.

- [ ] **Step 6: Run the tests to verify they pass**

Run:
```bash
cargo test --manifest-path OpenNOW/native/opennow-streamer/Cargo.toml
```
Expected: all streamer-core tests pass.

- [ ] **Step 7: Commit**

```bash
git -C OpenNOW add native/opennow-streamer/crates/opennow-streamer-core/src/queue_drops.rs native/opennow-core/src/settings.rs opennow-qt/qml
git -C OpenNOW commit -m "feat(streamer): report per-stage frame drops and local stalls"
```

---

### Task 8: Documentation and spec

**Files:**
- Create: `docs/superpowers/specs/2026-10-22-low-latency-governor-design.md` (this document, committed)
- Create: `docs/low-latency-profile.md` (user-facing guide)

**Interfaces:**
- Consumes: the finished implementation
- Produces: two documents

- [ ] **Step 1: Commit the spec**

```bash
git -C OpenNOW add docs/superpowers/specs/2026-10-22-low-latency-governor-design.md
git -C OpenNOW commit -m "docs: add low-latency governor design"
```

- [ ] **Step 2: Write the user guide**

`docs/low-latency-profile.md` covers: what the "Recommended" button does, why the bitrate is capped at 65% of the measured downlink (bufferbloat), why quality is reduced instead of the frame rate, how to override any single setting, how to re-measure, and how to read the Lag report.

- [ ] **Step 3: Commit**

```bash
git -C OpenNOW add docs/low-latency-profile.md
git -C OpenNOW commit -m "docs: add low-latency profile guide"
```

---

### Task 9: Green branch and fast-forward the fork's main

**Files:**
- Modify: none (git only)

- [ ] **Step 1: Push the branch to the fork**

```bash
git -C OpenNOW push -u origin feat/low-latency-governor
```
Expected: branch pushed to `UPGRADER-RANDOM-PROJECTS/OpenNOW`. **No pull request is created** (per the user's instruction).

- [ ] **Step 2: Verify CI is green**

Run:
```bash
gh run list --repo UPGRADER-RANDOM-PROJECTS/OpenNOW --branch feat/low-latency-governor --limit 5
```
Expected: `qt-checks` and the Rust test workflows are green.

- [ ] **Step 3: Fast-forward the fork's main**

```bash
git -C OpenNOW push origin feat/low-latency-governor:main
```
Expected: `main` in the fork now contains the governor work. This is a fast-forward (the branch is a descendant of `main`).

- [ ] **Step 4: Report the result**

Report the fork's `main` commit SHA, the CI run URL, and the list of behavior changes, plus the honest limits: the region distance is not fixable in code, and a weak GPU still limits the ceiling.

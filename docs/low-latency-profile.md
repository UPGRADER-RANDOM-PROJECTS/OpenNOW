# Low-Latency Profile Guide

OpenNOW now includes a **low-latency governor** that helps the client hold a stable, low ping on weak links and weaker PCs. It works in four layers:

1. **Recommended profile** — one-click settings tuned to your machine and connection
2. **Bitrate cap** — the stream never asks for more than ~65% of your measured downlink, so the link can't saturate and spike your ping
3. **Stable RTT recovery** — NACK retries use a filtered RTT floor, so a transient ping spike doesn't stretch recovery
4. **Lag report** — per-session drop counters tell you whether the network or your PC is the bottleneck

## Using the "Recommended" button

Open **Settings › Stream Quality** and click **Recommended**. OpenNOW will:

1. Refresh your regions and ping them
2. Check your CPU tier and available hardware decoders
3. Call the core recommendation engine
4. Show a **Suggested profile** card with each change and the reason for it

Review the card, then click **Apply**. Nothing is written to your settings until you confirm.

### What the recommendation changes

| Setting | Weak link / weak PC | Strong link / strong PC |
|---------|---------------------|-------------------------|
| Resolution | 1280×720 | up to 2560×1440 |
| FPS | 30 | 60 |
| Codec | H.264 | H.265 (if hardware decode confirmed) |
| Bitrate cap | 3–12 Mbps | up to 50 Mbps |
| Color | 8-bit 4:2:0 | 10-bit 4:2:0 (if 10-bit decode + wide link) |
| HDR | Off | Off (unless explicitly validated) |
| Upscaling | Off | Off |
| Save bandwidth | On | On |

## Why the bitrate is capped

The video stream flows **server → you**. If OpenNOW asks for 75 Mbps on a 20 Mbps downlink, your router buffers the excess, your ping jumps from 20 ms to 200 ms, packets drop, and frames freeze. The governor caps the bitrate at **65% of your measured downlink** (floor 3 Mbps) so the link stays below saturation and your ping stays flat.

If you don't know your downlink speed, the governor falls back to a tier-based cap (Low: 12 Mbps, Medium: 25 Mbps, High: 50 Mbps) and never writes zero.

## Why quality goes down instead of frame rate

When the link stutters, the governor lowers resolution and bitrate first. Keeping the frame rate at 60 (or dropping to 30 only when necessary) preserves input responsiveness — a lower-resolution frame at 60 FPS feels more responsive than a sharp frame at 30 FPS.

## Lag report

Open **Settings › Statistics › Lag report** to see:

- **Last session local stall** — whether the previous session dropped frames in the local decode path (GPU/CPU bottleneck) vs. the network
- **Session video drops** — total frames dropped this session

If you see a local stall, apply the Recommended profile — it will tighten the profile by one tier step. The flag is one-way: it only gets cleared when you manually reset your profile.

## Stable ping in the stream overlay

The statistics overlay shows **Ping** (current, can spike) and **Stable Ping** (filtered floor). If the two diverge during a session, your link is spiking — apply the Recommended profile and lower the bitrate cap.

## Manual overrides

You can override any single setting after applying a recommendation. The governor never raises quality automatically — degradation is one-way per session and always visible in the UI. To re-run the recommendation, click **Recommended** again.

## What this does not fix

- **Distance to the server region.** Ping is bounded by physics. Use the region picker to select the closest region with the lowest measured ping.
- **Weak GPU hardware.** If your machine can't hardware-decode HEVC/AV1 or 10-bit, the governor won't select them. Consider H.264 at 720p60.
- **Wi-Fi congestion.** Use Ethernet where possible; Wi-Fi contention causes the same bufferbloat the bitrate cap is designed to prevent.

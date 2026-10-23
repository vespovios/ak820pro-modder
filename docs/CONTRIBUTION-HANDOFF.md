# Upstream contribution handoff

This fork preserves the complete working macOS compatibility implementation
developed against v0.7.0-beta (`382d59ced1c17abb27e0c1c247049cb473e69b66`).
Development branch: `feature/ak820-800a-macos`.

Follow-up TFT finding: both the original snapshot and PR #12 at `e217a427`
hard-coded IMAGE_CFG byte 2 to 3. The vendor driver identifies this as a
one-based destination slot; its single-slot ANSI configuration requires 1.
The original static Resolve PNG appeared briefly among other animation content
with either unmodified uploader. Changing only this slot byte to 1 in a
temporary PR #12 CLI made the image remain visible, confirmed by the owner.
Our app now uses slot 1 with a packet regression fixture. See the destination
slot correction in COMPATIBILITY.md before proposing or validating TFT work.
No message about this finding has been posted upstream.

## Scope

- Detect the FF13 feature-report endpoint and preserve the original AA path.
- Lighting, sleep and clock transactions, ANSI per-key RGB and named colour
  profiles with import/export and a global cycling shortcut.
- Mac-side device-scoped remapping and macro recording/playback.
- FF68/AKS075 TFT static image and GIF upload, with per-block acknowledgements.
- Complete setup snapshots, import/export, shortcut cycling and foreground-app
  rules, including partial-apply reports and upload serialization.
- Login startup, close-to-menu-bar, single instance and saved setup selection
  from the native menu. Stable status-item placement avoids the camera notch.

Detailed protocol, limitations and hardware evidence are in COMPATIBILITY.md,
PER-KEY-RGB.md, MAC-SIDE-CONTROLS.md, SETUP-PROFILES.md and APP-LIFECYCLE.md.
LOCAL-BUILD-NOTES.txt retains the build and physical verification history.
No vendor bundles, USB captures, personal runtime profiles or application
bundles are included in this source snapshot.

## Reconcile with upstream before proposing changes

At fork preparation, upstream main was
`fe93c13972445f92b00b485875da2811173eb940`, seven commits after the base.
Those commits introduce AA-protocol TFT presets, image upload, TFT state
restoration and Now Playing. They overlap app wiring and dependencies with
this implementation and need deliberate integration, rather than a blind
overwrite. Fetch upstream again before starting; it may have moved further.

Keep each protocol's framing and TFT semantics separate. Preserve upstream's
AA-device features and verify that feature-report devices never receive those
commands. Confirm layout support and unsupported capability messaging.
Consider splitting the contribution into reviewable protocol, host controls,
profile and lifecycle changes. Review README, CHANGELOG and PROTOCOL coverage
against the final integrated implementation and upstream's PR template.

## Validation

Hardware: wired ANSI/US AK820 Pro, VID 0C45 / PID 800A, macOS Apple Silicon.
Firmware version is not exposed by this protocol; do not invent a version.
Physical checks confirmed lighting, individual WASD/E colours, static TFT,
animated TFT, setup shortcuts, app switching and menu-bar setup application.
After the final installed build's Accessibility approval, the saved H/I macro
typed `hi` into the app's test field. See the detailed documents for limits.

The snapshot passed 83 workspace library tests (one opt-in native integration
test ignored), cargo formatting, TypeScript/Vite build and native app/DMG build
and signature checks. Tests, formatting and the frontend build were rerun when
publishing the fork. Strict Clippy stops at `clippy::manual_div_ceil` in
`crates/ak820-protocol/src/legacy.rs` (TFT frame delay conversion); subsequent
lint findings may still be hidden by that first error. Clean up and rerun all
required CI checks after upstream integration.

Do not replace or rebuild the user's installed app merely to prepare source
contributions: a changed local signature invalidates its Accessibility approval.
Do not apply experimental hardware settings without preserving the user's
existing setup. Native UI interactions use the computer-use tool; terminal
checks should not simulate UI input.

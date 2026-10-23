# Mac-side key remapping and macros

For feature-report AK820 keyboards (`0C45:8009` / `0C45:800A`, usage page
`FF13`), this build supplies key remapping and macros through macOS. These
features do not upload an undocumented key table or macro to the firmware.
The previous onboard implementation remains available for the original
output-report devices.

## Key remapping

Select a key in the keyboard view, choose its destination and click **Save &
apply**. **Capture a key** identifies the physical code macOS receives if a
legend differs from the chosen layout. The **Key test** area displays the
resulting code. Macro destinations appear after a nonempty macro is saved.

The application uses Apple's `IOHIDEventSystemClient` / `UserKeyMapping`
API. It matches only wired AK820 keyboard services with vendor `0C45` and
product `8009` or `800A`; other keyboards and mice are excluded. Multiple
matching AK820s with the same IDs share the profile.

Every service's proposed map is validated before any write. Unrelated
entries are preserved, read-back verifies each update, and a failed update
rolls back already changed services. A conflicting change from another tool
is reported, not overwritten. Pausing or removing this profile preserves
an external tool's replacement mapping.

A local profile is restored on app launch and reconnect. Pause the profile
and save to temporarily remove its mappings. A normal app exit cancels
playback, releases held macro keys and removes owned mappings. A crash can
leave macOS mappings active until restart/reconnect; reopening the app and
pausing the profile restores them. The app does not install a login agent.

The hardware Fn layer and volume knob remain firmware-controlled. Mac-side
mappings do not travel to another computer or become onboard firmware maps.

## Macros

The Macros view records keyboard events only while recording is enabled in
that window. Escape or losing window focus stops recording. Repeated key-down
notifications and orphan releases are ignored; remaining presses get matching
releases when recording stops. Events and names are saved locally.

A macro can contain 512 events and up to 60 seconds of delays. Incomplete
press/release pairs, unknown keys, mouse events and recursive trigger keys
are rejected before playback. Saved macros can be played after a 3-second
countdown, allowing you to focus the destination or the built-in test area.
**Stop playback** cancels the countdown or playback; cleanup releases held
keys. Pressing a bound trigger a second time also stops playback.

Bindings use F13–F20 marker keys, allocated without colliding with the
existing automation library or explicit function-key remaps. The global
shortcut listener is active while the app is open. Avoid assigning those
same markers to unrelated global shortcuts or using them from another
keyboard while macros are bound. Macro playback itself cannot emit the
reserved F13–F24 markers or their macOS aliases.

## Permissions

macOS requires **Accessibility** permission for the app to generate macro
keystrokes in other applications. Click **Open Accessibility settings**, add
or enable **AK820 Pro Modder**, then **Check permission**. Permission checks
alone do not request or grant access; only the explicit settings button
requests the system prompt.
Quit and reopen the app after first enabling Accessibility if playback
completes without producing keystrokes; macOS may retain the old access
decision until the process restarts.

Recent macOS versions may require **Input Monitoring** for alphanumeric or
punctuation remaps. The Keymap view includes a separate settings button and
permission check. Although this is a broad system permission, this app's
recorder listens only in its own window and does not record global typing.
No permission is granted by the app itself.

This is a local ad-hoc signed build. Changing or moving the app can require
re-authorizing it in macOS settings. If the switch is on but the app still
reports permission needed after restarting, remove the stale Accessibility
entry and add the current `.app` path again.

## Storage and validation

`~/Library/Application Support/io.github.wsclx.ak820pro-modder/host-controls.json`
contains the profile, named macros and trigger assignments. Updates use a
temporary file and rename. It is separate from the existing onboard macro
interface and automation/iCloud library.

Automated checks cover balanced recordings, duration limits, duplicate
sources, marker allocation, external-map preservation and Apple virtual key
codes. An explicit native test temporarily maps an unused function key on
the physical AK820, checks macOS read-back and restores the original table.

References:

- [Apple TN2450: key remapping API](https://developer.apple.com/library/archive/technotes/tn2450/_index.html)
- Installed macOS SDK: `IOHIDEventSystemClient.h`, `IOHIDServiceClient.h`,
  `IOHIDLib.h`, and HIToolbox `Events.h`.

## Local verification on 2026-10-23

- 60 workspace unit tests passed; the separate native write/read/restore
  test also passed against the connected AK820.
- In the packaged UI, Page Down → Page Up was applied and confirmed on both
  AK820 keyboard services. Quitting removed it; reopening restored it from
  the profile. The temporary mapping was then removed and normal keys restored.
- The recorder captured H press/release and I press/release. Its named macro
  saved successfully and survived an application restart.
- After the user enabled Accessibility and the app restarted, the sample
  `Compatibility playback check` macro typed `hi` into the UI test area.
  Stop cancelled a pending countdown. The user physically pressed a temporary
  Page Down binding and confirmed `hi` appeared; the UI also showed the output.
  That binding was removed and macOS read-back verified normal keys restored.
  The saved sample remains available without a key binding.
- The final build with TFT support was restarted after the user removed and
  re-added its stale Accessibility entry. Its permission check showed enabled,
  and actual playback again produced `hi` in the packaged UI test area.

# Complete keyboard setups

The **Setup profiles** sidebar view saves named configurations for the wired
ANSI AK820 feature-report revision. Each setup has independent copies of:

- Global lighting effect parameters or the full per-key colour map and brightness.
- Sleep timer (never, 1, 5 or 30 minutes).
- Mac-side remaps, macro names/actions, and the remaps-enabled setting.
- TFT image or GIF, embedded as normalized RGB565 frames and frame delays.

## Saving and applying

Configure the other app views and save/apply their settings, then choose
**Capture app settings**. This captures acknowledged app submissions and the
saved Mac controls, not settings read back from the keyboard. Earlier releases
saved per-key colours, so those can be recovered on first use; earlier sleep
settings and screen uploads cannot be inferred and remain uncaptured.

You can choose a saved colour profile, set sleep, and attach a screen image/GIF
directly in the setup editor. Captured values are copies: subsequent edits to
the colour library or original image file do not change the setup. Review
remaps and macro actions in the expandable contents panel before applying an
imported setup. Macro playback is never started by import or setup application.

Enter a name and **Save as new setup**. Select an existing entry and **Save
setup changes** to update/rename it. Save before applying edits. Applying a
setup replaces the current Mac remap and macro library. A lighting, sleep or
display value explicitly left uncaptured/unchanged is skipped during apply.
The keyboard cannot supply a screen image, sleep value, or full settings dump.
The UI does not substitute guessed defaults for these missing values.

Keep the app open for Mac remaps, macro triggers and shortcuts. Accessibility
approval is required for replaying macros. Native security approvals are not
stored in setup files or granted by applying one.

## Shortcut

**Control + Option + Command + S** cycles complete setups in list order, then
wraps to the first. It is separate from the colour-only shortcut on P. Use
Record setup shortcut and Enable / update setup shortcut to change it, or
Disable setup shortcut to release it. Shortcut preferences persist on this Mac.

Only one complete-setup application runs at a time. Repeated presses while a
setup is applying are ignored; they do not queue multiple screen uploads.
The selection advances only after all requested components complete. After a
restart, the first cycle applies the first saved setup. Macro/automation marker
registration updates preserve both profile-cycling shortcuts.

## Automatic switching by app (macOS)

In **Switch setups by app**, choose an open application and a saved setup,
then **Add app rule**. Repeat for other apps and choose **Save rules & enable**.
Open an app and use **Refresh open apps** if it is missing from the picker.
Rules match stable macOS bundle identifiers and survive app and computer restarts.
The app must remain running. The initial state is paused with no rules.

For unmatched apps, either keep the current keyboard settings or select a
fallback setup. **Save rules & pause** persists edits and stops future automatic
switches. An upload already in progress finishes before the pause takes effect.
Returning to AK820 Pro Modder never applies the fallback, so editing is possible.

The native NSWorkspace foreground identity is sampled every 400 ms and must
remain stable for 800 ms before applying a setup. This requires no new macOS
privacy approval; only app names and bundle identifiers are observed, never
window titles, content, screenshots or keystrokes. Macro replay retains its
existing Accessibility requirement.

Rules run in the backend even when another view is open. The status shows the
active app, last automatic setup, in-progress state and failures. A full upload
finishes before the next foreground app is evaluated; old app activations are
not queued. The existing setup/HID/host gates also serialize manual shortcuts.
If the same complete setup is still current, another matching app does not
repeat its TFT upload. Manual lighting, sleep, display, remap or macro changes,
and edits to a saved setup, invalidate that optimization. Manual overrides last
until the next app activation. A failed/partial apply invalidates the previous
complete-setup identity and is not retried on every poll: switch away and back,
or save/resume the rules to retry. Reconnect a disconnected keyboard first.
A rule whose setup was deleted reports an error rather than using the fallback.

Rules are local in `app-setup-rules.json` and refer to local setup IDs. They are
not included in portable setup exports. Up to 100 unique application rules can
be saved. Setup contents and the order of component application are unchanged.

## Menu bar selection

**AK820 → Saved setups** lists this same complete-setup library. Select any
name to apply it with the window closed. The submenu updates after saving,
renaming, deleting or importing, disables setup entries during an upload, and
shows the last result. **Manage saved setups…** opens this editor explicitly.
Lighting, sleep, display and Mac controls use the same apply path and error
reporting described below. Loaded macros are not replayed.

## Import and export

Export selected setup or Export all setups uses the native Save dialog and
includes embedded screen frames. The exported JSON has:

```
format: "ak820-complete-setups"
version: 1
keyboard: "ak820-pro-ansi"
profiles: [{ name, snapshot: { lighting, sleep_minutes, host, display } }]
```

Lighting is either `{ kind: "global", config: LightingConfig }` or
`{ kind: "per_key", profile: { map: CustomLedMap, brightness } }`.
Display is `{ name, animation: { frames: [{ pixels: [RGB565 bytes], delay_ms }] } }`.
An uncaptured/unchanged component is `null`. Host contains `enabled`, `remaps`,
`macros`, and computed `markers`. Imported marker assignments are discarded and
allocated against the receiving Mac's existing automation bindings before apply.

Import setups validates the entire file before changing the library. Duplicate
names gain an imported-number suffix; no saved entry is overwritten. Imports
never apply themselves to the keyboard. IDs, original image paths, shortcut
preferences, automation scripts and security approvals are not exported.
Colour-only JSON files remain a separate format for the Lighting paint editor.

The library holds up to 20 setups and 64 MB of JSON. Each screen uses the
existing 128×128, 30-frame limit. The full animation is embedded, so larger
collections may produce large files. Original source images remain limited to
10 MB during preparation.

## Apply transactions and failure reporting

The entire setup is validated, including macro references and available trigger
markers, before any configuration write. The host-control gate and HID gate
serialize the application against native remap updates and USB writes.

Application order is lighting → sleep → TFT → Mac controls. The currently
running macro is stopped first. Every requested component uses its existing
acknowledgement/validation path. Successful hardware submissions update the
last-known app cache, and UI views reload after the setup report.

A failure stops the sequence and reports the completed components, failed
component and error. Earlier completed components remain applied. Hardware
rollback is unavailable without reliable settings getters; the app does not
claim an atomic hardware restore or automatically replay a partially sent bank
or screen. Native remap commit retains its existing rollback on persistence or
shortcut-registration failure.

## Software validation

83 workspace unit tests pass, including complete setup round-trip with animated
frames, a bound macro, all-or-nothing invalid imports, invalid components,
incremental current-setting capture, cycle ordering/wraparound, and stopping
after a failed component. TypeScript/Vite and Rust formatting checks pass.

## Installed-app validation

- The native editor saved Current setup with the user's per-key colours,
  30-minute sleep, existing macro library and embedded two-frame GIF.
- Applying it completed lighting, sleep, TFT and Mac controls successfully.
- Native Save exported the complete setup including both GIF frames; native
  Open imported a separate prepared test setup without changing the keyboard.
- The user pressed Control + Option + Command + S and confirmed cyan breathing
  lighting and the four-colour test screen. The cache independently confirmed
  global lighting, sleep 5, one screen frame, and the test macro binding.
- `hidutil` showed the Page Down → F13 macro marker on both AK820 keyboard
  services. After renewed Accessibility approval and restart, the restored
  macro typed `hi` in the app test area.
- The app signature verifies strictly and the Apple Silicon DMG passes
  `hdiutil verify`.
- After the test, Current setup restored the original colours, two-frame GIF,
  30-minute sleep and Mac controls. Colour libraries and the host-control file
  match their pre-update backups; `hidutil` confirms the temporary mapping was
  removed from both services. The temporary setup was deleted and Current setup
  was left selected, with the setup shortcut enabled and persisted.

App-rule policy tests cover activation debounce, one attempt per activation,
rapid app changes while busy, fallback/keep-current behavior, ignoring the
modder itself, pause, duplicate rules and missing setup validation.

Native app-rule validation: the picker listed running applications, a persisted
Finder rule applied a temporary Gaming setup with all four component groups
acknowledged, and pause survived relaunch. Original setup and all saved data
were restored after testing; automatic rules are left paused and empty.

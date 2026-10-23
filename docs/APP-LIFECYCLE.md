# Startup and menu bar

The **App settings** sidebar page works without a connected keyboard.

- **Start at login** is optional and initially off. It registers a per-user
  LaunchAgent which starts the installed executable with `--background`.
  Login launches have no visible window or Dock icon; the keyboard menu bar
  icon remains available. macOS can additionally disable background items in
  System Settings → General → Login Items & Extensions.
- **Keep running when the window closes** is initially on. The red close
  button hides the existing window. The Dock stays available as a fallback
  after opening the window interactively. Background
  app-based setup rules, both profile shortcuts, native remaps and macro
  triggers continue in the same process. The webview is retained, including
  unsaved UI drafts. Switch this off to quit when closing instead.
- The keyboard menu bar icon labelled **AK820** offers **Show AK820 Pro Modder**, **App settings…**,
  **Hide window**, and **Quit AK820 Pro Modder**. Reopening the application from
  Applications or Finder also shows its existing window. Showing restores
  the Dock icon after a quiet login launch. The template keyboard glyph follows the menu bar appearance.
- **Quit app**, the menu bar's Quit, and Command + Q explicitly exit. Existing
  shutdown removes native key mappings and unregisters global shortcuts.
  The login item has no KeepAlive setting, so quitting does not relaunch it.
- A single-instance plugin prevents login/manual launches from creating two
  independent keyboard controllers. A second manual launch shows the existing
  window; a second `--background` launch leaves its visibility unchanged.
- Window close behavior is saved in `app-preferences.json` in the app data
  directory. Login registration is read from its native registration, rather
  than duplicated in that file. Registration is
  `~/Library/LaunchAgents/AK820 Pro Modder.plist`, using Tauri's LaunchAgent
  autostart backend. Enabling/disabling does not change any keyboard setup.
- Startup begins with a hidden window. After native setup, normal launch shows
  it; `--background` keeps it hidden. If the tray cannot initialize, background
  launch shows the window instead, and close-to-background keeps it visible
  so controls cannot become inaccessible.

The implementation retains the existing Tauri 2.11.1 runtime. Autostart 2.5.1
and single-instance 2.4.5 are pinned to compatible releases; newer releases
require a framework upgrade outside this change.

## Validation

83 workspace unit tests pass, one opt-in native integration test is ignored.
Added coverage checks that closing hides only with a working tray, keeps the
window accessible on tray failure, quits when opted out, and persists the
close preference independently of native login registration.

Native validation also exercised both setting toggles and persistence across
relaunch; quit-on-close and explicit Quit left no app process. Disabling login
removed its plist, enabling recreated valid background-only launch arguments,
and launchctl bootstrap successfully ran that job without logging the user out.
All pre-existing saved keyboard/profile/rule files match their pre-change backup.

## Notched display placement

On this Mac the initial status item was natively visible but entirely inside
its camera-notch exclusion area: x=926–1008, while the right usable menu bar
starts at x=1010. A title or extra retained reference does not solve that.

The status item now uses the stable AppKit autosave name `AK820ProModder`.
On first placement only, its own app-domain
`NSStatusItem Preferred Position AK820ProModder` default is seeded to zero,
placing it toward the right edge. Subsequent starts keep the saved preference,
so a user's Command-dragged position is respected. Native diagnostics confirmed
x=1527–1609, fully inside the right usable area, including after a restart.
No other application's menu item preference is edited.

Debug builds can record eight samples of the app's own status item geometry
and screen menu areas in `menu-bar-diagnostics.json` in the app data folder.
Production builds omit this diagnostic collection.

The user confirmed the final AK820 menu item is visible and Show reopens the
window. After renewed Accessibility approval and restart, saved macro playback
typed `hi` in the test area. Both options remain enabled and the app was left
running in the menu bar with its window closed.

## Apply setups from the menu bar

Choose **AK820 → Saved setups → a setup name** to apply that complete saved
setup without opening or focusing the app window. The submenu reads the same
validated library as Setup profiles, preserving its saved order. **Manage saved
setups…** explicitly opens the setup editor when requested.

Save, rename, delete and import refresh the native menu immediately. Native
menu construction runs on the main thread after library loading on a worker;
a generation counter discards stale refreshes. Names containing `&` are escaped
for native menu labels and selections dispatch by numeric ID, never by name.
The ID is resolved against the current library again when applying.

Menu selections share the existing complete-setup busy gate, hardware/host
locks, validation and component order with shortcuts, automatic app rules and
the setup editor. During an apply the submenu says **applying…** and profile
entries are disabled; repeated clicks do not queue additional TFT uploads.
Applying never calls the window-show path or plays macros.

The submenu reports the last apply result, including errors, the failed
component, completed components and macro-permission warnings. Long native
labels are shortened; the status icon's tooltip retains the full message and
the existing app status events retain full reports. A successful **Last applied**
line is a receipt, not device read-back or a claim that later manual edits are
still identical. Results from all setup-apply entry points update this receipt.

The library and menu start loading when the app starts, including quiet login
launches. If the library cannot load, the submenu shows the error and keeps
management available. No setup library or login settings are changed by adding
these controls.

Installed-build verification: the user selected **Current setup** from the menu,
confirmed **Last applied: Current setup**, and confirmed the window stayed
closed. Acknowledged cache writes covered lighting, setup state and host controls.
After renewed Accessibility approval and restart, the saved test macro typed
`hi` into the test area. Both startup options remain enabled; the app was left
running with its window closed, without another rebuild.

use ak820_protocol::{
    commands::keymap::Keymap,
    commands::lighting::{Direction, LightingConfig, Mode},
    commands::macros::{Macro, MACRO_BYTE_LIMIT, MACRO_SLOT_COUNT, MAX_ACTIONS_PER_MACRO},
    commands::per_key_rgb::CustomLedMap,
    commands::system::{DeviceInfoReport, GameMode, SleepPreset, SLEEP_PRESETS},
    device::ProbeReport,
    Connection, DeviceInfo,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tokio::sync::Mutex;
use tracing_subscriber::EnvFilter;

mod app_lifecycle;
mod app_setups;
#[cfg(target_os = "macos")]
mod audio_reactive;
mod automations;
mod colour_files;
mod colour_shortcuts;
mod feature_rgb;
mod host_controls;
mod icloud_sync;
mod now_playing;
mod presets;
mod setup_menu;
mod setup_profiles;
mod setup_runtime;
mod setup_shortcuts;
mod starter_library;
mod tft_images;
use automations::{Automation, RunResult};
use now_playing::NowPlaying;
use presets::Preset;
use starter_library::StarterAutomation;
use std::sync::Arc;

/// HID codes F13..F24 reserved as global-shortcut markers for automations.
/// Inclusive range — gives the user 12 keyboard-triggerable automations.
const MARKER_HID_RANGE: std::ops::RangeInclusive<u8> = 104..=115;

/// Map an HID Keyboard Usage Code to the string label
/// `tauri-plugin-global-shortcut` expects (and back-channel parses).
fn hid_to_shortcut_label(hid: u8) -> Option<&'static str> {
    match hid {
        104 => Some("F13"),
        105 => Some("F14"),
        106 => Some("F15"),
        107 => Some("F16"),
        108 => Some("F17"),
        109 => Some("F18"),
        110 => Some("F19"),
        111 => Some("F20"),
        112 => Some("F21"),
        113 => Some("F22"),
        114 => Some("F23"),
        115 => Some("F24"),
        _ => None,
    }
}

/// Re-register the global-shortcut handlers from the current on-disk
/// automations list. Called on app startup and after every
/// `save_automations` / `assign_automation_marker` / `unassign_*`.
///
/// Replaces marker registrations only — a marker can only be bound to
/// one automation at a time. Other global shortcuts remain registered.
fn refresh_automation_shortcuts(app: &AppHandle) -> Result<(), String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    let list = automations::load(dir.clone())?;
    let host_config = host_controls::load(&dir)?;
    if host_config.enabled
        && host_config.markers.values().any(|marker| {
            list.iter()
                .any(|a| a.marker_hid.map(u16::from) == Some(*marker))
        })
    {
        return Err(
            "An automation and a macro share a trigger marker. Choose a different binding.".into(),
        );
    }
    let mgr = app.global_shortcut();
    // Refresh marker bindings without unregistering the user's colour hotkey.
    for hid in MARKER_HID_RANGE {
        if let Some(label) = hid_to_shortcut_label(hid) {
            if mgr.is_registered(label) {
                mgr.unregister(label).map_err(|e| e.to_string())?;
            }
        }
    }
    for a in list {
        let Some(marker) = a.marker_hid else { continue };
        let Some(label) = hid_to_shortcut_label(marker) else {
            continue;
        };
        let shortcut: Shortcut = label.parse().map_err(|e| format!("parse {label}: {e}"))?;
        let automation_id = a.id;
        let app_handle = app.clone();
        mgr.on_shortcut(shortcut, move |_app, _sc, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let app = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                run_automation_by_id(&app, automation_id).await;
            });
        })
        .map_err(|e| format!("on_shortcut({label}): {e}"))?;
        tracing::info!(automation_id, label, "registered global-shortcut marker");
    }
    if host_config.enabled {
        for (macro_id, marker) in host_config.markers {
            let label = hid_to_shortcut_label(marker as u8).ok_or("Invalid macro trigger")?;
            let shortcut: Shortcut = label
                .parse()
                .map_err(|e| format!("parse macro trigger: {e}"))?;
            let app_handle = app.clone();
            mgr.on_shortcut(shortcut, move |_app, _shortcut, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                let host = app_handle.state::<Arc<host_controls::HostControls>>();
                if host.playback.is_running() {
                    host.playback.stop();
                    return;
                }
                let result = (|| {
                    let dir = app_handle
                        .path()
                        .app_data_dir()
                        .map_err(|e| e.to_string())?;
                    let cfg = host_controls::load(&dir)?;
                    let m = cfg
                        .macros
                        .into_iter()
                        .find(|m| m.macro_id == macro_id)
                        .ok_or("Macro no longer exists")?;
                    host.playback.start(m, 80)
                })();
                if let Err(error) = result {
                    host.playback.record_error(error.clone());
                    tracing::warn!(%error, "macro trigger failed");
                }
            })
            .map_err(|e| format!("Register {label}: {e}"))?;
        }
    }
    Ok(())
}

async fn run_automation_by_id(app: &AppHandle, id: u64) {
    let dir = match app.path().app_data_dir() {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!("app_data_dir: {e}");
            return;
        }
    };
    let dir_clone = dir.clone();
    let list = match tokio::task::spawn_blocking(move || automations::load(dir_clone)).await {
        Ok(Ok(l)) => l,
        Ok(Err(e)) => {
            tracing::warn!("load: {e}");
            return;
        }
        Err(e) => {
            tracing::warn!("join: {e}");
            return;
        }
    };
    let Some(automation) = list.into_iter().find(|a| a.id == id) else {
        tracing::warn!("automation {id} no longer exists");
        return;
    };
    let name = automation.name.clone();
    let res = tokio::task::spawn_blocking(move || automations::run(&automation))
        .await
        .unwrap_or_else(|_| automations::RunResult {
            exit_code: None,
            stdout: String::new(),
            stderr: "spawn_blocking failed".into(),
            success: false,
        });
    tracing::info!(
        automation_id = id,
        automation_name = %name,
        success = res.success,
        exit_code = ?res.exit_code,
        "shortcut-triggered automation done"
    );
}

#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    #[error("protocol error: {0}")]
    Protocol(String),
}

impl From<ak820_protocol::Error> for AppError {
    fn from(value: ak820_protocol::Error) -> Self {
        AppError::Protocol(value.to_string())
    }
}

/// Holds a single open control connection across calls so we don't
/// re-enumerate and re-open the HID device on every UI tick.
///
/// **Why `tokio::sync::Mutex`** (not `std::sync::Mutex`):
/// HID I/O is synchronous and can block for tens of milliseconds per call.
/// With a `std` mutex, an awaiting Tauri command parks its tokio worker
/// thread; under modest concurrency that exhausts the worker pool and the
/// WKWebView freezes (we hit this on the System, Macros, and Keymap views).
/// The tokio mutex suspends only the awaiting *task*, leaving the worker
/// free for unrelated commands (probe polling, frontend rendering callbacks,
/// etc.). All HID-touching commands are now `async fn` and acquire this
/// mutex via `.lock().await`.
#[derive(Default)]
pub struct ConnState(Mutex<Option<Connection>>);

impl ConnState {
    /// Acquire the connection slot, run a synchronous closure that may touch
    /// HID, and release the lock. Wraps `tokio::sync::Mutex::lock().await`
    /// so callers don't repeat the boilerplate. The closure itself stays
    /// sync because the underlying `hidapi` I/O is blocking — that's
    /// acceptable because we hold the mutex for short bursts and the
    /// concurrent runtime can still progress other tasks.
    ///
    /// **Auto-reconnect**: if a *cached* `Connection` returns a stale-handle
    /// error (device unplugged, USB sleep, hidapi handle invalidated), the
    /// slot is cleared and the closure runs once more. The retry re-enters
    /// `ensure_open()`, which re-enumerates and re-opens the HID device —
    /// so the first action after a re-plug succeeds transparently without
    /// the user having to click Refresh twice.
    ///
    /// We require `FnMut` (not `FnOnce`) for this. All current call sites
    /// borrow their inputs (`&keymap`, `&map`, etc.) rather than consuming
    /// them, so the closure can be called twice without lifetime issues.
    async fn with<R, F>(&self, mut f: F) -> Result<R, AppError>
    where
        F: FnMut(&mut Option<Connection>) -> Result<R, AppError>,
    {
        let mut guard = self.0.lock().await;
        let had_cached_conn = guard.is_some();
        match f(&mut guard) {
            Ok(v) => Ok(v),
            Err(err) if had_cached_conn && is_stale_handle_error(&err) => {
                // The cached HID handle is no good. Drop it and retry once
                // with a fresh open — covers unplug/replug and macOS sleep
                // wake-up where hidapi returns "Device is disconnected".
                tracing::info!(?err, "stale HID handle, clearing slot and retrying once");
                *guard = None;
                f(&mut guard)
            }
            Err(err) => Err(err),
        }
    }
}

/// Pattern-match on the stringified error to decide whether the cached HID
/// handle is suspect. Errors raised by `Connection::open_control()` itself
/// (i.e. when the slot was already empty) shouldn't trigger a retry — the
/// caller in [`ConnState::with`] gates this with `had_cached_conn`.
fn is_stale_handle_error(err: &AppError) -> bool {
    let AppError::Protocol(msg) = err;
    // hidapi wraps platform errors with a "HID error: …" prefix, the body
    // typically mentions "disconnected" or "Device not found" depending
    // on which call (read / write / re-enumeration) tripped first.
    msg.contains("disconnected") || msg.contains("Device not found") || msg.contains("HID error")
}

pub(crate) fn ensure_open(slot: &mut Option<Connection>) -> Result<&Connection, AppError> {
    if slot.is_none() {
        *slot = Some(Connection::open_control()?);
    }
    Ok(slot.as_ref().unwrap())
}

#[tauri::command]
fn list_devices() -> Result<Vec<DeviceInfo>, AppError> {
    Ok(ak820_protocol::enumerate()?)
}

/// Read-only liveness probe. Crucially does NOT touch the cached HID handle
/// in `ConnState` — enumeration is cheap, and locking the mutex from a polling
/// loop would block view-level operations (lighting set / system reads).
#[tauri::command]
fn probe_device() -> Result<ProbeReport, AppError> {
    let candidates = ak820_protocol::enumerate()?;
    Ok(
        match candidates
            .iter()
            .find(|d| d.usage_page == ak820_protocol::legacy::USAGE_PAGE)
            .or_else(|| candidates.iter().find(|d| d.usage_page == 0xff68))
        {
            Some(d) => ProbeReport {
                connected: true,
                interface: d.interface,
                product: d.product.clone(),
                firmware_version: None,
                legacy: d.usage_page == ak820_protocol::legacy::USAGE_PAGE,
            },
            None => ProbeReport {
                connected: false,
                interface: -1,
                product: None,
                firmware_version: None,
                legacy: false,
            },
        },
    )
}

/// On-demand protocol verification, distinct from the cheap presence poll.
#[tauri::command]
async fn verify_connection(state: State<'_, Arc<ConnState>>) -> Result<ProbeReport, AppError> {
    state.with(|slot| Ok(ensure_open(slot)?.probe()?)).await
}

#[tauri::command]
async fn sync_keyboard_clock(
    state: State<'_, Arc<ConnState>>,
    time: ak820_protocol::legacy::ClockTime,
) -> Result<(), AppError> {
    state
        .with(|slot| Ok(ensure_open(slot)?.sync_clock(&time)?))
        .await
}

#[tauri::command]
async fn set_sleep_minutes(
    app: AppHandle,
    state: State<'_, Arc<ConnState>>,
    minutes: u8,
) -> Result<(), AppError> {
    let dir = host_dir(&app).map_err(AppError::Protocol)?;
    state
        .with(|slot| {
            ensure_open(slot)?.set_sleep_minutes(minutes)?;
            setup_profiles::record(&dir, |current| current.sleep_minutes = Some(minutes)).map_err(
                |e| {
                    AppError::Protocol(format!(
                        "Sleep timer applied, but saving its last value failed: {e}"
                    ))
                },
            )
        })
        .await
}

#[tauri::command]
async fn prepare_tft_image(bytes: Vec<u8>) -> Result<tft_images::PreparedImage, String> {
    tokio::task::spawn_blocking(move || tft_images::prepare(&bytes))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Clone, Serialize)]
struct TftProgress {
    upload_id: String,
    completed: usize,
    total: usize,
}

#[tauri::command]
async fn upload_tft_image(
    app: AppHandle,
    state: State<'_, Arc<ConnState>>,
    animation: ak820_protocol::commands::tft::TftAnimation,
    upload_id: String,
    name: Option<String>,
) -> Result<(), String> {
    // Validate before acquiring hardware. Never retry a partially completed
    // image transaction through ConnState's ordinary stale-handle retry.
    ak820_protocol::legacy::encode_tft(&animation).map_err(|e| e.to_string())?;
    let state = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        let dir = host_dir(&app)?;
        let display_name: String = name
            .unwrap_or_else(|| "Custom display".into())
            .chars()
            .filter(|c| !c.is_control())
            .take(128)
            .collect();
        let mut slot = state.0.blocking_lock();
        let conn = ensure_open(&mut slot).map_err(|e| e.to_string())?;
        conn.upload_feature_tft(&animation, |completed, total| {
            let _ = app.emit(
                "tft-progress",
                TftProgress {
                    upload_id: upload_id.clone(),
                    completed,
                    total,
                },
            );
        })
        .map_err(|e| e.to_string())?;
        setup_profiles::record(&dir, |current| {
            current.display = Some(setup_profiles::Display {
                name: if display_name.trim().is_empty() {
                    "Custom display".into()
                } else {
                    display_name
                },
                animation,
            })
        })
        .map_err(|e| format!("Screen uploaded, but saving its last image failed: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn close_device(state: State<'_, Arc<ConnState>>) -> Result<(), AppError> {
    state
        .with(|slot| {
            slot.take();
            Ok(())
        })
        .await
}

#[derive(Serialize)]
struct LightingModeInfo {
    name: &'static str,
    supports_direction: bool,
    directions: Vec<&'static str>,
}

#[tauri::command]
fn list_lighting_modes() -> Vec<LightingModeInfo> {
    Mode::ALL
        .iter()
        .map(|m| {
            let dirs = m.supported_directions();
            LightingModeInfo {
                name: m.name(),
                supports_direction: !dirs.is_empty(),
                directions: dirs.iter().map(direction_name).collect(),
            }
        })
        .collect()
}

#[tauri::command]
async fn get_device_info(state: State<'_, Arc<ConnState>>) -> Result<DeviceInfoReport, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_device_info()?)
        })
        .await
}

#[tauri::command]
async fn get_game_mode(state: State<'_, Arc<ConnState>>) -> Result<GameMode, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_game_mode()?)
        })
        .await
}

#[tauri::command]
async fn set_game_mode(state: State<'_, Arc<ConnState>>, mode: GameMode) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_game_mode(&mode)?;
            Ok(())
        })
        .await
}

#[tauri::command]
fn list_sleep_presets() -> Vec<SleepPreset> {
    SLEEP_PRESETS.to_vec()
}

#[tauri::command]
async fn get_keymap(state: State<'_, Arc<ConnState>>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_keymap()?)
        })
        .await
}

#[tauri::command]
async fn get_fn_keymap(state: State<'_, Arc<ConnState>>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_fn_keymap()?)
        })
        .await
}

/// Read the firmware's factory-default base-layer keymap without touching
/// the user's current state. The UI uses this to stage a reset that the
/// user reviews before pressing Save.
#[tauri::command]
async fn get_default_keymap(state: State<'_, Arc<ConnState>>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_default_keymap()?)
        })
        .await
}

#[tauri::command]
async fn get_default_fn_keymap(state: State<'_, Arc<ConnState>>) -> Result<Keymap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_default_fn_keymap()?)
        })
        .await
}

#[tauri::command]
async fn set_keymap(state: State<'_, Arc<ConnState>>, keymap: Keymap) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_keymap(&keymap)?;
            Ok(())
        })
        .await
}

#[tauri::command]
async fn set_fn_keymap(state: State<'_, Arc<ConnState>>, keymap: Keymap) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_fn_keymap(&keymap)?;
            Ok(())
        })
        .await
}

#[tauri::command]
async fn get_macros(state: State<'_, Arc<ConnState>>) -> Result<Vec<Macro>, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_macros()?)
        })
        .await
}

#[tauri::command]
async fn set_macros(state: State<'_, Arc<ConnState>>, macros: Vec<Macro>) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_macros(&macros)?;
            Ok(())
        })
        .await
}

#[tauri::command]
async fn get_custom_led(state: State<'_, Arc<ConnState>>) -> Result<CustomLedMap, AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            Ok(conn.get_custom_led()?)
        })
        .await
}

#[tauri::command]
async fn set_custom_led(
    state: State<'_, Arc<ConnState>>,
    map: CustomLedMap,
) -> Result<(), AppError> {
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_custom_led(&map)?;
            Ok(())
        })
        .await
}

#[tauri::command]
fn get_feature_rgb_profile(app: AppHandle) -> Result<Option<feature_rgb::Profile>, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    feature_rgb::load(&dir)
}

#[tauri::command]
fn list_feature_rgb_profiles(app: AppHandle) -> Result<Vec<feature_rgb::NamedProfile>, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    feature_rgb::list_profiles(&dir)
}

#[tauri::command]
fn save_feature_rgb_profile(
    app: AppHandle,
    id: Option<u64>,
    name: String,
    profile: feature_rgb::Profile,
) -> Result<feature_rgb::NamedProfile, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    feature_rgb::save_profile(&dir, id, name, profile)
}

#[tauri::command]
fn delete_feature_rgb_profile(app: AppHandle, id: u64) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    feature_rgb::delete_profile(&dir, id)
}

#[tauri::command]
async fn apply_feature_rgb(
    app: AppHandle,
    profile: feature_rgb::Profile,
    profile_id: Option<u64>,
) -> Result<(), String> {
    tokio::task::spawn_blocking(move || colour_shortcuts::apply_locked(&app, profile, profile_id))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
struct MacroLimits {
    slot_count: usize,
    byte_limit: usize,
    max_actions_per_macro: usize,
}

#[tauri::command]
fn macro_limits() -> MacroLimits {
    MacroLimits {
        slot_count: MACRO_SLOT_COUNT,
        byte_limit: MACRO_BYTE_LIMIT,
        max_actions_per_macro: MAX_ACTIONS_PER_MACRO,
    }
}

/// List every saved automation. Returns an empty list on first launch
/// (no file yet) — that's deliberate, not an error.
#[tauri::command]
async fn list_automations(app: tauri::AppHandle) -> Result<Vec<Automation>, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    tokio::task::spawn_blocking(move || automations::load(dir))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?
        .map_err(AppError::Protocol)
}

#[tauri::command]
async fn save_automations(app: AppHandle, list: Vec<Automation>) -> Result<(), AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let list_clone = list.clone();
    tokio::task::spawn_blocking(move || automations::save(dir, &list_clone))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?
        .map_err(AppError::Protocol)?;
    refresh_automation_shortcuts(&app).map_err(AppError::Protocol)?;
    Ok(())
}

/// Bind an automation to a keyboard marker (one of HID 104..=115 = F13..F24).
/// If the automation already has a marker, returns it unchanged. Otherwise
/// picks the first free marker, persists, and re-registers the global
/// hotkey handlers. The frontend then writes a keyboard slot with the
/// returned HID code so the physical key emits the marker, which the
/// listener catches and runs the automation.
#[tauri::command]
async fn assign_automation_marker(
    app: AppHandle,
    id: u64,
    suggested: Option<u8>,
) -> Result<u8, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let chosen = {
        let dir = dir.clone();
        tokio::task::spawn_blocking(move || -> Result<u8, String> {
            let mut list = automations::load(dir.clone())?;
            let pos = list
                .iter()
                .position(|a| a.id == id)
                .ok_or_else(|| format!("automation {id} not found"))?;
            if let Some(m) = list[pos].marker_hid {
                return Ok(m);
            }
            let mut used: std::collections::HashSet<u8> =
                list.iter().filter_map(|a| a.marker_hid).collect();
            used.extend(
                host_controls::load(&dir)?
                    .markers
                    .values()
                    .map(|marker| *marker as u8),
            );
            let chosen = match suggested {
                Some(s) if MARKER_HID_RANGE.contains(&s) && !used.contains(&s) => s,
                _ => MARKER_HID_RANGE
                    .clone()
                    .find(|c| !used.contains(c))
                    .ok_or_else(|| {
                        "All 12 marker slots (F13–F24) already bound — unassign one first."
                            .to_string()
                    })?,
            };
            list[pos].marker_hid = Some(chosen);
            list[pos].updated_at = unix_millis();
            automations::save(dir, &list)?;
            Ok(chosen)
        })
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?
        .map_err(AppError::Protocol)?
    };
    refresh_automation_shortcuts(&app).map_err(AppError::Protocol)?;
    Ok(chosen)
}

/// Clear an automation's marker assignment. Idempotent — no-op if the
/// automation had no marker. After clearing, the global hotkey is
/// unregistered; any keyboard slot that still emits the old marker just
/// types F13/F14/… as a regular HID keystroke.
#[tauri::command]
async fn unassign_automation_marker(app: AppHandle, id: u64) -> Result<(), AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let dir_clone = dir.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let mut list = automations::load(dir_clone.clone())?;
        if let Some(pos) = list.iter().position(|a| a.id == id) {
            if list[pos].marker_hid.is_some() {
                list[pos].marker_hid = None;
                list[pos].updated_at = unix_millis();
                automations::save(dir_clone, &list)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Protocol(format!("join: {e}")))?
    .map_err(AppError::Protocol)?;
    refresh_automation_shortcuts(&app).map_err(AppError::Protocol)?;
    Ok(())
}

fn unix_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tauri::command]
async fn run_automation(app: tauri::AppHandle, id: u64) -> Result<RunResult, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let list = tokio::task::spawn_blocking({
        let dir = dir.clone();
        move || automations::load(dir)
    })
    .await
    .map_err(|e| AppError::Protocol(format!("join: {e}")))?
    .map_err(AppError::Protocol)?;
    let automation = list
        .into_iter()
        .find(|a| a.id == id)
        .ok_or_else(|| AppError::Protocol(format!("automation {id} not found")))?;
    let res = tokio::task::spawn_blocking(move || automations::run(&automation))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?;
    Ok(res)
}

/// Enumerate macOS Shortcuts the user has installed. Empty list on
/// non-macOS or when the `shortcuts` CLI isn't available (pre-Monterey).
#[tauri::command]
async fn list_shortcuts() -> Vec<String> {
    tokio::task::spawn_blocking(automations::list_shortcuts)
        .await
        .unwrap_or_default()
}

/// Curated starter library — 15 ready-to-adopt examples so the
/// Automations tab isn't a blank slate on first launch.
#[tauri::command]
fn get_starter_library() -> Vec<StarterAutomation> {
    starter_library::library()
}

/// Curated cross-cutting presets (lighting + keymap overrides + automation
/// seeds) for common use-cases. Returns every preset with its full payload
/// so the UI can render a preview without an extra round-trip.
#[tauri::command]
fn list_presets() -> Vec<Preset> {
    presets::library()
}

/// Which parts of a preset to apply. Lets the user pick — e.g. take only
/// the lighting from "Gaming FPS" but keep their custom keymap.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ApplyPresetOptions {
    pub lighting: bool,
    pub keymap: bool,
    pub fn_keymap: bool,
    pub automations: bool,
}

/// Result of an apply — what actually happened, surfaced to the UI.
#[derive(Debug, Clone, Serialize, Default)]
pub struct ApplyPresetReport {
    pub lighting_applied: bool,
    pub keymap_slots_changed: usize,
    pub fn_keymap_slots_changed: usize,
    pub automations_added: usize,
    pub automations_skipped_existing: usize,
    pub warnings: Vec<String>,
}

#[tauri::command]
async fn apply_preset(
    app: AppHandle,
    state: State<'_, Arc<ConnState>>,
    id: String,
    options: ApplyPresetOptions,
) -> Result<ApplyPresetReport, AppError> {
    let preset =
        presets::find(&id).ok_or_else(|| AppError::Protocol(format!("preset '{id}' not found")))?;
    // Reject an unsupported combination before applying its lighting or
    // automation components, avoiding a partially applied preset.
    if options.keymap || options.fn_keymap {
        state.with(|slot| {
            if ensure_open(slot)?.is_legacy() {
                return Err(AppError::Protocol("Onboard key overrides are unavailable on this revision. Select lighting and automations only.".into()));
            }
            Ok(())
        }).await?;
    }
    let mut report = ApplyPresetReport::default();

    // 1. Lighting — single SET_LED_EFFECT write, optional.
    if options.lighting {
        if let Some(cfg) = preset.lighting.as_ref() {
            let cfg = cfg.clone();
            state
                .with(|slot| {
                    let conn = ensure_open(slot)?;
                    conn.set_lighting(&cfg)?;
                    if conn.is_legacy() {
                        let dir = host_dir(&app).map_err(AppError::Protocol)?;
                        setup_profiles::record(&dir, |current| {
                            current.lighting = Some(setup_profiles::Lighting::Global {
                                config: cfg.clone(),
                            })
                        })
                        .map_err(AppError::Protocol)?;
                        let _ = app.emit("setup-global-lighting", &cfg);
                    }
                    Ok(())
                })
                .await?;
            report.lighting_applied = true;
        }
    }

    // 2. Base-layer keymap overrides — read-modify-write the existing
    // 128-slot keymap so unchanged slots stay untouched.
    if options.keymap && !preset.keymap_overrides.is_empty() {
        let overrides = preset.keymap_overrides.clone();
        let changed = state
            .with(|slot| {
                let conn = ensure_open(slot)?;
                let mut km = conn.get_keymap()?;
                let mut changed = 0usize;
                for (s, action) in &overrides {
                    let i = *s as usize;
                    if i < km.slots.len() && km.slots[i] != *action {
                        km.slots[i] = action.clone();
                        changed += 1;
                    }
                }
                if changed > 0 {
                    conn.set_keymap(&km)?;
                }
                Ok(changed)
            })
            .await?;
        report.keymap_slots_changed = changed;
    }

    // 3. Fn-layer keymap overrides.
    if options.fn_keymap && !preset.fn_keymap_overrides.is_empty() {
        let overrides = preset.fn_keymap_overrides.clone();
        let changed = state
            .with(|slot| {
                let conn = ensure_open(slot)?;
                let mut km = conn.get_fn_keymap()?;
                let mut changed = 0usize;
                for (s, action) in &overrides {
                    let i = *s as usize;
                    if i < km.slots.len() && km.slots[i] != *action {
                        km.slots[i] = action.clone();
                        changed += 1;
                    }
                }
                if changed > 0 {
                    conn.set_fn_keymap(&km)?;
                }
                Ok(changed)
            })
            .await?;
        report.fn_keymap_slots_changed = changed;
    }

    // 4. Automation seeds — add to the user's library, skip names that
    // are already present (no destructive overwrite).
    if options.automations {
        let seeds = presets::seeds_for(&preset);
        if !seeds.is_empty() {
            let dir = app
                .path()
                .app_data_dir()
                .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
            let dir_clone = dir.clone();
            let added = tokio::task::spawn_blocking(move || -> Result<(usize, usize), String> {
                let mut list = automations::load(dir_clone.clone())?;
                let mut added = 0usize;
                let mut skipped = 0usize;
                for s in &seeds {
                    if list.iter().any(|a| a.name == s.name) {
                        skipped += 1;
                        continue;
                    }
                    let now = unix_millis();
                    list.push(automations::Automation {
                        id: now as u64 + added as u64,
                        name: s.name.into(),
                        description: s.description.into(),
                        kind: s.kind,
                        payload: s.payload.into(),
                        created_at: now,
                        updated_at: now,
                        marker_hid: None,
                    });
                    added += 1;
                }
                automations::save(dir_clone, &list)?;
                Ok((added, skipped))
            })
            .await
            .map_err(|e| AppError::Protocol(format!("join: {e}")))?
            .map_err(AppError::Protocol)?;
            report.automations_added = added.0;
            report.automations_skipped_existing = added.1;
        }
    }

    Ok(report)
}

/// macOS Now-Playing snapshot — covers Music.app and Spotify desktop today.
/// Returns the "nothing playing" sentinel on non-macOS or when nothing is
/// playing, distinguishing both from infrastructure failure (which surfaces
/// as `Err` via the `osascript` exit code).
#[tauri::command]
async fn get_now_playing() -> Result<NowPlaying, AppError> {
    // osascript is a cheap subprocess (~50–200 ms). Run it on tokio's
    // blocking pool so it doesn't tie up an async worker.
    let result = tokio::task::spawn_blocking(now_playing::fetch)
        .await
        .map_err(|e| AppError::Protocol(format!("now-playing join error: {e}")))?;
    Ok(result.unwrap_or_else(NowPlaying::none))
}

/// Drop any cached HID handle so the next call has to re-enumerate and re-open
/// the device. The UI exposes this as the manual "Reconnect" action and we
/// also use it internally when the keyboard goes away (unplug, sleep, etc.).
#[tauri::command]
async fn force_reconnect(state: State<'_, Arc<ConnState>>) -> Result<(), AppError> {
    state
        .with(|slot| {
            slot.take();
            Ok(())
        })
        .await
}

#[tauri::command]
async fn apply_lighting(
    app: AppHandle,
    state: State<'_, Arc<ConnState>>,
    config: LightingConfig,
) -> Result<(), AppError> {
    // Stale-handle retry is now handled generically by `ConnState::with`.
    state
        .with(|slot| {
            let conn = ensure_open(slot)?;
            conn.set_lighting(&config)?;
            if conn.is_legacy() {
                let dir = host_dir(&app).map_err(AppError::Protocol)?;
                setup_profiles::record(&dir, |current| {
                    current.lighting = Some(setup_profiles::Lighting::Global {
                        config: config.clone(),
                    })
                })
                .map_err(|e| {
                    AppError::Protocol(format!(
                        "Lighting applied, but saving its last value failed: {e}"
                    ))
                })?;
            }
            Ok(())
        })
        .await
}

fn direction_name(d: &Direction) -> &'static str {
    match d {
        Direction::Left => "left",
        Direction::Down => "down",
        Direction::Up => "up",
        Direction::Right => "right",
    }
}

/* ---------------------------------------------- audio-reactive (Phase 6) --
 *
 * Three commands the Lighting view uses:
 *   audio_reactive_start    - open SCStream, switch firmware to Custom,
 *                             begin streaming Frame→CustomLedMap at 30 fps.
 *                             Returns Err on TCC-denied / no displays /
 *                             permission-revoked-at-runtime cases so the
 *                             toggle can revert and show a useful message.
 *   audio_reactive_stop     - signal the loop, return immediately. The
 *                             loop self-clears its handle slot ≤33 ms later.
 *   audio_reactive_status   - non-blocking is-running probe for the UI's
 *                             periodic poll.
 *
 * On non-macOS hosts (no SCStream) the commands stay registered but each
 * returns `Err("audio-reactive requires macOS 13+")` so the UI gets a
 * clean message rather than a missing-command error.
 */

#[cfg(target_os = "macos")]
#[tauri::command]
async fn audio_reactive_start(
    conn: State<'_, Arc<ConnState>>,
    audio: State<'_, Arc<audio_reactive::AudioReactiveState>>,
) -> Result<(), AppError> {
    conn.with(|slot| {
        if ensure_open(slot)?.is_legacy() {
            return Err(AppError::Protocol("Audio-reactive lighting requires per-key RGB, which is unavailable on this keyboard revision.".into()));
        }
        Ok(())
    }).await?;
    let audio = audio.inner().clone();
    let conn = conn.inner().clone();
    audio.start(conn).await
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn audio_reactive_stop(
    audio: State<'_, Arc<audio_reactive::AudioReactiveState>>,
) -> Result<(), AppError> {
    audio.stop().await;
    Ok(())
}

#[cfg(target_os = "macos")]
#[tauri::command]
async fn audio_reactive_status(
    audio: State<'_, Arc<audio_reactive::AudioReactiveState>>,
) -> Result<bool, AppError> {
    Ok(audio.is_running().await)
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn audio_reactive_start() -> Result<(), AppError> {
    Err(AppError::Protocol(
        "audio-reactive requires macOS 13+ (ScreenCaptureKit not available)".into(),
    ))
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn audio_reactive_stop() -> Result<(), AppError> {
    Ok(())
}

#[cfg(not(target_os = "macos"))]
#[tauri::command]
async fn audio_reactive_status() -> Result<bool, AppError> {
    Ok(false)
}

/* --------------------------------------------------- iCloud Sync (Phase 6) --
 *
 * Backend is a thin transport: detect the iCloud Drive root, copy a file
 * each way, report mtime. The frontend keeps the "enabled" flag (in
 * localStorage) and orchestrates *when* to push or pull — on app start
 * (pull), after every save (push), or via the manual "Sync Now" button.
 *
 * Three commands:
 *   icloud_sync_status   - cheap probe: is iCloud Drive present + does
 *                           the remote automations file exist?
 *   icloud_sync_push     - copy local automations.json → iCloud
 *   icloud_sync_pull     - copy iCloud automations.json → local *only*
 *                           if the iCloud copy is strictly newer
 *
 * `push` and `pull` both return the resulting remote mtime (or `None`
 * on pull when nothing was newer) so the UI can render "last synced: …".
 */

#[tauri::command]
async fn icloud_sync_status() -> Result<icloud_sync::SyncStatus, AppError> {
    // Spawn-blocking because `std::fs::metadata` does I/O and we don't
    // want to stall the tokio worker even briefly on a slow disk.
    tokio::task::spawn_blocking(icloud_sync::status)
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))
}

#[tauri::command]
async fn icloud_sync_push(app: AppHandle) -> Result<u64, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let local_path = automations::storage_path(dir);
    tokio::task::spawn_blocking(move || icloud_sync::push_automations(&local_path))
        .await
        .map_err(|e| AppError::Protocol(format!("join: {e}")))?
}

#[tauri::command]
async fn icloud_sync_pull(app: AppHandle) -> Result<Option<u64>, AppError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Protocol(format!("app_data_dir: {e}")))?;
    let local_path = automations::storage_path(dir);
    let result =
        tokio::task::spawn_blocking(move || icloud_sync::pull_automations_if_newer(&local_path))
            .await
            .map_err(|e| AppError::Protocol(format!("join: {e}")))??;
    // After a successful pull we need to re-register the global
    // shortcuts because the marker bindings may have changed. Same
    // call as `save_automations` runs after a write.
    if result.is_some() {
        refresh_automation_shortcuts(&app).map_err(AppError::Protocol)?;
    }
    Ok(result)
}

// Mac-side keyboard controls. These do not use undocumented firmware writes.
fn host_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}
fn commit_host_config(
    app: &AppHandle,
    host: &host_controls::HostControls,
    previous: &host_controls::Config,
    next: host_controls::Config,
) -> Result<host_controls::Status, String> {
    let dir = host_dir(app)?;
    let reserved = automations::load(dir.clone())?
        .into_iter()
        .filter_map(|a| a.marker_hid.map(u16::from))
        .collect();
    let next = host_controls::prepare(next, &reserved)?;
    host.commit(&dir, previous, &next)?;
    if let Err(error) = refresh_automation_shortcuts(app) {
        let rollback = host.commit(&dir, &next, previous);
        let restore = refresh_automation_shortcuts(app);
        return Err(format!("Could not register macro triggers: {error}. Restore profile: {rollback:?}; restore shortcuts: {restore:?}"));
    }
    host.status(&dir)
}
#[tauri::command]
async fn get_host_controls(
    app: AppHandle,
    host: State<'_, Arc<host_controls::HostControls>>,
) -> Result<host_controls::Status, String> {
    let _guard = host.gate.lock().await;
    let _ = host.reapply(&host_dir(&app)?);
    host.status(&host_dir(&app)?)
}
#[tauri::command]
async fn save_host_remaps(
    app: AppHandle,
    host: State<'_, Arc<host_controls::HostControls>>,
    remaps: Vec<host_controls::Remap>,
    enabled: bool,
) -> Result<host_controls::Status, String> {
    let _guard = host.gate.lock().await;
    host.playback.stop_and_wait();
    let previous = host_controls::load(&host_dir(&app)?)?;
    let mut next = previous.clone();
    next.remaps = remaps;
    next.enabled = enabled;
    commit_host_config(&app, &host, &previous, next)
}
#[tauri::command]
async fn get_host_macros(app: AppHandle) -> Result<Vec<Macro>, String> {
    Ok(host_controls::load(&host_dir(&app)?)?.macros)
}
#[tauri::command]
fn host_macro_limits() -> MacroLimits {
    MacroLimits {
        slot_count: 100,
        byte_limit: 4 + host_controls::MAX_ACTIONS * 4,
        max_actions_per_macro: host_controls::MAX_ACTIONS,
    }
}
#[tauri::command]
async fn save_host_macros(
    app: AppHandle,
    host: State<'_, Arc<host_controls::HostControls>>,
    macros: Vec<Macro>,
) -> Result<(), String> {
    let _guard = host.gate.lock().await;
    host.playback.stop_and_wait();
    let previous = host_controls::load(&host_dir(&app)?)?;
    let mut next = previous.clone();
    next.macros = macros;
    commit_host_config(&app, &host, &previous, next)?;
    Ok(())
}
#[tauri::command]
async fn play_host_macro(
    app: AppHandle,
    host: State<'_, Arc<host_controls::HostControls>>,
    macro_id: u8,
) -> Result<(), String> {
    let cfg = host_controls::load(&host_dir(&app)?)?;
    let m = cfg
        .macros
        .into_iter()
        .find(|m| m.macro_id == macro_id)
        .ok_or("Save the macro before playing it")?;
    host.playback.start(m, 3000)
}
#[tauri::command]
fn stop_host_macro(host: State<'_, Arc<host_controls::HostControls>>) {
    host.playback.stop();
}
#[tauri::command]
fn host_playback_status(host: State<'_, Arc<host_controls::HostControls>>) -> serde_json::Value {
    serde_json::json!({ "running": host.playback.is_running(), "accessibility": host_controls::accessibility(), "error": host.playback.last_error() })
}
#[tauri::command]
fn open_keyboard_accessibility_settings() -> Result<(), String> {
    host_controls::request_accessibility();
    std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status()
        .map_err(|e| e.to_string())
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err("Could not open Accessibility settings".into())
            }
        })
}

#[tauri::command]
fn open_keyboard_input_settings() -> Result<(), String> {
    host_controls::request_input_monitoring();
    std::process::Command::new("/usr/bin/open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent")
        .status()
        .map_err(|e| e.to_string())
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err("Could not open Input Monitoring settings".into())
            }
        })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _| {
            if !args.iter().any(|a| a == "--background") {
                app_lifecycle::show(app);
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .args(["--background"])
                .app_name("AK820 Pro Modder")
                .build(),
        )
        .manage(Arc::new(app_lifecycle::Runtime::default()))
        .manage(Arc::new(setup_menu::Runtime::default()))
        .manage(Arc::new(ConnState::default()))
        .manage(Arc::new(host_controls::HostControls::default()))
        .manage(Arc::new(colour_shortcuts::Runtime::default()))
        .manage(Arc::new(setup_shortcuts::Runtime::default()))
        .manage(Arc::new(app_setups::Runtime::default()))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init());

    // Audio-reactive state is macOS-only; on Linux/Windows we still
    // expose the three Tauri commands (they stub out with "requires
    // macOS 13+") so the frontend can render its disabled state
    // without conditionally importing.
    #[cfg(target_os = "macos")]
    let builder = builder.manage(Arc::new(audio_reactive::AudioReactiveState::default()));

    builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .invoke_handler(tauri::generate_handler![
            app_lifecycle::app_lifecycle_status,
            app_lifecycle::set_start_at_login,
            app_lifecycle::set_keep_running_on_close,
            app_lifecycle::hide_to_menu_bar,
            app_lifecycle::quit_keyboard_app,
            setup_runtime::get_setup_snapshot,
            setup_runtime::list_setup_profiles,
            setup_runtime::save_setup_profile,
            setup_runtime::delete_setup_profile,
            setup_runtime::apply_setup_profile,
            setup_runtime::get_saved_sleep,
            setup_runtime::get_saved_tft,
            setup_runtime::export_setup_profiles,
            setup_runtime::import_setup_profiles,
            setup_shortcuts::setup_shortcut_status,
            setup_shortcuts::save_setup_shortcut,
            app_setups::app_setup_status,
            app_setups::save_app_setup_rules,
            app_setups::list_setup_apps,
            colour_files::export_colour_profiles,
            colour_files::import_colour_profiles,
            colour_shortcuts::colour_shortcut_status,
            colour_shortcuts::save_colour_shortcut,
            get_host_controls,
            save_host_remaps,
            get_host_macros,
            save_host_macros,
            host_macro_limits,
            play_host_macro,
            stop_host_macro,
            host_playback_status,
            open_keyboard_accessibility_settings,
            open_keyboard_input_settings,
            list_devices,
            probe_device,
            verify_connection,
            sync_keyboard_clock,
            set_sleep_minutes,
            prepare_tft_image,
            upload_tft_image,
            close_device,
            list_lighting_modes,
            apply_lighting,
            get_device_info,
            get_game_mode,
            set_game_mode,
            list_sleep_presets,
            force_reconnect,
            get_keymap,
            get_fn_keymap,
            get_default_keymap,
            get_default_fn_keymap,
            set_keymap,
            set_fn_keymap,
            get_macros,
            set_macros,
            macro_limits,
            get_custom_led,
            set_custom_led,
            get_feature_rgb_profile,
            list_feature_rgb_profiles,
            save_feature_rgb_profile,
            delete_feature_rgb_profile,
            apply_feature_rgb,
            get_now_playing,
            list_automations,
            save_automations,
            run_automation,
            list_shortcuts,
            get_starter_library,
            assign_automation_marker,
            unassign_automation_marker,
            list_presets,
            apply_preset,
            audio_reactive_start,
            audio_reactive_stop,
            audio_reactive_status,
            icloud_sync_status,
            icloud_sync_push,
            icloud_sync_pull,
        ])
        .setup(|app| {
            use tauri::Manager;
            use tauri::menu::{AboutMetadataBuilder, Menu, MenuItem, PredefinedMenuItem, SubmenuBuilder};

            // About-dialog payload: version, copyright, "by wsclx" attribution,
            // and a homepage link so users discover the GitHub repo.
            let about_meta = AboutMetadataBuilder::new()
                .name(Some("AK820 Pro Modder"))
                .version(Some(env!("CARGO_PKG_VERSION")))
                .copyright(Some("Copyright (c) 2026 wsclx · MIT licensed"))
                .authors(Some(vec!["wsclx".to_string()]))
                .website(Some("https://github.com/wsclx/ak820pro-modder"))
                .website_label(Some("github.com/wsclx/ak820pro-modder"))
                .comments(Some(
                    "Open-source, macOS-first control software for the Epomaker / Ajazz AK820 Pro mechanical keyboard.",
                ))
                .build();

            // Native macOS-style menu with the shortcuts users expect.
            let app_menu = SubmenuBuilder::new(app, "AK820 Pro Modder")
                .item(&PredefinedMenuItem::about(app, Some("About AK820 Pro Modder"), Some(about_meta))?)
                .separator()
                .item(&PredefinedMenuItem::hide(app, None)?)
                .item(&PredefinedMenuItem::hide_others(app, None)?)
                .item(&PredefinedMenuItem::show_all(app, None)?)
                .separator()
                .item(&PredefinedMenuItem::quit(app, None)?)
                .build()?;

            let edit_menu = SubmenuBuilder::new(app, "Edit")
                .item(&PredefinedMenuItem::undo(app, None)?)
                .item(&PredefinedMenuItem::redo(app, None)?)
                .separator()
                .item(&PredefinedMenuItem::cut(app, None)?)
                .item(&PredefinedMenuItem::copy(app, None)?)
                .item(&PredefinedMenuItem::paste(app, None)?)
                .item(&PredefinedMenuItem::select_all(app, None)?)
                .build()?;

            let view_menu = SubmenuBuilder::new(app, "View")
                .item(&MenuItem::with_id(
                    app, "reload", "Reload", true, Some("CmdOrCtrl+R"),
                )?)
                .item(&MenuItem::with_id(
                    app, "force_reload", "Force Reload", true, Some("CmdOrCtrl+Shift+R"),
                )?)
                .separator()
                .item(&MenuItem::with_id(
                    app, "toggle_devtools", "Toggle DevTools", true, Some("CmdOrCtrl+Alt+I"),
                )?)
                .build()?;

            let window_menu = SubmenuBuilder::new(app, "Window")
                .item(&PredefinedMenuItem::minimize(app, None)?)
                .item(&PredefinedMenuItem::maximize(app, None)?)
                .item(&PredefinedMenuItem::fullscreen(app, None)?)
                .build()?;

            let menu = Menu::with_items(app, &[&app_menu, &edit_menu, &view_menu, &window_menu])?;
            app.set_menu(menu)?;

            app.on_menu_event(|app, event| match event.id().as_ref() {
                "reload" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.eval("location.reload()");
                    }
                }
                "force_reload" => {
                    if let Some(win) = app.get_webview_window("main") {
                        // Bypass any HTTP cache the webview may hold.
                        let _ = win.eval("location.reload()");
                    }
                }
                "toggle_devtools" => {
                    if let Some(win) = app.get_webview_window("main") {
                        if win.is_devtools_open() {
                            win.close_devtools();
                        } else {
                            win.open_devtools();
                        }
                    }
                }
                _ => {}
            });

            // DevTools is one keypress away (Cmd+Alt+I) — don't auto-open,
            // because that has previously coincided with WKWebView freezes
            // during initial page load on some macOS versions.

            // Register global hotkeys for every automation that already
            // has a marker assigned. Done after the menu setup so the
            // plugin is fully initialised. Failure is logged but doesn't
            // block launch — the user can still adopt + re-bind.
            if let Err(e) = refresh_automation_shortcuts(app.handle()) {
                tracing::warn!("initial automation shortcut registration failed: {e}");
            }
            if let Err(e) = colour_shortcuts::init(app.handle()) {
                tracing::warn!("colour shortcut registration failed: {e}");
            }
            if let Err(e)=setup_shortcuts::init(app.handle()) {
                tracing::warn!("setup shortcut registration failed: {e}");
            }
            if let Err(e) = app_lifecycle::init(app.handle()) {
                tracing::warn!("Menu bar initialization failed: {e}");
            }
            app_setups::init(app.handle());
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    let host = app_handle.state::<Arc<host_controls::HostControls>>();
                    {
                        let _guard = host.gate.lock().await;
                        if let Ok(dir) = host_dir(&app_handle) { let _ = host.reapply(&dir); }
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building AK820 Pro app")
        .run(|app, event| {
            match &event {
                tauri::RunEvent::Ready => app_lifecycle::startup(app),
                tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::CloseRequested { api, .. }, .. } if label == "main" => app_lifecycle::handle_close(app, api),
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => app_lifecycle::show(app),
                _ => {}
            }
            if matches!(event, tauri::RunEvent::Exit) {
                let _ = app.global_shortcut().unregister_all();
                if let Ok(dir) = host_dir(app) {
                    app.state::<Arc<host_controls::HostControls>>().shutdown(&dir);
                }
            }
        });
}

use crate::{
    ensure_open,
    feature_rgb::{self, NamedProfile, Profile},
    ConnState,
};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    pub enabled: bool,
    pub shortcut: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            shortcut: "Control+Alt+Super+KeyP".into(),
        }
    }
}
#[derive(Default)]
pub struct Runtime {
    settings: Mutex<Settings>,
    busy: AtomicBool,
    held: AtomicBool,
    applied: Mutex<Option<Applied>>,
    error: Mutex<Option<String>>,
}
#[derive(Clone, Serialize)]
pub struct Applied {
    pub id: Option<u64>,
    pub name: Option<String>,
    pub profile: Profile,
}
#[derive(Serialize)]
pub struct Status {
    pub settings: Settings,
    pub running: bool,
    pub applied: Option<Applied>,
    pub error: Option<String>,
}

fn parse(value: &str) -> Result<Shortcut, String> {
    let shortcut: Shortcut = value
        .parse()
        .map_err(|e| format!("Invalid shortcut: {e}"))?;
    if !shortcut
        .mods
        .intersects(Modifiers::CONTROL | Modifiers::ALT | Modifiers::SUPER)
    {
        return Err("Include Control, Option/Alt, or Command/Super in the shortcut".into());
    }
    Ok(shortcut)
}
fn persist(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let tmp = dir.join("feature-rgb-shortcut.json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(tmp, dir.join("feature-rgb-shortcut.json")).map_err(|e| e.to_string())
}
fn register(app: &AppHandle, shortcut: Shortcut) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _, event| {
            let runtime = app.state::<Arc<Runtime>>();
            if event.state() == ShortcutState::Released {
                runtime.held.store(false, Ordering::SeqCst);
                return;
            }
            if runtime.held.swap(true, Ordering::SeqCst) {
                return;
            }
            start_cycle(app);
        })
        .map_err(|e| format!("Could not register shortcut; choose another combination: {e}"))
}
fn configure(app: &AppHandle, settings: Settings, save: bool) -> Result<(), String> {
    let runtime = app.state::<Arc<Runtime>>();
    let mut current = runtime.settings.lock().map_err(|e| e.to_string())?;
    let old = if current.enabled {
        Some(parse(&current.shortcut)?)
    } else {
        None
    };
    let new = if settings.enabled {
        Some(parse(&settings.shortcut)?)
    } else {
        None
    };
    let changed = old.map(|s| s.id()) != new.map(|s| s.id());
    if changed {
        if let Some(new) = new {
            register(app, new)?;
        }
    }
    if save {
        if let Err(e) = persist(app, &settings) {
            if changed {
                if let Some(new) = new {
                    let _ = app.global_shortcut().unregister(new);
                }
            }
            return Err(e);
        }
    }
    if changed {
        if let Some(old) = old {
            if let Err(e) = app.global_shortcut().unregister(old) {
                if save {
                    let _ = persist(app, &current);
                }
                if let Some(new) = new {
                    let _ = app.global_shortcut().unregister(new);
                }
                return Err(format!("Could not replace previous shortcut: {e}"));
            }
        }
    }
    *current = settings;
    runtime.held.store(false, Ordering::SeqCst);
    *runtime.error.lock().map_err(|e| e.to_string())? = None;
    Ok(())
}
pub fn init(app: &AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let settings = match std::fs::read(dir.join("feature-rgb-shortcut.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings {
            enabled: true,
            ..Settings::default()
        },
        Err(e) => return Err(e.to_string()),
    };
    let result = configure(app, settings, false);
    if let Err(e) = &result {
        *app.state::<Arc<Runtime>>()
            .error
            .lock()
            .map_err(|e| e.to_string())? = Some(e.clone());
    }
    result
}
#[tauri::command]
pub async fn colour_shortcut_status(app: AppHandle) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || status(&app))
        .await
        .map_err(|e| e.to_string())?
}
fn status(app: &AppHandle) -> Result<Status, String> {
    let runtime = app.state::<Arc<Runtime>>();
    let status = Status {
        settings: runtime.settings.lock().map_err(|e| e.to_string())?.clone(),
        running: runtime.busy.load(Ordering::SeqCst),
        applied: runtime.applied.lock().map_err(|e| e.to_string())?.clone(),
        error: runtime.error.lock().map_err(|e| e.to_string())?.clone(),
    };
    Ok(status)
}
#[tauri::command]
pub async fn save_colour_shortcut(app: AppHandle, settings: Settings) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || configure(&app, settings, true))
        .await
        .map_err(|e| e.to_string())?
}

pub fn apply_locked(app: &AppHandle, profile: Profile, id: Option<u64>) -> Result<(), String> {
    profile.validate()?;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let state = app.state::<Arc<ConnState>>();
    let mut slot = state.0.blocking_lock();
    apply_inner(app, &dir, &mut slot, profile, id)
}
pub(crate) fn apply_inner(
    app: &AppHandle,
    dir: &std::path::Path,
    slot: &mut Option<ak820_protocol::Connection>,
    profile: Profile,
    id: Option<u64>,
) -> Result<(), String> {
    let named = if let Some(id) = id {
        Some(
            feature_rgb::list_profiles(dir)?
                .into_iter()
                .find(|p| p.id == id && same(&p.profile, &profile))
                .ok_or("Saved profile changed; select it again")?,
        )
    } else {
        None
    };
    let conn = ensure_open(slot).map_err(|e| e.to_string())?;
    conn.set_feature_custom_lighting(&profile.map, profile.brightness)
        .map_err(|e| e.to_string())?;
    // Keep cache/cursor unchanged on hardware failure. Never auto-retry a partial bank.
    feature_rgb::save(dir, &profile)
        .map_err(|e| format!("Colours applied, but saving this Mac's profile failed: {e}"))?;
    crate::setup_profiles::record(dir, |current| {
        current.lighting = Some(crate::setup_profiles::Lighting::PerKey {
            profile: profile.clone(),
        })
    })
    .map_err(|e| format!("Colours applied, but saving the setup's last lighting failed: {e}"))?;
    let applied = Applied {
        id: named.as_ref().map(|p| p.id),
        name: named.map(|p| p.name),
        profile,
    };
    let runtime = app.state::<Arc<Runtime>>();
    *runtime.applied.lock().map_err(|e| e.to_string())? = Some(applied.clone());
    *runtime.error.lock().map_err(|e| e.to_string())? = None;
    let _ = app.emit("feature-rgb-applied", applied);
    Ok(())
}
fn same(a: &Profile, b: &Profile) -> bool {
    a.brightness == b.brightness && a.map.leds == b.map.leds
}
fn next<'a>(
    profiles: &'a [NamedProfile],
    last: Option<&Profile>,
    cursor: Option<u64>,
) -> Result<&'a NamedProfile, String> {
    if profiles.is_empty() {
        return Err("Save or import a colour profile before cycling".into());
    }
    let index = last.and_then(|last| {
        profiles
            .iter()
            .position(|p| Some(p.id) == cursor && same(&p.profile, last))
            .or_else(|| profiles.iter().position(|p| same(&p.profile, last)))
    });
    Ok(&profiles[index.map(|i| (i + 1) % profiles.len()).unwrap_or(0)])
}
fn cycle(app: &AppHandle) -> Result<(), String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let state = app.state::<Arc<ConnState>>();
    let mut slot = state.0.blocking_lock();
    let profiles = feature_rgb::list_profiles(&dir)?;
    let last = feature_rgb::load(&dir)?;
    let runtime = app.state::<Arc<Runtime>>();
    let cursor = runtime
        .applied
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .and_then(|p| p.id);
    let chosen = next(&profiles, last.as_ref(), cursor)?;
    apply_inner(
        app,
        &dir,
        &mut slot,
        chosen.profile.clone(),
        Some(chosen.id),
    )
}
fn start_cycle(app: &AppHandle) {
    let runtime = app.state::<Arc<Runtime>>().inner().clone();
    if runtime.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = cycle(&app) {
            if let Ok(mut error) = runtime.error.lock() {
                *error = Some(e.clone());
            }
            let _ = app.emit("feature-rgb-error", e);
        }
        runtime.busy.store(false, Ordering::SeqCst);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn p(id: u64, brightness: u8) -> NamedProfile {
        NamedProfile {
            id,
            name: format!("Profile {id}"),
            profile: Profile {
                map: Default::default(),
                brightness,
            },
        }
    }
    #[test]
    fn cycles_from_manual_and_duplicate_profiles_and_wraps() {
        let list = vec![p(1, 2), p(2, 5), p(3, 5)];
        assert_eq!(next(&list, None, None).unwrap().id, 1);
        assert_eq!(next(&list, Some(&list[0].profile), None).unwrap().id, 2);
        assert_eq!(next(&list, Some(&list[1].profile), Some(2)).unwrap().id, 3);
        assert_eq!(next(&list, Some(&list[2].profile), Some(3)).unwrap().id, 1);
        assert_eq!(next(&list, Some(&p(9, 1).profile), None).unwrap().id, 1);
        assert!(next(&[], None, None).is_err());
    }
    #[test]
    fn validates_shortcut_modifiers() {
        assert!(parse("Control+Alt+Super+KeyP").is_ok());
        assert!(parse("KeyP").is_err());
        assert!(parse("Shift+KeyP").is_err());
        assert!(parse("not a shortcut").is_err());
    }
}

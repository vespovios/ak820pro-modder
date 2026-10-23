use crate::{setup_profiles, setup_runtime};
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
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
            shortcut: "Control+Alt+Super+KeyS".into(),
        }
    }
}
#[derive(Default)]
pub struct Runtime {
    settings: Mutex<Settings>,
    busy: AtomicBool,
    held: AtomicBool,
    cursor: Mutex<Option<u64>>,
    applied_revision: AtomicU64,
    error: Mutex<Option<String>>,
}
#[derive(Serialize)]
pub struct Status {
    pub settings: Settings,
    pub running: bool,
    pub cursor: Option<u64>,
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
    let tmp = dir.join("setup-shortcut.json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(tmp, dir.join("setup-shortcut.json")).map_err(|e| e.to_string())
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
    let settings = match std::fs::read(dir.join("setup-shortcut.json")) {
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
pub async fn setup_shortcut_status(app: AppHandle) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || status(&app))
        .await
        .map_err(|e| e.to_string())?
}
fn status(app: &AppHandle) -> Result<Status, String> {
    let runtime = app.state::<Arc<Runtime>>();
    let status = Status {
        settings: runtime.settings.lock().map_err(|e| e.to_string())?.clone(),
        running: runtime.busy.load(Ordering::SeqCst),
        cursor: *runtime.cursor.lock().map_err(|e| e.to_string())?,
        error: runtime.error.lock().map_err(|e| e.to_string())?.clone(),
    };
    Ok(status)
}
#[tauri::command]
pub async fn save_setup_shortcut(app: AppHandle, settings: Settings) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || configure(&app, settings, true))
        .await
        .map_err(|e| e.to_string())?
}

pub fn run(app: &AppHandle, id: Option<u64>) -> Result<setup_runtime::Report, String> {
    let runtime = app.state::<Arc<Runtime>>();
    if runtime.busy.swap(true, Ordering::SeqCst) {
        return Err("A complete setup is still applying; wait for it to finish".into());
    }
    run_claimed(app, id)
}
fn run_claimed(app: &AppHandle, id: Option<u64>) -> Result<setup_runtime::Report, String> {
    let runtime = app.state::<Arc<Runtime>>();
    struct Reset<'a>(&'a AtomicBool, AppHandle);
    impl Drop for Reset<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::SeqCst);
            crate::setup_menu::refresh(&self.1);
        }
    }
    let _reset = Reset(&runtime.busy, app.clone());
    let result: Result<setup_runtime::Report, String> = (|| {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let list = setup_profiles::list(&dir)?;
        let entry = if let Some(id) = id {
            list.into_iter()
                .find(|p| p.id == id)
                .ok_or("Setup no longer exists")?
        } else {
            let cursor = *runtime.cursor.lock().map_err(|e| e.to_string())?;
            let index = next_index(&list, cursor)?;
            list[index].clone()
        };
        crate::setup_menu::set_message(app, format!("Applying: {}", entry.name), None);
        let report = setup_runtime::apply(app, entry)?;
        crate::setup_menu::report(app, &report);
        if report.success {
            *runtime.cursor.lock().map_err(|e| e.to_string())? = Some(report.id);
            runtime
                .applied_revision
                .store(report.revision, Ordering::SeqCst);
        }
        *runtime.error.lock().map_err(|e| e.to_string())? = report.error.clone();
        Ok(report)
    })();
    if let Err(error) = &result {
        crate::setup_menu::set_message(app, "Setup switch failed".into(), Some(error.clone()));
        if let Ok(mut current) = runtime.error.lock() {
            *current = Some(error.clone());
        }
        let _ = app.emit("setup-switch-error", error);
    }
    result
}
fn next_index(list: &[setup_profiles::Named], cursor: Option<u64>) -> Result<usize, String> {
    if list.is_empty() {
        return Err("Save or import a setup before cycling".into());
    }
    Ok(list
        .iter()
        .position(|p| Some(p.id) == cursor)
        .map(|i| (i + 1) % list.len())
        .unwrap_or(0))
}
fn start_cycle(app: &AppHandle) {
    let runtime = app.state::<Arc<Runtime>>();
    if runtime.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = run_claimed(&app, None);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_setup_cycle_wraps_and_handles_deleted_selection() {
        let make = |id| setup_profiles::Named {
            id,
            name: format!("Setup {id}"),
            snapshot: setup_profiles::Snapshot {
                lighting: None,
                sleep_minutes: None,
                host: Default::default(),
                display: None,
            },
        };
        let profiles = vec![make(1), make(5), make(9)];
        assert_eq!(next_index(&profiles, None).unwrap(), 0);
        assert_eq!(next_index(&profiles, Some(1)).unwrap(), 1);
        assert_eq!(next_index(&profiles, Some(9)).unwrap(), 0);
        assert_eq!(next_index(&profiles, Some(3)).unwrap(), 0);
        assert!(next_index(&[], None).is_err());
    }
}

pub fn is_busy(app: &AppHandle) -> bool {
    app.state::<Arc<Runtime>>().busy.load(Ordering::SeqCst)
}
pub fn current_applied_id(app: &AppHandle) -> Option<u64> {
    let runtime = app.state::<Arc<Runtime>>();
    if runtime.applied_revision.load(Ordering::SeqCst) != setup_profiles::revision() {
        return None;
    }
    let id = runtime.cursor.lock().ok().and_then(|id| *id);
    id
}

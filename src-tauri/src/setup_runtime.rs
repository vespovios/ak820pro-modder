use crate::{
    automations, colour_shortcuts, commit_host_config, ensure_open, host_controls,
    setup_profiles::{self, Display, Lighting, Named, Snapshot},
    ConnState,
};
use serde::Serialize;
use std::{io::Read, sync::Arc};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone, Serialize)]
pub struct Report {
    pub revision: u64,
    pub id: u64,
    pub name: String,
    pub success: bool,
    pub completed: Vec<String>,
    pub failed: Option<String>,
    pub error: Option<String>,
    pub warnings: Vec<String>,
}
#[derive(Clone, Serialize)]
struct Progress {
    name: String,
    component: String,
}

fn run_steps(
    mut report: Report,
    parts: &[&str],
    mut run: impl FnMut(&str) -> Result<(), String>,
) -> Report {
    for part in parts {
        if let Err(error) = run(part) {
            report.failed = Some((*part).into());
            report.error = Some(error);
            return report;
        }
        report.completed.push((*part).into());
    }
    report.success = true;
    report
}
pub fn apply(app: &AppHandle, entry: Named) -> Result<Report, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let snapshot = entry.snapshot;
    snapshot.validate()?;
    let host = app.state::<Arc<host_controls::HostControls>>();
    let _host_guard = host.gate.blocking_lock();
    let reserved = automations::load(dir.clone())?
        .into_iter()
        .filter_map(|a| a.marker_hid.map(u16::from))
        .collect();
    let prepared = host_controls::prepare(snapshot.host.clone(), &reserved)?;
    let previous = host_controls::load(&dir)?;
    // Resolve every reference and validate all components before any hardware write.
    host.playback.stop_and_wait();
    let state = app.state::<Arc<ConnState>>();
    let mut slot = state.0.blocking_lock();
    let conn = ensure_open(&mut slot).map_err(|e| e.to_string())?;
    if !conn.is_legacy() {
        return Err("Complete setups support the ANSI feature-report AK820 revision".into());
    }
    // A failed/partial attempt must invalidate any previous complete-setup identity.
    setup_profiles::mark_changed();
    let mut parts = vec![];
    if snapshot.lighting.is_some() {
        parts.push("Lighting");
    }
    if snapshot.sleep_minutes.is_some() {
        parts.push("Sleep timer");
    }
    if snapshot.display.is_some() {
        parts.push("TFT display");
    }
    parts.push("Key remaps and macros");
    let report = Report {
        revision: 0,
        id: entry.id,
        name: entry.name.clone(),
        success: false,
        completed: vec![],
        failed: None,
        error: None,
        warnings: vec![],
    };
    let mut report = run_steps(report, &parts, |part| {
        let _ = app.emit(
            "setup-progress",
            Progress {
                name: entry.name.clone(),
                component: part.into(),
            },
        );
        match part {
            "Lighting" => match snapshot.lighting.as_ref().unwrap() {
                Lighting::PerKey { profile } => {
                    colour_shortcuts::apply_inner(app, &dir, &mut slot, profile.clone(), None)
                }
                Lighting::Global { config } => {
                    ensure_open(&mut slot)
                        .map_err(|e| e.to_string())?
                        .set_lighting(config)
                        .map_err(|e| e.to_string())?;
                    setup_profiles::record(&dir, |c| {
                        c.lighting = Some(Lighting::Global {
                            config: config.clone(),
                        })
                    })
                    .map_err(|e| {
                        format!("Lighting applied, but saving its last value failed: {e}")
                    })?;
                    let _ = app.emit("setup-global-lighting", config);
                    Ok(())
                }
            },
            "Sleep timer" => {
                let minutes = snapshot.sleep_minutes.unwrap();
                ensure_open(&mut slot)
                    .map_err(|e| e.to_string())?
                    .set_sleep_minutes(minutes)
                    .map_err(|e| e.to_string())?;
                setup_profiles::record(&dir, |c| c.sleep_minutes = Some(minutes)).map_err(|e| {
                    format!("Sleep timer applied, but saving its last value failed: {e}")
                })
            }
            "TFT display" => {
                let display = snapshot.display.as_ref().unwrap();
                ensure_open(&mut slot)
                    .map_err(|e| e.to_string())?
                    .upload_feature_tft(&display.animation, |completed, total| {
                        let _ = app.emit(
                            "setup-tft-progress",
                            crate::TftProgress {
                                upload_id: entry.id.to_string(),
                                completed,
                                total,
                            },
                        );
                    })
                    .map_err(|e| e.to_string())?;
                setup_profiles::record(&dir, |c| c.display = Some(display.clone()))
                    .map_err(|e| format!("Screen uploaded, but saving its last image failed: {e}"))
            }
            "Key remaps and macros" => {
                commit_host_config(app, &host, &previous, prepared.clone()).map(|_| ())
            }
            _ => Err("Unknown setup component".into()),
        }
    });
    if report.success && !prepared.macros.is_empty() && !host_controls::accessibility() {
        report
            .warnings
            .push("Macros are saved; enable Accessibility to replay them on this Mac.".into());
    }
    report.revision = setup_profiles::revision();
    let _ = app.emit("setup-applied", report.clone());
    Ok(report)
}

#[tauri::command]
pub async fn get_setup_snapshot(app: AppHandle) -> Result<Snapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let host = app.state::<Arc<host_controls::HostControls>>();
        let _host = host.gate.blocking_lock();
        let state = app.state::<Arc<ConnState>>();
        let _connection = state.0.blocking_lock();
        setup_profiles::capture(&app.path().app_data_dir().map_err(|e| e.to_string())?)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn list_setup_profiles(app: AppHandle) -> Result<Vec<Named>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        setup_profiles::list(&app.path().app_data_dir().map_err(|e| e.to_string())?)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn save_setup_profile(
    app: AppHandle,
    id: Option<u64>,
    name: String,
    snapshot: Snapshot,
) -> Result<Named, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let saved = setup_profiles::save(
            &app.path().app_data_dir().map_err(|e| e.to_string())?,
            id,
            name,
            snapshot,
        )?;
        crate::setup_menu::refresh(&app);
        Ok(saved)
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn delete_setup_profile(app: AppHandle, id: u64) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        setup_profiles::remove(&app.path().app_data_dir().map_err(|e| e.to_string())?, id)?;
        crate::setup_menu::refresh(&app);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn apply_setup_profile(app: AppHandle, id: u64) -> Result<Report, String> {
    tauri::async_runtime::spawn_blocking(move || crate::setup_shortcuts::run(&app, Some(id)))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn get_saved_sleep(app: AppHandle) -> Result<Option<u8>, String> {
    Ok(
        setup_profiles::current(&app.path().app_data_dir().map_err(|e| e.to_string())?)?
            .sleep_minutes,
    )
}
#[tauri::command]
pub async fn get_saved_tft(app: AppHandle) -> Result<Option<Display>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        Ok(
            setup_profiles::current(&app.path().app_data_dir().map_err(|e| e.to_string())?)?
                .display,
        )
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn export_setup_profiles(
    app: AppHandle,
    id: Option<u64>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let mut list = setup_profiles::list(&dir)?;
        if let Some(id) = id {
            list.retain(|p| p.id == id);
        }
        let bytes = setup_profiles::export(&list)?;
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Export complete keyboard setups")
            .add_filter("AK820 complete setups", &["json"])
            .set_file_name("ak820-setups.json")
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn import_setup_profiles(app: AppHandle) -> Result<Option<Vec<Named>>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_title("Import complete keyboard setups")
            .add_filter("AK820 complete setups", &["json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        let mut bytes = vec![];
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take((setup_profiles::MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let added = setup_profiles::import(
            &app.path().app_data_dir().map_err(|e| e.to_string())?,
            &bytes,
        )?;
        crate::setup_menu::refresh(&app);
        Ok(Some(added))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_apply_stops_and_reports_only_completed_components() {
        let report = Report {
            revision: 0,
            id: 1,
            name: "Test".into(),
            success: false,
            completed: vec![],
            failed: None,
            error: None,
            warnings: vec![],
        };
        let mut calls = vec![];
        let report = run_steps(
            report,
            &[
                "Lighting",
                "Sleep timer",
                "TFT display",
                "Key remaps and macros",
            ],
            |part| {
                calls.push(part.to_string());
                if part == "TFT display" {
                    Err("Missing block ACK".into())
                } else {
                    Ok(())
                }
            },
        );
        assert!(!report.success);
        assert_eq!(report.completed, vec!["Lighting", "Sleep timer"]);
        assert_eq!(report.failed.as_deref(), Some("TFT display"));
        assert_eq!(calls.len(), 3);
    }
}

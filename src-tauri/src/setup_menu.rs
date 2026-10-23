//! Saved complete setups in the native tray menu. Applying never opens a window.
use crate::{setup_profiles, setup_runtime::Report, setup_shortcuts};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu},
    AppHandle, Emitter, Manager,
};

const PREFIX: &str = "tray_setup_";
#[derive(Default)]
pub struct Runtime {
    generation: AtomicU64,
    message: Mutex<Option<(String, Option<String>)>>,
}
fn profile_id(event: &str) -> Option<u64> {
    let raw = event.strip_prefix(PREFIX)?;
    let id: u64 = raw.parse().ok()?;
    (raw == id.to_string()).then_some(id)
}
fn menu_text(value: &str) -> String {
    let mut chars = value.chars().map(|c| if c.is_control() { ' ' } else { c });
    let result: String = chars.by_ref().take(140).collect();
    let result = if chars.next().is_some() {
        format!("{result}…")
    } else {
        result
    };
    // Native menu labels use ampersands for mnemonics on some platforms.
    result.replace('&', "&&")
}
fn outcome(report: &Report) -> (String, Option<String>) {
    if report.success {
        (
            format!("Last applied: {}", report.name),
            (!report.warnings.is_empty()).then(|| report.warnings.join(" ")),
        )
    } else {
        (
            format!("Stopped at {}", report.failed.as_deref().unwrap_or("setup")),
            Some(format!(
                "{} Completed: {}.",
                report.error.as_deref().unwrap_or("Setup did not complete."),
                if report.completed.is_empty() {
                    "none".into()
                } else {
                    report.completed.join(", ")
                }
            )),
        )
    }
}
pub fn set_message(app: &AppHandle, summary: String, detail: Option<String>) {
    if let Ok(mut message) = app.state::<Arc<Runtime>>().message.lock() {
        *message = Some((summary, detail));
    }
    refresh(app);
}
pub fn report(app: &AppHandle, report: &Report) {
    let (summary, detail) = outcome(report);
    set_message(app, summary, detail);
}

fn build(
    app: &AppHandle,
    entries: &[(u64, String)],
    busy: bool,
    summary: &str,
    detail: Option<&str>,
) -> tauri::Result<Menu<tauri::Wry>> {
    let root = Menu::new(app)?;
    root.append(&MenuItem::with_id(
        app,
        "tray_show",
        "Show AK820 Pro Modder",
        true,
        None::<&str>,
    )?)?;
    let setups = Submenu::with_id(
        app,
        "tray_saved_setups",
        if busy {
            "Saved setups — applying…"
        } else {
            "Saved setups"
        },
        true,
    )?;
    setups.append(&MenuItem::with_id(
        app,
        "tray_setup_status",
        menu_text(summary),
        false,
        None::<&str>,
    )?)?;
    if let Some(detail) = detail {
        setups.append(&MenuItem::with_id(
            app,
            "tray_setup_detail",
            menu_text(detail),
            false,
            None::<&str>,
        )?)?;
    }
    setups.append(&PredefinedMenuItem::separator(app)?)?;
    if entries.is_empty() {
        setups.append(&MenuItem::with_id(
            app,
            "tray_setup_empty",
            "No saved setups yet",
            false,
            None::<&str>,
        )?)?;
    } else {
        for (id, name) in entries {
            setups.append(&MenuItem::with_id(
                app,
                format!("{PREFIX}{id}"),
                name.replace('&', "&&"),
                !busy,
                None::<&str>,
            )?)?;
        }
    }
    setups.append(&PredefinedMenuItem::separator(app)?)?;
    setups.append(&MenuItem::with_id(
        app,
        "tray_manage_setups",
        "Manage saved setups…",
        true,
        None::<&str>,
    )?)?;
    root.append(&setups)?;
    root.append(&PredefinedMenuItem::separator(app)?)?;
    root.append(&MenuItem::with_id(
        app,
        "tray_settings",
        "App settings…",
        true,
        None::<&str>,
    )?)?;
    root.append(&MenuItem::with_id(
        app,
        "tray_hide",
        "Hide window",
        true,
        None::<&str>,
    )?)?;
    root.append(&PredefinedMenuItem::separator(app)?)?;
    root.append(&MenuItem::with_id(
        app,
        "tray_quit",
        "Quit AK820 Pro Modder",
        true,
        None::<&str>,
    )?)?;
    Ok(root)
}
pub fn initial(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    build(app, &[], false, "Loading saved setups…", None)
}

// Read/validate libraries off the main thread. A later refresh invalidates older
// queued work so save/apply/delete races cannot reinstall a stale native menu.
pub fn refresh(app: &AppHandle) {
    let runtime = app.state::<Arc<Runtime>>().inner().clone();
    let generation = runtime.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let entries = app
            .path()
            .app_data_dir()
            .map_err(|e| e.to_string())
            .and_then(|dir| setup_profiles::list(&dir))
            .map(|profiles| {
                profiles
                    .into_iter()
                    .map(|p| (p.id, p.name))
                    .collect::<Vec<_>>()
            });
        if runtime.generation.load(Ordering::SeqCst) != generation {
            return;
        }
        let busy = setup_shortcuts::is_busy(&app);
        let message = runtime.message.lock().ok().and_then(|m| m.clone());
        let (entries, summary, detail) = match entries {
            Ok(entries) => {
                let (summary, detail) =
                    message.unwrap_or_else(|| ("Choose a saved setup".into(), None));
                (entries, summary, detail)
            }
            Err(e) => (vec![], "Could not load saved setups".into(), Some(e)),
        };
        let main = app.clone();
        let _ = app.run_on_main_thread(move || {
            if runtime.generation.load(Ordering::SeqCst) != generation {
                return;
            }
            let Some(tray) = main.tray_by_id("ak820-menu-bar") else {
                return;
            };
            let result = build(&main, &entries, busy, &summary, detail.as_deref())
                .and_then(|menu| tray.set_menu(Some(menu)));
            if let Err(e) = result {
                tracing::warn!("Could not refresh saved-setup menu: {e}");
            }
            let tooltip = detail.map_or(summary.clone(), |detail| format!("{summary}\n{detail}"));
            let _ = tray.set_tooltip(Some(format!("AK820 Pro Modder\n{tooltip}")));
        });
    });
}
pub fn handle_event(app: &AppHandle, event: &str) {
    if event == "tray_manage_setups" {
        crate::app_lifecycle::show(app);
        let _ = app.emit("open-setup-profiles", ());
    } else if let Some(id) = profile_id(event) {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            // Shared atomic gate rejects repeated clicks and all other apply
            // paths re-read the selected ID from the latest validated library.
            let _ = setup_shortcuts::run(&app, Some(id));
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_canonical_profile_ids_dispatch_not_status_or_management_items() {
        assert_eq!(profile_id("tray_setup_12"), Some(12));
        for value in [
            "tray_setup_status",
            "tray_setup_detail",
            "tray_setup_empty",
            "tray_manage_setups",
            "tray_quit",
            "tray_setup_01",
            "tray_setup_+2",
            "tray_setup_2_extra",
            "tray_setup_18446744073709551616",
        ] {
            assert_eq!(profile_id(value), None);
        }
    }
    #[test]
    fn partial_results_never_claim_a_complete_apply_and_keep_recovery_details() {
        let mut report = Report {
            revision: 0,
            id: 1,
            name: "Editing".into(),
            success: false,
            completed: vec!["Lighting".into()],
            failed: Some("TFT display".into()),
            error: Some("Missing block ACK".into()),
            warnings: vec![],
        };
        let (summary, detail) = outcome(&report);
        assert_eq!(summary, "Stopped at TFT display");
        let detail = detail.unwrap();
        assert!(detail.contains("Missing block ACK"));
        assert!(detail.contains("Completed: Lighting"));
        report.success = true;
        report
            .warnings
            .push("Enable Accessibility for macros".into());
        let (summary, detail) = outcome(&report);
        assert_eq!(summary, "Last applied: Editing");
        assert!(detail.unwrap().contains("Accessibility"));
    }
}

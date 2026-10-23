//! Window lifetime and per-user login launch preferences, independent of the keyboard.
use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub keep_running_on_close: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            keep_running_on_close: true,
        }
    }
}
#[derive(Default)]
pub struct Runtime {
    preferences: Mutex<Preferences>,
    tray_ready: AtomicBool,
    tray: Mutex<Option<tauri::tray::TrayIcon>>,
}
#[derive(Serialize)]
pub struct Status {
    pub keep_running_on_close: bool,
    pub start_at_login: bool,
    pub menu_bar_ready: bool,
}
#[tauri::command]
pub async fn app_lifecycle_status(app: AppHandle) -> Result<Status, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Arc<Runtime>>();
        let keep_running_on_close = state
            .preferences
            .lock()
            .map_err(|e| e.to_string())?
            .keep_running_on_close;
        Ok(Status {
            keep_running_on_close,
            start_at_login: app.autolaunch().is_enabled().map_err(|e| e.to_string())?,
            menu_bar_ready: state.tray_ready.load(Ordering::SeqCst),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn set_start_at_login(app: AppHandle, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "macos")]
        if enabled
            && !std::env::current_exe()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .contains(".app/Contents/MacOS/")
        {
            return Err(
                "Install and open the app in Applications before enabling start at login".into(),
            );
        }
        if enabled {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        }
        .map_err(|e| e.to_string())?;
        if app.autolaunch().is_enabled().map_err(|e| e.to_string())? != enabled {
            return Err("macOS did not save the requested login setting".into());
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn set_keep_running_on_close(app: AppHandle, enabled: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<Arc<Runtime>>();
        let mut prefs = state.preferences.lock().map_err(|e| e.to_string())?;
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let next = Preferences {
            keep_running_on_close: enabled,
        };
        let tmp = dir.join("app-preferences.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join("app-preferences.json")).map_err(|e| e.to_string())?;
        *prefs = next;
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub fn show(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Regular) {
        tracing::warn!("show app policy: {e}");
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
fn hide(app: &AppHandle) -> Result<(), String> {
    if !app
        .state::<Arc<Runtime>>()
        .tray_ready
        .load(Ordering::SeqCst)
    {
        return Err("The menu bar icon is unavailable; the window will remain open".into());
    }
    app.get_webview_window("main")
        .ok_or("The main window is unavailable")?
        .hide()
        .map_err(|e| e.to_string())?;
    // Keep Dock reopening available after an interactive launch, even if macOS
    // crowds a status item behind the display notch or other menu bar items.
    if let Some(tray) = app.tray_by_id("ak820-menu-bar") {
        tray.set_visible(true).map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub fn hide_to_menu_bar(app: AppHandle) -> Result<(), String> {
    hide(&app)
}
#[tauri::command]
pub fn quit_keyboard_app(app: AppHandle) {
    app.exit(0);
}

// A monochrome 2× keyboard glyph. Template rendering follows the menu bar theme.
fn tray_icon() -> tauri::image::Image<'static> {
    let mut rgba = vec![0u8; 44 * 44 * 4];
    for y in 0..44 {
        for x in 0..44 {
            let outline = (4..40).contains(&x)
                && (10..34).contains(&y)
                && (!(6..38).contains(&x) || !(12..32).contains(&y));
            let keys = [14, 20].iter().any(|r| (*r..*r + 3).contains(&y))
                && [9, 15, 21, 27, 33]
                    .iter()
                    .any(|c| (*c..*c + 3).contains(&x));
            let space = (13..31).contains(&x) && (26..29).contains(&y);
            if outline || keys || space {
                rgba[(y * 44 + x) * 4 + 3] = 255;
            }
        }
    }
    tauri::image::Image::new_owned(rgba, 44, 44)
}
pub fn init(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let state = app.state::<Arc<Runtime>>();
    let dir = app.path().app_data_dir()?;
    match std::fs::read(dir.join("app-preferences.json")) {
        Ok(bytes) => match serde_json::from_slice::<Preferences>(&bytes) {
            Ok(prefs) => *state.preferences.lock().unwrap() = prefs,
            Err(e) => tracing::warn!("Could not read app preferences; using defaults: {e}"),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("Could not read app preferences; using defaults: {e}"),
    }
    let menu = crate::setup_menu::initial(app)?;
    let tray = tauri::tray::TrayIconBuilder::with_id("ak820-menu-bar")
        .icon(tray_icon())
        .icon_as_template(true)
        .tooltip("AK820 Pro Modder")
        .title("AK820")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "tray_show" => show(app),
            "tray_settings" => {
                show(app);
                let _ = app.emit("open-app-settings", ());
            }
            "tray_hide" => {
                if let Err(e) = hide(app) {
                    tracing::warn!("hide window: {e}");
                }
            }
            "tray_quit" => app.exit(0),
            other => crate::setup_menu::handle_event(app, other),
        })
        .build(app)?;
    #[cfg(target_os = "macos")]
    configure_menu_bar_position(&tray)?;
    *state.tray.lock().unwrap() = Some(tray);
    state.tray_ready.store(true, Ordering::SeqCst);
    crate::setup_menu::refresh(app);
    Ok(())
}
pub fn startup(app: &AppHandle) {
    #[cfg(all(debug_assertions, target_os = "macos"))]
    diagnose_menu_bar(app);
    if std::env::args().any(|a| a == "--background") {
        #[cfg(target_os = "macos")]
        if let Err(e) = app.set_activation_policy(tauri::ActivationPolicy::Accessory) {
            tracing::warn!("background launch policy: {e}");
        }
        if let Err(e) = hide(app) {
            tracing::warn!("Background launch: {e}");
            show(app);
        }
    } else {
        show(app);
    }
}
#[derive(Debug, PartialEq, Eq)]
enum CloseAction {
    Hide,
    KeepOpen,
    Quit,
}
fn close_action(keep_running: bool, tray_ready: bool) -> CloseAction {
    if !keep_running {
        CloseAction::Quit
    } else if tray_ready {
        CloseAction::Hide
    } else {
        CloseAction::KeepOpen
    }
}
pub fn handle_close(app: &AppHandle, api: &tauri::CloseRequestApi) {
    let state = app.state::<Arc<Runtime>>();
    let keep_running = state
        .preferences
        .lock()
        .map(|p| p.keep_running_on_close)
        .unwrap_or(true);
    match close_action(keep_running, state.tray_ready.load(Ordering::SeqCst)) {
        CloseAction::Hide => {
            api.prevent_close();
            if let Err(e) = hide(app) {
                tracing::warn!("Could not keep window in menu bar: {e}");
            }
        }
        CloseAction::KeepOpen => {
            api.prevent_close();
            tracing::warn!("Keeping window visible because the menu bar is unavailable");
        }
        CloseAction::Quit => app.exit(0),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn close_only_hides_when_the_menu_bar_can_reopen_it() {
        assert_eq!(close_action(true, true), CloseAction::Hide);
        assert_eq!(close_action(true, false), CloseAction::KeepOpen);
        assert_eq!(close_action(false, true), CloseAction::Quit);
        assert_eq!(close_action(false, false), CloseAction::Quit);
    }
    #[test]
    fn preferences_default_to_background_and_round_trip_opt_out() {
        assert!(
            serde_json::from_str::<Preferences>("{}")
                .unwrap()
                .keep_running_on_close
        );
        let prefs = Preferences {
            keep_running_on_close: false,
        };
        let loaded: Preferences =
            serde_json::from_slice(&serde_json::to_vec(&prefs).unwrap()).unwrap();
        assert!(!loaded.keep_running_on_close);
        assert!(serde_json::from_str::<Preferences>(r#"{"start_at_login":true}"#).is_err());
    }
}

// Debug builds record only our own native status item geometry, never other UI content.
#[cfg(all(debug_assertions, target_os = "macos"))]
pub fn diagnose_menu_bar(app: &AppHandle) {
    let worker = app.clone();
    tauri::async_runtime::spawn(async move {
        for sample in 0..8 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let app = worker.clone();
            let _ = worker.run_on_main_thread(move || {
                let mtm = objc2::MainThreadMarker::new().unwrap();
                let screens: Vec<_> = objc2_app_kit::NSScreen::screens(mtm)
                    .iter()
                    .map(|screen| {
                        serde_json::json!({
                            "frame": format!("{:?}", screen.frame()),
                            "right_menu_area": format!("{:?}", screen.auxiliaryTopRightArea()),
                            "left_menu_area": format!("{:?}", screen.auxiliaryTopLeftArea())
                        })
                    })
                    .collect();
                let native = app.tray_by_id("ak820-menu-bar").and_then(|tray| {
                    tray.with_inner_tray_icon(|tray| {
                        tray.ns_status_item().map(|item| {
                            let button = item.button(objc2::MainThreadMarker::new().unwrap());
                            let window = button.as_ref().and_then(|button| button.window());
                            serde_json::json!({
                                "item_visible": item.isVisible(),
                                "button_hidden": button.as_ref().map(|b| b.isHidden()),
                                "button_frame": button.as_ref().map(|b| format!("{:?}",b.frame())),
                                "window_visible": window.as_ref().map(|w| w.isVisible()),
                                "window_frame": window.as_ref().map(|w| format!("{:?}",w.frame())),
                                "rect": format!("{:?}",tray.rect()),
                            })
                        })
                    })
                    .ok()
                    .flatten()
                });
                if let Ok(dir) = app.path().app_data_dir() {
                    let data =
                        serde_json::json!({"sample":sample,"native":native,"screens":screens});
                    let _ = std::fs::write(
                        dir.join("menu-bar-diagnostics.json"),
                        serde_json::to_vec_pretty(&data).unwrap(),
                    );
                }
            });
        }
    });
}

#[cfg(target_os = "macos")]
fn configure_menu_bar_position(tray: &tauri::tray::TrayIcon) -> tauri::Result<()> {
    tray.with_inner_tray_icon(|tray| {
        use objc2_foundation::{NSString, NSUserDefaults};
        let name = NSString::from_str("AK820ProModder");
        let key = NSString::from_str("NSStatusItem Preferred Position AK820ProModder");
        let defaults = NSUserDefaults::standardUserDefaults();
        // Seed only our first placement, at the right edge. Thereafter AppKit
        // restores the user's Command-dragged position using this stable name.
        // Without a seed a crowded notched display can hide a new item entirely.
        if defaults.objectForKey(&key).is_none() {
            defaults.setInteger_forKey(0, &key);
        }
        if let Some(item) = tray.ns_status_item() {
            item.setAutosaveName(Some(&name));
            item.setVisible(true);
        }
    })
}

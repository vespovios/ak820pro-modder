//! Local, foreground-app rules. Only app identity is observed; no window content.
use crate::{setup_profiles, setup_shortcuts};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::Instant,
};
use tauri::{AppHandle, Manager};

const OWN_ID: &str = "io.github.wsclx.ak820pro-modder";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct App {
    pub bundle_id: String,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub app: App,
    pub setup_id: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub fallback_id: Option<u64>,
    pub rules: Vec<Rule>,
}
#[derive(Clone, Default, Serialize)]
pub struct Status {
    pub settings: Settings,
    pub active_app: Option<App>,
    pub running: bool,
    pub last_setup: Option<String>,
    pub error: Option<String>,
}
#[derive(Default)]
pub struct Runtime {
    state: Mutex<Status>,
    generation: std::sync::atomic::AtomicU64,
}

fn validate(settings: &Settings, ids: &HashSet<u64>) -> Result<(), String> {
    if settings.rules.len() > 100 {
        return Err("Keep at most 100 app rules".into());
    }
    let mut seen = HashSet::new();
    for rule in &settings.rules {
        let id = &rule.app.bundle_id;
        if id.is_empty()
            || id.len() > 255
            || !id.contains('.')
            || id
                .chars()
                .any(|c| !(c.is_ascii_alphanumeric() || "._-".contains(c)))
            || id == OWN_ID
        {
            return Err(
                "Choose a valid application bundle identifier other than AK820 Pro Modder".into(),
            );
        }
        if rule.app.name.trim().is_empty() || rule.app.name.chars().count() > 128 {
            return Err("App names must contain 1–128 characters".into());
        }
        if !seen.insert(id) {
            return Err(format!("{} already has a rule", rule.app.name));
        }
        if settings.enabled && !ids.contains(&rule.setup_id) {
            return Err(format!("Choose an existing setup for {}", rule.app.name));
        }
    }
    if settings.enabled && settings.fallback_id.is_some_and(|id| !ids.contains(&id)) {
        return Err("Choose an existing fallback setup".into());
    }
    Ok(())
}
#[tauri::command]
pub fn app_setup_status(app: AppHandle) -> Result<Status, String> {
    Ok(app
        .state::<Arc<Runtime>>()
        .state
        .lock()
        .map_err(|e| e.to_string())?
        .clone())
}
#[tauri::command]
pub async fn save_app_setup_rules(app: AppHandle, settings: Settings) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        validate(
            &settings,
            &setup_profiles::list(&dir)?.iter().map(|p| p.id).collect(),
        )?;
        let runtime = app.state::<Arc<Runtime>>();
        let mut state = runtime.state.lock().map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let tmp = dir.join("app-setup-rules.json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join("app-setup-rules.json")).map_err(|e| e.to_string())?;
        state.settings = settings;
        state.error = None;
        runtime
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

// Dispatch AppKit reads to the main thread; never hold a rule/HID lock while waiting.
async fn observe(app: &AppHandle, all: bool) -> Result<(Option<App>, Vec<App>), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        #[cfg(target_os = "macos")]
        let result = objc2::rc::autoreleasepool(|_| {
            use objc2_app_kit::{NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};
            fn identity(app: &NSRunningApplication) -> Option<App> {
                let id = app.bundleIdentifier()?.to_string();
                Some(App {
                    name: app
                        .localizedName()
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| id.clone()),
                    bundle_id: id,
                })
            }
            let workspace = NSWorkspace::sharedWorkspace();
            let active = workspace.frontmostApplication().and_then(|a| identity(&a));
            let mut apps = vec![];
            if all {
                for a in workspace.runningApplications().iter() {
                    if a.activationPolicy() == NSApplicationActivationPolicy::Regular {
                        if let Some(a) = identity(&a) {
                            if a.bundle_id != OWN_ID {
                                apps.push(a);
                            }
                        }
                    }
                }
            }
            apps.sort_by_key(|a| a.name.to_lowercase());
            apps.dedup_by(|a, b| a.bundle_id == b.bundle_id);
            (active, apps)
        });
        #[cfg(not(target_os = "macos"))]
        let result = (None, vec![]);
        let _ = tx.send(result);
    })
    .map_err(|e| e.to_string())?;
    rx.await.map_err(|e| e.to_string())
}
#[tauri::command]
pub async fn list_setup_apps(app: AppHandle) -> Result<Vec<App>, String> {
    observe(&app, true).await.map(|(_, apps)| apps)
}

#[derive(Default)]
struct Policy {
    candidate: Option<String>,
    since: u64,
    attempted: bool,
}
impl Policy {
    fn target(
        &mut self,
        settings: &Settings,
        active: Option<&App>,
        now: u64,
        busy: bool,
    ) -> Option<u64> {
        let active = active.filter(|a| a.bundle_id != OWN_ID);
        if !settings.enabled || active.is_none() {
            *self = Self::default();
            return None;
        }
        let active = active.unwrap();
        if self.candidate.as_deref() != Some(&active.bundle_id) {
            self.candidate = Some(active.bundle_id.clone());
            self.since = now;
            self.attempted = false;
        }
        if busy || self.attempted || now.saturating_sub(self.since) < 800 {
            return None;
        }
        self.attempted = true;
        settings
            .rules
            .iter()
            .find(|r| r.app.bundle_id == active.bundle_id)
            .map(|r| r.setup_id)
            .or(settings.fallback_id)
    }
}
pub fn init(app: &AppHandle) {
    let runtime = app.state::<Arc<Runtime>>().inner().clone();
    let result = (|| -> Result<Settings, String> {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        match std::fs::read(dir.join("app-setup-rules.json")) {
            Ok(bytes) => {
                if bytes.len() > 65536 {
                    return Err("App rules file is too large".into());
                }
                let s: Settings = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                validate(
                    &s,
                    &setup_profiles::list(&dir)?.iter().map(|p| p.id).collect(),
                )?;
                Ok(s)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(e) => Err(e.to_string()),
        }
    })();
    {
        let mut state = runtime.state.lock().unwrap();
        match result {
            Ok(s) => state.settings = s,
            Err(e) => state.error = Some(e),
        }
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let start = Instant::now();
        let mut policy = Policy::default();
        let mut generation = 0;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            let (active, _) = match observe(&app, false).await {
                Ok(v) => v,
                Err(_) => break,
            };
            let current_generation = runtime.generation.load(std::sync::atomic::Ordering::SeqCst);
            if generation != current_generation {
                policy = Policy::default();
                generation = current_generation;
            }
            let settings = {
                let mut state = runtime.state.lock().unwrap();
                state.active_app = active.clone();
                state.settings.clone()
            };
            let Some(id) = policy.target(
                &settings,
                active.as_ref(),
                start.elapsed().as_millis() as u64,
                setup_shortcuts::is_busy(&app),
            ) else {
                continue;
            };
            if setup_shortcuts::current_applied_id(&app) == Some(id) {
                continue;
            }
            {
                let mut s = runtime.state.lock().unwrap();
                s.running = true;
                s.error = None;
            }
            let worker = app.clone();
            let result = tauri::async_runtime::spawn_blocking(move || {
                setup_shortcuts::run(&worker, Some(id))
            })
            .await;
            let mut state = runtime.state.lock().unwrap();
            state.running = false;
            match result {
                Ok(Ok(report)) if report.success => {
                    state.last_setup = Some(report.name);
                    state.error = None;
                }
                Ok(Ok(report)) => state.error = report.error,
                Ok(Err(e)) => state.error = Some(e),
                Err(e) => state.error = Some(e.to_string()),
            }
            // Refresh foreground identity before another decision; never queue old activations.
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    fn app(id: &str) -> App {
        App {
            bundle_id: id.into(),
            name: id.into(),
        }
    }
    fn settings() -> Settings {
        Settings {
            enabled: true,
            fallback_id: Some(2),
            rules: vec![Rule {
                app: app("com.example.Editor"),
                setup_id: 1,
            }],
        }
    }
    #[test]
    fn debounces_and_applies_once_per_activation_including_failures() {
        let mut p = Policy::default();
        let s = settings();
        let a = app("com.example.Editor");
        assert_eq!(p.target(&s, Some(&a), 0, false), None);
        assert_eq!(p.target(&s, Some(&a), 799, false), None);
        assert_eq!(p.target(&s, Some(&a), 800, false), Some(1));
        assert_eq!(p.target(&s, Some(&a), 5000, false), None);
        p.target(&s, Some(&app("com.example.Other")), 6000, false);
        assert_eq!(
            p.target(&s, Some(&app("com.example.Other")), 6800, false),
            Some(2)
        );
        p.target(&s, Some(&a), 7000, false);
        assert_eq!(p.target(&s, Some(&a), 7800, false), Some(1));
    }
    #[test]
    fn busy_waits_for_latest_app_and_own_app_is_ignored() {
        let mut p = Policy::default();
        let s = settings();
        let a = app("com.example.Editor");
        p.target(&s, Some(&a), 0, true);
        assert_eq!(p.target(&s, Some(&a), 900, true), None);
        p.target(&s, Some(&app("com.example.Other")), 1000, true);
        assert_eq!(
            p.target(&s, Some(&app("com.example.Other")), 1800, false),
            Some(2)
        );
        assert_eq!(p.target(&s, Some(&app(OWN_ID)), 3000, false), None);
        p.target(&s, Some(&a), 4000, false);
        assert_eq!(p.target(&s, Some(&a), 4800, false), Some(1));
    }
    #[test]
    fn disabled_and_unmatched_keep_current_and_validation_rejects_bad_rules() {
        let mut p = Policy::default();
        let mut s = settings();
        s.fallback_id = None;
        let a = app("com.example.Other");
        p.target(&s, Some(&a), 0, false);
        assert_eq!(p.target(&s, Some(&a), 900, false), None);
        s.enabled = false;
        assert_eq!(
            p.target(&s, Some(&app("com.example.Editor")), 2000, false),
            None
        );
        let ids = HashSet::from([1, 2]);
        assert!(validate(&s, &ids).is_ok());
        s.rules.push(s.rules[0].clone());
        assert!(validate(&s, &ids).is_err());
        s.rules.pop();
        s.rules[0].setup_id = 99;
        assert!(validate(&s, &ids).is_ok()); // Pausing remains possible after a setup is deleted.
        s.enabled = true;
        assert!(validate(&s, &ids).is_err());
    }
}

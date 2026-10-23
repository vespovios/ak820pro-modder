//! Mac-side controls for the AK820 revision whose onboard keymap format is unknown.
//! Profiles stay local. macOS maps only this vendor/product's keyboard services.
use ak820_protocol::commands::macros::{Macro, MacroActionKind};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(not(target_os = "macos"))]
mod macos {
    use super::RawMapping;
    pub fn accessibility() -> bool {
        false
    }
    pub fn input_monitoring() -> bool {
        false
    }
    pub fn request_input_monitoring() {}
    pub fn request_accessibility() {}
    pub fn apply(_: &[u16], _: &[RawMapping], _: &[RawMapping]) -> Result<usize, String> {
        Err("Mac-side controls require macOS".into())
    }
    pub fn post_key(_: u16, _: bool, _: u64) -> Result<(), String> {
        Err("Macro playback requires macOS".into())
    }
}

const KEY_PAGE: u64 = 0x700000000;
pub const MAX_ACTIONS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RawMapping {
    pub source: u64,
    pub target: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    Key { usage: u16 },
    Macro { macro_id: u8 },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Remap {
    pub source: u16,
    pub target: Target,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub remaps: Vec<Remap>,
    pub macros: Vec<Macro>,
    pub markers: BTreeMap<u8, u16>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            remaps: vec![],
            macros: vec![],
            markers: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    pub config: Config,
    pub services: usize,
    pub accessibility: bool,
    pub input_monitoring: bool,
    pub running: bool,
    pub error: Option<String>,
}
#[derive(Default)]
pub struct HostControls {
    pub gate: tokio::sync::Mutex<()>,
    pub playback: Arc<Playback>,
    native_gate: Mutex<()>,
    shutting_down: AtomicBool,
    services: AtomicUsize,
    error: Mutex<Option<String>>,
}

pub fn load(dir: &Path) -> Result<Config, String> {
    let path = dir.join("host-controls.json");
    if !path.exists() {
        return Ok(Config::default());
    }
    serde_json::from_slice(&std::fs::read(&path).map_err(|e| format!("Read local controls: {e}"))?)
        .map_err(|e| format!("Read local controls: {e}"))
}
fn persist(dir: &Path, cfg: &Config) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("host-controls.json");
    let temporary = dir.join("host-controls.json.tmp");
    std::fs::write(
        &temporary,
        serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temporary, path).map_err(|e| e.to_string())
}

/// Allocate stable F13-F20 markers without colliding with automations or
/// explicit remaps. F13-F24 cannot be replayed inside a macro (recursion).
pub fn prepare(mut cfg: Config, automation_markers: &BTreeSet<u16>) -> Result<Config, String> {
    validate_macros(&cfg.macros)?;
    if cfg.remaps.len() > 128 {
        return Err("At most 128 remaps are supported".into());
    }
    let mut sources = BTreeSet::new();
    let mut needed = BTreeSet::new();
    let mut reserved = automation_markers.clone();
    for mapping in &cfg.remaps {
        if !valid_usage(mapping.source) || !sources.insert(mapping.source) {
            return Err("Each source must be a supported key and appear only once".into());
        }
        if let Some(marker) = marker_alias(mapping.source) {
            reserved.insert(marker);
        }
        match mapping.target {
            Target::Key { usage } => {
                if !valid_usage(usage) || usage == mapping.source {
                    return Err("Choose a different supported destination key".into());
                }
                if let Some(marker) = marker_alias(usage) {
                    reserved.insert(marker);
                }
            }
            Target::Macro { macro_id } => {
                let m = cfg
                    .macros
                    .iter()
                    .find(|m| m.macro_id == macro_id)
                    .ok_or("Remove this macro's key binding before deleting it")?;
                if m.actions.is_empty() {
                    return Err("Record and save some actions before binding a macro".into());
                }
                needed.insert(macro_id);
            }
        }
    }
    cfg.markers.retain(|id, marker| {
        needed.contains(id) && (104..=111).contains(marker) && !reserved.contains(marker)
    });
    let mut used = reserved;
    // Discard duplicate stale assignments, including malformed hand-edited files.
    cfg.markers.retain(|_, marker| used.insert(*marker));
    for id in needed {
        if !cfg.markers.contains_key(&id) {
            let marker = (104..=111)
                .find(|marker| !used.contains(marker))
                .ok_or("All eight macro trigger markers are in use; remove a binding first")?;
            cfg.markers.insert(id, marker);
            used.insert(marker);
        }
    }
    Ok(cfg)
}
fn valid_usage(usage: u16) -> bool {
    virtual_key(usage).is_some() || (104..=115).contains(&usage)
}
fn marker_alias(usage: u16) -> Option<u16> {
    match usage {
        70..=72 => Some(usage + 34),
        104..=115 => Some(usage),
        _ => None,
    }
}

pub fn compiled(cfg: &Config) -> Result<Vec<RawMapping>, String> {
    if !cfg.enabled {
        return Ok(vec![]);
    }
    cfg.remaps
        .iter()
        .map(|r| {
            let target = match r.target {
                Target::Key { usage } => usage,
                Target::Macro { macro_id } => {
                    *cfg.markers.get(&macro_id).ok_or("Missing macro marker")?
                }
            };
            Ok(RawMapping {
                source: KEY_PAGE | u64::from(r.source),
                target: KEY_PAGE | u64::from(target),
            })
        })
        .collect()
}

/// Remove only our previous entries. Refuse to overwrite a conflicting
/// change from another mapping tool. Never erase unrelated mappings.
pub fn merge_mappings(
    current: &[RawMapping],
    previous: &[RawMapping],
    next: &[RawMapping],
) -> Result<Vec<RawMapping>, String> {
    let mut merged = Vec::new();
    for entry in current {
        if let Some(old) = previous.iter().find(|old| old.source == entry.source) {
            if old != entry {
                if next.iter().any(|new| new.source == entry.source) {
                    return Err("Another tool changed an AK820 mapping. Resolve that conflict before applying this profile.".into());
                }
                merged.push(*entry);
            }
        } else {
            merged.push(*entry);
        }
    }
    for entry in next {
        if merged
            .iter()
            .any(|external| external.source == entry.source && external != entry)
        {
            return Err("Another tool already remaps one of these source keys. Its mapping has been preserved.".into());
        }
        if !merged.contains(entry) {
            merged.push(*entry);
        }
    }
    merged.sort();
    Ok(merged)
}
fn connected_pids() -> Result<Vec<u16>, String> {
    let mut pids: Vec<_> = ak820_protocol::enumerate()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|d| {
            d.usage_page == ak820_protocol::legacy::USAGE_PAGE && [0x8009, 0x800a].contains(&d.pid)
        })
        .map(|d| d.pid)
        .collect();
    pids.sort();
    pids.dedup();
    Ok(pids)
}
impl HostControls {
    pub fn status(&self, dir: &Path) -> Result<Status, String> {
        let playback_error = self
            .playback
            .error
            .lock()
            .map_err(|e| e.to_string())?
            .clone();
        Ok(Status {
            config: load(dir)?,
            services: self.services.load(Ordering::SeqCst),
            accessibility: macos::accessibility(),
            input_monitoring: macos::input_monitoring(),
            running: self.playback.running.load(Ordering::SeqCst),
            error: playback_error.or_else(|| self.error.lock().ok().and_then(|e| e.clone())),
        })
    }
    pub fn commit(&self, dir: &Path, previous: &Config, next: &Config) -> Result<(), String> {
        let _native = self.native_gate.lock().map_err(|e| e.to_string())?;
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err("The app is closing".into());
        }
        let old = compiled(previous)?;
        let new = compiled(next)?;
        let pids = connected_pids()?;
        let services = macos::apply(&pids, &old, &new)?;
        if let Err(error) = persist(dir, next) {
            let rollback = macos::apply(&pids, &new, &old);
            return Err(format!(
                "Could not save profile: {error}; restore: {rollback:?}"
            ));
        }
        crate::setup_profiles::mark_changed();
        self.services.store(services, Ordering::SeqCst);
        *self.error.lock().map_err(|e| e.to_string())? = None;
        Ok(())
    }
    pub fn reapply(&self, dir: &Path) -> Result<(), String> {
        let _native = self.native_gate.lock().map_err(|e| e.to_string())?;
        if self.shutting_down.load(Ordering::SeqCst) {
            return Ok(());
        }
        let cfg = load(dir)?;
        let entries = compiled(&cfg)?;
        let result = macos::apply(&connected_pids()?, &entries, &entries);
        match result {
            Ok(count) => {
                self.services.store(count, Ordering::SeqCst);
                *self.error.lock().map_err(|e| e.to_string())? = None;
                Ok(())
            }
            Err(error) => {
                *self.error.lock().map_err(|e| e.to_string())? = Some(error.clone());
                Err(error)
            }
        }
    }
    pub fn shutdown(&self, dir: &Path) {
        self.shutting_down.store(true, Ordering::SeqCst);
        self.playback.stop_and_wait();
        let Ok(_native) = self.native_gate.lock() else {
            return;
        };
        if let Ok(cfg) = load(dir) {
            if let (Ok(pids), Ok(entries)) = (connected_pids(), compiled(&cfg)) {
                if let Err(error) = macos::apply(&pids, &entries, &[]) {
                    tracing::warn!(%error, "restore keyboard on quit");
                }
            }
        }
    }
}

pub fn validate_macros(macros: &[Macro]) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    for m in macros {
        if m.macro_id >= 100 || !ids.insert(m.macro_id) {
            return Err("Macro IDs must be unique and between 0 and 99".into());
        }
        if m.name.as_ref().is_some_and(|n| n.chars().count() > 120) {
            return Err("Macro names can contain at most 120 characters".into());
        }
        if m.actions.len() > MAX_ACTIONS {
            return Err("A macro can contain at most 512 actions".into());
        }
        let mut held = BTreeSet::new();
        let mut duration = 0u32;
        for (i, a) in m.actions.iter().enumerate() {
            if a.kind != MacroActionKind::Keyboard
                || virtual_key(u16::from(a.key_code)).is_none()
                || marker_alias(u16::from(a.key_code)).is_some()
            {
                return Err(format!("Macro M{} action {} must be a supported keyboard key; F13–F24 are reserved for triggers", m.macro_id + 1, i + 1));
            }
            duration += u32::from(a.delay_ms);
            if duration > 60_000 {
                return Err("A macro can run for at most 60 seconds".into());
            }
            if a.is_press {
                if !held.insert(a.key_code) {
                    return Err(format!(
                        "Macro M{} presses a key twice without releasing it",
                        m.macro_id + 1
                    ));
                }
            } else if !held.remove(&a.key_code) {
                return Err(format!(
                    "Macro M{} releases a key that wasn't pressed",
                    m.macro_id + 1
                ));
            }
        }
        if !held.is_empty() {
            return Err(format!("Macro M{} has unreleased keys; record their releases or remove the incomplete presses", m.macro_id + 1));
        }
    }
    Ok(())
}

#[derive(Default)]
pub struct Playback {
    running: AtomicBool,
    cancel: AtomicBool,
    error: Mutex<Option<String>>,
    thread: Mutex<Option<JoinHandle<()>>>,
}
impl Playback {
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
    pub fn last_error(&self) -> Option<String> {
        self.error.lock().ok().and_then(|slot| slot.clone())
    }
    pub fn record_error(&self, error: String) {
        if let Ok(mut slot) = self.error.lock() {
            *slot = Some(error);
        }
    }
    pub fn start(self: &Arc<Self>, m: Macro, delay_ms: u64) -> Result<(), String> {
        validate_macros(std::slice::from_ref(&m))?;
        if m.actions.is_empty() {
            return Err("This macro has no actions".into());
        }
        if !macos::accessibility() {
            return Err("Enable AK820 Pro Modder in System Settings → Privacy & Security → Accessibility, then check permission again".into());
        }
        let mut thread = self.thread.lock().map_err(|e| e.to_string())?;
        if self.running.load(Ordering::SeqCst) {
            return Err("A macro is already running; stop it before playing another".into());
        }
        if let Some(previous) = thread.take() {
            let _ = previous.join();
        }
        self.cancel.store(false, Ordering::SeqCst);
        *self.error.lock().map_err(|e| e.to_string())? = None;
        self.running.store(true, Ordering::SeqCst);
        let state = self.clone();
        *thread = Some(std::thread::spawn(move || {
            let result = state.play(&m, delay_ms);
            if let Err(error) = result {
                if let Ok(mut slot) = state.error.lock() {
                    *slot = Some(error);
                }
            }
            state.running.store(false, Ordering::SeqCst);
        }));
        Ok(())
    }
    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    pub fn stop_and_wait(&self) {
        self.stop();
        if let Ok(mut slot) = self.thread.lock() {
            if let Some(handle) = slot.take() {
                let _ = handle.join();
            }
        }
    }
    fn wait(&self, milliseconds: u64) -> bool {
        let until = Instant::now() + Duration::from_millis(milliseconds);
        while Instant::now() < until {
            if self.cancel.load(Ordering::SeqCst) {
                return false;
            }
            std::thread::sleep(
                until
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(10)),
            );
        }
        !self.cancel.load(Ordering::SeqCst)
    }
    fn play(&self, m: &Macro, delay_ms: u64) -> Result<(), String> {
        let mut keys = PressedKeys::default();
        if !self.wait(delay_ms) {
            return Ok(());
        }
        for a in &m.actions {
            if !self.wait(u64::from(a.delay_ms)) {
                break;
            }
            keys.send(u16::from(a.key_code), a.is_press)?;
        }
        Ok(())
    }
}
pub fn accessibility() -> bool {
    macos::accessibility()
}
pub fn request_accessibility() {
    macos::request_accessibility();
}
pub fn request_input_monitoring() {
    macos::request_input_monitoring();
}
#[derive(Default)]
struct PressedKeys {
    held: Vec<u16>,
}
fn modifier_flags(keys: &[u16]) -> u64 {
    keys.iter().fold(0, |flags, key| {
        flags
            | match key {
                224 | 228 => 0x40000,
                225 | 229 => 0x20000,
                226 | 230 => 0x80000,
                227 | 231 => 0x100000,
                _ => 0,
            }
    })
}
impl PressedKeys {
    fn send(&mut self, usage: u16, down: bool) -> Result<(), String> {
        if down {
            self.held.push(usage);
        } else {
            self.held.retain(|k| *k != usage);
        }
        macos::post_key(usage, down, modifier_flags(&self.held))
    }
}
impl Drop for PressedKeys {
    fn drop(&mut self) {
        while let Some(usage) = self.held.pop() {
            let _ = macos::post_key(usage, false, modifier_flags(&self.held));
        }
    }
}

/// USB usage -> physical macOS virtual key. Values from the installed
/// HIToolbox/Events.h, independent of the active character input layout.
pub fn virtual_key(usage: u16) -> Option<u16> {
    Some(match usage {
        4 => 0,
        5 => 11,
        6 => 8,
        7 => 2,
        8 => 14,
        9 => 3,
        10 => 5,
        11 => 4,
        12 => 34,
        13 => 38,
        14 => 40,
        15 => 37,
        16 => 46,
        17 => 45,
        18 => 31,
        19 => 35,
        20 => 12,
        21 => 15,
        22 => 1,
        23 => 17,
        24 => 32,
        25 => 9,
        26 => 13,
        27 => 7,
        28 => 16,
        29 => 6,
        30 => 18,
        31 => 19,
        32 => 20,
        33 => 21,
        34 => 23,
        35 => 22,
        36 => 26,
        37 => 28,
        38 => 25,
        39 => 29,
        40 => 36,
        41 => 53,
        42 => 51,
        43 => 48,
        44 => 49,
        45 => 27,
        46 => 24,
        47 => 33,
        48 => 30,
        49 | 50 => 42,
        51 => 41,
        52 => 39,
        53 => 50,
        54 => 43,
        55 => 47,
        56 => 44,
        57 => 57,
        58 => 122,
        59 => 120,
        60 => 99,
        61 => 118,
        62 => 96,
        63 => 97,
        64 => 98,
        65 => 100,
        66 => 101,
        67 => 109,
        68 => 103,
        69 => 111,
        70 => 105,
        71 => 107,
        72 => 113,
        73 => 114,
        74 => 115,
        75 => 116,
        76 => 117,
        77 => 119,
        78 => 121,
        79 => 124,
        80 => 123,
        81 => 125,
        82 => 126,
        83 => 71,
        84 => 75,
        85 => 67,
        86 => 78,
        87 => 69,
        88 => 76,
        89 => 83,
        90 => 84,
        91 => 85,
        92 => 86,
        93 => 87,
        94 => 88,
        95 => 89,
        96 => 91,
        97 => 92,
        98 => 82,
        99 => 65,
        100 => 10,
        103 => 81,
        104 => 105,
        105 => 107,
        106 => 113,
        107 => 106,
        108 => 64,
        109 => 79,
        110 => 80,
        111 => 90,
        224 => 59,
        225 => 56,
        226 => 58,
        227 => 55,
        228 => 62,
        229 => 60,
        230 => 61,
        231 => 54,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ak820_protocol::commands::macros::MacroAction;
    fn step(key: u8, down: bool) -> MacroAction {
        MacroAction {
            delay_ms: 0,
            key_code: key,
            is_press: down,
            kind: MacroActionKind::Keyboard,
        }
    }
    fn example_macro() -> Macro {
        Macro {
            macro_id: 0,
            name: Some("Example".into()),
            actions: vec![
                step(227, true),
                step(6, true),
                step(6, false),
                step(227, false),
            ],
        }
    }
    #[test]
    fn refuses_incomplete_recordings_and_recursive_markers() {
        let mut m = example_macro();
        validate_macros(&[m.clone()]).unwrap();
        m.actions.pop();
        assert!(validate_macros(&[m]).is_err());
        let mut m = example_macro();
        m.actions[1].key_code = 104;
        assert!(validate_macros(&[m]).is_err());
    }
    #[test]
    fn markers_do_not_collide_with_automations_or_explicit_function_keys() {
        let cfg = Config {
            macros: vec![example_macro()],
            remaps: vec![
                Remap {
                    source: 78,
                    target: Target::Macro { macro_id: 0 },
                },
                Remap {
                    source: 77,
                    target: Target::Key { usage: 105 },
                },
            ],
            ..Config::default()
        };
        let result = prepare(cfg, &BTreeSet::from([104])).unwrap();
        assert_eq!(result.markers[&0], 106);
    }
    #[test]
    fn preserves_external_maps_and_refuses_to_overwrite_changed_entries() {
        let ours = RawMapping {
            source: KEY_PAGE | 78,
            target: KEY_PAGE | 75,
        };
        let external = RawMapping {
            source: KEY_PAGE | 57,
            target: KEY_PAGE | 41,
        };
        assert_eq!(
            merge_mappings(&[ours, external], &[ours], &[]).unwrap(),
            [external]
        );
        assert!(merge_mappings(
            &[external],
            &[],
            &[RawMapping {
                source: external.source,
                target: ours.target
            }]
        )
        .is_err());
        let changed = RawMapping {
            source: ours.source,
            target: external.target,
        };
        assert_eq!(merge_mappings(&[changed], &[ours], &[]).unwrap(), [changed]);
        assert!(merge_mappings(&[changed], &[ours], &[ours]).is_err());
    }
    #[test]
    fn pause_compiles_to_no_mappings_and_macro_binding_requires_events() {
        let cfg = Config {
            enabled: false,
            ..Config::default()
        };
        assert!(compiled(&cfg).unwrap().is_empty());
        let cfg = Config {
            macros: vec![Macro::default()],
            remaps: vec![Remap {
                source: 78,
                target: Target::Macro { macro_id: 0 },
            }],
            ..Config::default()
        };
        assert!(prepare(cfg, &BTreeSet::new()).is_err());
    }
    #[test]
    fn modifiers_and_virtual_codes_match_apple_key_definitions() {
        assert_eq!(virtual_key(4), Some(0));
        assert_eq!(virtual_key(227), Some(55));
        assert_eq!(modifier_flags(&[227, 225, 4]), 0x120000);
        assert_eq!(modifier_flags(&[225, 229]), 0x20000);
    }
    #[test]
    fn rejects_oversized_duration_and_duplicate_sources() {
        let mut m = example_macro();
        m.actions[0].delay_ms = 60001;
        assert!(validate_macros(&[m]).is_err());
        let r = Remap {
            source: 78,
            target: Target::Key { usage: 75 },
        };
        assert!(prepare(
            Config {
                remaps: vec![r.clone(), r],
                ..Config::default()
            },
            &BTreeSet::new()
        )
        .is_err());
    }
}

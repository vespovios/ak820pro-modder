//! Complete Mac-saved keyboard setups; cached values are never device read-back.
use crate::{feature_rgb, host_controls};
use ak820_protocol::{
    commands::{lighting::LightingConfig, tft::TftAnimation},
    legacy,
};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Mutex};

static STORE: Mutex<()> = Mutex::new(());
static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
pub fn revision() -> u64 {
    REVISION.load(std::sync::atomic::Ordering::SeqCst)
}
pub fn mark_changed() {
    REVISION.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_PROFILES: usize = 20;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Lighting {
    Global { config: LightingConfig },
    PerKey { profile: feature_rgb::Profile },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Display {
    pub name: String,
    pub animation: TftAnimation,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Current {
    pub lighting: Option<Lighting>,
    pub sleep_minutes: Option<u8>,
    pub display: Option<Display>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub lighting: Option<Lighting>,
    pub sleep_minutes: Option<u8>,
    pub host: host_controls::Config,
    pub display: Option<Display>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(lighting) = &self.lighting {
            match lighting {
                Lighting::Global { config } => {
                    if config.brightness > 5 || config.speed > 5 || config.color_mode > 1 {
                        return Err(
                            "Lighting brightness/speed must be 0–5; rainbow must be 0 or 1".into(),
                        );
                    }
                    legacy::lighting_packet(config).map_err(|e| e.to_string())?;
                }
                Lighting::PerKey { profile } => profile.validate()?,
            }
        }
        if self
            .sleep_minutes
            .is_some_and(|v| ![0, 1, 5, 30].contains(&v))
        {
            return Err("Sleep timer must be never, 1, 5 or 30 minutes".into());
        }
        host_controls::prepare(self.host.clone(), &Default::default())?;
        if let Some(display) = &self.display {
            if display.name.trim().is_empty()
                || display.name.chars().count() > 128
                || display.name.chars().any(char::is_control)
            {
                return Err("Display names must contain 1–128 characters".into());
            }
            legacy::encode_tft(&display.animation).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Named {
    pub id: u64,
    pub name: String,
    pub snapshot: Snapshot,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableEntry {
    name: String,
    snapshot: Snapshot,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: String,
    version: u8,
    keyboard: String,
    profiles: Vec<PortableEntry>,
}

fn write<T: Serialize + ?Sized>(dir: &Path, file: &str, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("Setup data exceeds the 64 MB limit".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let temporary = dir.join(format!("{file}.tmp"));
    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(temporary, dir.join(file)).map_err(|e| e.to_string())
}
fn read_current(dir: &Path) -> Result<Current, String> {
    match std::fs::read(dir.join("setup-current.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Current::default()),
        Err(e) => Err(e.to_string()),
    }
}
pub fn current(dir: &Path) -> Result<Current, String> {
    let _guard = STORE.lock().map_err(|e| e.to_string())?;
    read_current(dir)
}
pub fn record(dir: &Path, change: impl FnOnce(&mut Current)) -> Result<(), String> {
    let _guard = STORE.lock().map_err(|e| e.to_string())?;
    let mut current = read_current(dir)?;
    change(&mut current);
    write(dir, "setup-current.json", &current)?;
    mark_changed();
    Ok(())
}
pub fn capture(dir: &Path) -> Result<Snapshot, String> {
    let mut current = current(dir)?;
    // Existing releases persisted this acknowledged upload before setup tracking.
    if current.lighting.is_none() {
        current.lighting = feature_rgb::load(dir)?.map(|profile| Lighting::PerKey { profile });
    }
    let snapshot = Snapshot {
        lighting: current.lighting,
        sleep_minutes: current.sleep_minutes,
        display: current.display,
        host: host_controls::load(dir)?,
    };
    snapshot.validate()?;
    Ok(snapshot)
}
fn read_library(dir: &Path) -> Result<Vec<Named>, String> {
    let bytes = match std::fs::read(dir.join("setup-profiles.json")) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
    };
    if bytes.len() > MAX_BYTES {
        return Err("Setup library exceeds the 64 MB limit".into());
    }
    let list: Vec<Named> = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if list.len() > MAX_PROFILES {
        return Err("You can save up to 20 complete setups".into());
    }
    for (i, p) in list.iter().enumerate() {
        feature_rgb::validate_name(&p.name)?;
        p.snapshot.validate()?;
        if list[..i]
            .iter()
            .any(|old| old.id == p.id || old.name.to_lowercase() == p.name.to_lowercase())
        {
            return Err("Setup library contains duplicate names or IDs".into());
        }
    }
    Ok(list)
}
pub fn list(dir: &Path) -> Result<Vec<Named>, String> {
    let _guard = STORE.lock().map_err(|e| e.to_string())?;
    read_library(dir)
}
pub fn save(
    dir: &Path,
    id: Option<u64>,
    name: String,
    snapshot: Snapshot,
) -> Result<Named, String> {
    snapshot.validate()?;
    let name = name.trim().to_string();
    feature_rgb::validate_name(&name)?;
    let _guard = STORE.lock().map_err(|e| e.to_string())?;
    let mut list = read_library(dir)?;
    if list
        .iter()
        .any(|p| Some(p.id) != id && p.name.to_lowercase() == name.to_lowercase())
    {
        return Err("A setup with that name already exists".into());
    }
    let saved = if let Some(id) = id {
        let p = list
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Setup no longer exists")?;
        p.name = name;
        p.snapshot = snapshot;
        p.clone()
    } else {
        if list.len() >= MAX_PROFILES {
            return Err("You can save up to 20 complete setups".into());
        }
        let id = list
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Setup ID limit reached")?;
        let p = Named { id, name, snapshot };
        list.push(p.clone());
        p
    };
    write(dir, "setup-profiles.json", &list)?;
    mark_changed();
    Ok(saved)
}
pub fn remove(dir: &Path, id: u64) -> Result<(), String> {
    let _guard = STORE.lock().map_err(|e| e.to_string())?;
    let mut list = read_library(dir)?;
    let index = list
        .iter()
        .position(|p| p.id == id)
        .ok_or("Setup no longer exists")?;
    list.remove(index);
    write(dir, "setup-profiles.json", &list)?;
    mark_changed();
    Ok(())
}
pub fn export(profiles: &[Named]) -> Result<Vec<u8>, String> {
    if profiles.is_empty() {
        return Err("Save a setup before exporting".into());
    }
    let entries = profiles
        .iter()
        .map(|p| PortableEntry {
            name: p.name.clone(),
            snapshot: p.snapshot.clone(),
        })
        .collect();
    serde_json::to_vec_pretty(&Bundle {
        format: "ak820-complete-setups".into(),
        version: 1,
        keyboard: "ak820-pro-ansi".into(),
        profiles: entries,
    })
    .map_err(|e| e.to_string())
}
pub fn import(dir: &Path, bytes: &[u8]) -> Result<Vec<Named>, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Setup files must be smaller than 64 MB".into());
    }
    let bundle: Bundle =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid setup file: {e}"))?;
    if bundle.format != "ak820-complete-setups"
        || bundle.version != 1
        || bundle.keyboard != "ak820-pro-ansi"
    {
        return Err("Choose an AK820 Pro ANSI complete-setup file (version 1)".into());
    }
    if bundle.profiles.is_empty() || bundle.profiles.len() > MAX_PROFILES {
        return Err("A file must contain 1–20 setups".into());
    }
    for p in &bundle.profiles {
        feature_rgb::validate_name(p.name.trim())?;
        p.snapshot.validate()?;
    }
    let _guard = STORE.lock().map_err(|e| e.to_string())?;
    let mut list = read_library(dir)?;
    if list.len() + bundle.profiles.len() > MAX_PROFILES {
        return Err("Import would exceed the 20-setup limit".into());
    }
    let mut id = list.iter().map(|p| p.id).max().unwrap_or(0);
    let mut added = vec![];
    for p in bundle.profiles {
        let base = p.name.trim();
        let mut name = base.to_string();
        let mut n = 1;
        while list
            .iter()
            .any(|p| p.name.to_lowercase() == name.to_lowercase())
        {
            let suffix = format!(" (imported {n})");
            name = format!(
                "{}{}",
                base.chars().take(64 - suffix.len()).collect::<String>(),
                suffix
            );
            n += 1;
        }
        id = id.checked_add(1).ok_or("Setup ID limit reached")?;
        let mut snapshot = p.snapshot;
        snapshot.host.markers.clear(); // allocated afresh against the recipient's automation bindings
        let entry = Named { id, name, snapshot };
        list.push(entry.clone());
        added.push(entry);
    }
    write(dir, "setup-profiles.json", &list)?;
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ak820_protocol::commands::tft::TftFrame;
    fn snapshot() -> Snapshot {
        Snapshot {
            lighting: Some(Lighting::PerKey {
                profile: feature_rgb::Profile {
                    map: Default::default(),
                    brightness: 4,
                },
            }),
            sleep_minutes: Some(5),
            host: Default::default(),
            display: Some(Display {
                name: "Test image".into(),
                animation: TftAnimation {
                    frames: vec![TftFrame {
                        pixels: vec![42; 32768],
                        delay_ms: 0,
                    }],
                },
            }),
        }
    }
    #[test]
    fn full_setup_roundtrip_retains_all_components_and_atomic_invalid_import() {
        let dir =
            std::env::temp_dir().join(format!("ak820-setups-roundtrip-{}", std::process::id()));
        let mut full = snapshot();
        full.host=serde_json::from_value(serde_json::json!({"enabled":true,"remaps":[{"source":78,"target":{"kind":"macro","macro_id":0}}],"macros":[{"macro_id":0,"name":"Type a","actions":[{"key_code":4,"is_press":true,"delay_ms":0,"kind":"keyboard"},{"key_code":4,"is_press":false,"delay_ms":8,"kind":"keyboard"}]}],"markers":{}})).unwrap();
        let animation = &mut full.display.as_mut().unwrap().animation;
        animation.frames[0].delay_ms = 200;
        animation.frames.push(TftFrame {
            pixels: vec![84; 32768],
            delay_ms: 300,
        });
        let original = save(&dir, None, "Work".into(), full).unwrap();
        let bytes = export(&[original.clone()]).unwrap();
        let imported = import(&dir, &bytes).unwrap();
        assert_eq!(imported[0].name, "Work (imported 1)");
        assert_eq!(
            serde_json::to_value(&original.snapshot).unwrap(),
            serde_json::to_value(&imported[0].snapshot).unwrap()
        );
        let before = std::fs::read(dir.join("setup-profiles.json")).unwrap();
        let mut invalid: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let mut bad = invalid["profiles"][0].clone();
        bad["snapshot"]["sleep_minutes"] = 7.into();
        invalid["profiles"].as_array_mut().unwrap().push(bad);
        assert!(import(&dir, &serde_json::to_vec(&invalid).unwrap()).is_err());
        assert_eq!(
            before,
            std::fs::read(dir.join("setup-profiles.json")).unwrap()
        );
        remove(&dir, original.id).unwrap();
        assert_eq!(list(&dir).unwrap().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn invalid_component_payloads_are_rejected_before_apply() {
        let mut s = snapshot();
        s.sleep_minutes = Some(2);
        assert!(s.validate().is_err());
        let mut s = snapshot();
        s.display.as_mut().unwrap().animation.frames[0].pixels.pop();
        assert!(s.validate().is_err());
        let mut s = snapshot();
        s.host.remaps.push(host_controls::Remap {
            source: 4,
            target: host_controls::Target::Macro { macro_id: 99 },
        });
        assert!(s.validate().is_err());
        let mut s = snapshot();
        if let Some(Lighting::PerKey { profile }) = &mut s.lighting {
            profile.brightness = 6;
        }
        assert!(s.validate().is_err());
    }
    #[test]
    fn current_settings_merge_preserves_previously_recorded_components() {
        let dir = std::env::temp_dir().join(format!("ak820-setups-current-{}", std::process::id()));
        record(&dir, |c| c.sleep_minutes = Some(30)).unwrap();
        let lighting = snapshot().lighting;
        record(&dir, |c| c.lighting = lighting).unwrap();
        let current = current(&dir).unwrap();
        assert_eq!(current.sleep_minutes, Some(30));
        assert!(current.lighting.is_some());
        assert!(current.display.is_none());
        assert!(capture(&dir).unwrap().display.is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

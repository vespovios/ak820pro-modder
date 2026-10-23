//! Last successfully applied custom colours; never presented as device read-back.
use ak820_protocol::{commands::per_key_rgb::CustomLedMap, legacy_rgb};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

pub(crate) static LIBRARY_LOCK: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub map: CustomLedMap,
    pub brightness: u8,
}
impl Profile {
    pub fn validate(&self) -> Result<(), String> {
        legacy_rgb::encode(&self.map).map_err(|e| e.to_string())?;
        if self.brightness > 5 {
            return Err("Custom RGB brightness must be 0–5".into());
        }
        Ok(())
    }
}
pub fn load(dir: &Path) -> Result<Option<Profile>, String> {
    let path = dir.join("feature-rgb.json");
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    let profile: Profile = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    profile.validate()?;
    Ok(Some(profile))
}
pub fn save(dir: &Path, profile: &Profile) -> Result<(), String> {
    profile.validate()?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("feature-rgb.json");
    let temp = dir.join("feature-rgb.json.tmp");
    let bytes = serde_json::to_vec_pretty(profile).map_err(|e| e.to_string())?;
    std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(temp, path).map_err(|e| e.to_string())
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NamedProfile {
    pub id: u64,
    pub name: String,
    pub profile: Profile,
}

pub(crate) fn read_library(dir: &Path) -> Result<Vec<NamedProfile>, String> {
    match std::fs::read(dir.join("feature-rgb-profiles.json")) {
        Ok(bytes) => {
            let profiles: Vec<NamedProfile> =
                serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            for (i, entry) in profiles.iter().enumerate() {
                entry.profile.validate()?;
                validate_name(&entry.name)?;
                if profiles[..i]
                    .iter()
                    .any(|p| p.id == entry.id || p.name.to_lowercase() == entry.name.to_lowercase())
                {
                    return Err("Saved colour profiles contain duplicate IDs or names".into());
                }
            }
            Ok(profiles)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Preserve the single-profile editor's last successful upload.
            let profiles = load(dir)?
                .map(|profile| {
                    vec![NamedProfile {
                        id: 1,
                        name: "Current colours".into(),
                        profile,
                    }]
                })
                .unwrap_or_default();
            write_library(dir, &profiles)?;
            Ok(profiles)
        }
        Err(e) => Err(e.to_string()),
    }
}

pub(crate) fn write_library(dir: &Path, profiles: &[NamedProfile]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(profiles).map_err(|e| e.to_string())?;
    let temp = dir.join("feature-rgb-profiles.json.tmp");
    std::fs::write(&temp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(temp, dir.join("feature-rgb-profiles.json")).map_err(|e| e.to_string())
}

pub(crate) fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err("Profile names must contain 1–64 characters without line breaks".into());
    }
    Ok(())
}

pub fn list_profiles(dir: &Path) -> Result<Vec<NamedProfile>, String> {
    let _guard = LIBRARY_LOCK.lock().map_err(|e| e.to_string())?;
    read_library(dir)
}

pub fn save_profile(
    dir: &Path,
    id: Option<u64>,
    name: String,
    profile: Profile,
) -> Result<NamedProfile, String> {
    let _guard = LIBRARY_LOCK.lock().map_err(|e| e.to_string())?;
    profile.validate()?;
    let name = name.trim().to_string();
    validate_name(&name)?;
    let mut profiles = read_library(dir)?;
    if profiles
        .iter()
        .any(|p| Some(p.id) != id && p.name.to_lowercase() == name.to_lowercase())
    {
        return Err("A colour profile with that name already exists".into());
    }
    let entry = if let Some(id) = id {
        let entry = profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("Colour profile no longer exists")?;
        entry.name = name;
        entry.profile = profile;
        entry.clone()
    } else {
        if profiles.len() >= 100 {
            return Err("You can save up to 100 colour profiles".into());
        }
        let id = profiles
            .iter()
            .map(|p| p.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("Colour profile ID limit reached")?;
        let entry = NamedProfile { id, name, profile };
        profiles.push(entry.clone());
        entry
    };
    write_library(dir, &profiles)?;
    Ok(entry)
}

pub fn delete_profile(dir: &Path, id: u64) -> Result<(), String> {
    let _guard = LIBRARY_LOCK.lock().map_err(|e| e.to_string())?;
    let mut profiles = read_library(dir)?;
    let index = profiles
        .iter()
        .position(|p| p.id == id)
        .ok_or("Colour profile no longer exists")?;
    profiles.remove(index);
    write_library(dir, &profiles)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct TestDir(std::path::PathBuf);
    impl TestDir {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!(
                "ak820-rgb-library-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            )))
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn profile(red: u8, brightness: u8) -> Profile {
        let mut map = CustomLedMap::default();
        map.set(34, red, 2, 3);
        Profile { map, brightness }
    }
    #[test]
    fn migration_and_named_snapshots_preserve_last_upload() {
        let dir = TestDir::new();
        save(&dir.0, &profile(42, 5)).unwrap();
        let migrated = list_profiles(&dir.0).unwrap();
        assert_eq!(migrated.len(), 1);
        assert_eq!(migrated[0].name, "Current colours");
        assert_eq!(migrated[0].profile.map.leds[34].red, 42);
        save_profile(&dir.0, None, "Work".into(), profile(80, 2)).unwrap();
        save(&dir.0, &profile(99, 4)).unwrap();
        let persisted = list_profiles(&dir.0).unwrap();
        assert_eq!(persisted.len(), 2);
        assert_eq!(persisted[0].profile.map.leds[34].red, 42);
        assert_eq!(persisted[1].profile.brightness, 2);
        assert_eq!(load(&dir.0).unwrap().unwrap().map.leds[34].red, 99);
    }
    #[test]
    fn updates_and_deletion_persist_without_affecting_other_profiles() {
        let dir = TestDir::new();
        let a = save_profile(&dir.0, None, "Work".into(), profile(10, 2)).unwrap();
        let b = save_profile(&dir.0, None, "Games".into(), profile(20, 5)).unwrap();
        save_profile(&dir.0, Some(a.id), "Office".into(), profile(30, 1)).unwrap();
        let saved = list_profiles(&dir.0).unwrap();
        assert_eq!(saved[0].id, a.id);
        assert_eq!(saved[0].name, "Office");
        assert_eq!(saved[0].profile.map.leds[34].red, 30);
        delete_profile(&dir.0, a.id).unwrap();
        let saved = list_profiles(&dir.0).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].id, b.id);
        delete_profile(&dir.0, b.id).unwrap();
        assert!(list_profiles(&dir.0).unwrap().is_empty());
    }
    #[test]
    fn invalid_or_duplicate_saves_leave_library_unchanged() {
        let dir = TestDir::new();
        let a = save_profile(&dir.0, None, "Work".into(), profile(10, 2)).unwrap();
        let before = std::fs::read(dir.0.join("feature-rgb-profiles.json")).unwrap();
        for (id, name, profile) in [
            (None, " work ", profile(20, 3)),
            (None, "  ", profile(20, 3)),
            (None, "Bad\nName", profile(20, 3)),
            (None, "Invalid", profile(20, 6)),
            (Some(a.id + 1), "Missing", profile(20, 3)),
        ] {
            assert!(save_profile(&dir.0, id, name.into(), profile).is_err());
            assert_eq!(
                std::fs::read(dir.0.join("feature-rgb-profiles.json")).unwrap(),
                before
            );
        }
    }
}

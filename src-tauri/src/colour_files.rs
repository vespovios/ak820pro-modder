//! Portable, declarative ANSI profiles. Imported files never contain actions.
use crate::feature_rgb::{self, NamedProfile, Profile};
use ak820_protocol::{
    commands::{lighting::parse_hex_rgb, per_key_rgb::CustomLedMap},
    legacy_rgb::ANSI_LED_SLOTS,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

const MAX_FILE: usize = 1024 * 1024;
const KEY_NAMES: &str = "Esc F1 F2 F3 F4 F5 F6 F7 F8 F9 F10 F11 F12 Delete Backquote 1 2 3 4 5 6 7 8 9 0 Minus Equal Backspace Home Tab Q W E R T Y U I O P BracketLeft BracketRight Backslash PageUp CapsLock A S D F G H J K L Semicolon Quote Enter PageDown ShiftLeft Z X C V B N M Comma Period Slash ShiftRight ArrowUp ControlLeft MetaLeft AltLeft Space AltRight Fn ControlRight ArrowLeft ArrowDown ArrowRight";
fn black() -> String {
    "000000".into()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: String,
    version: u8,
    keyboard: String,
    profiles: Vec<PortableProfile>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PortableProfile {
    name: String,
    brightness: u8,
    #[serde(default = "black")]
    base_color: String,
    keys: BTreeMap<String, String>,
}

fn rgb(value: &str) -> Result<(u8, u8, u8), String> {
    parse_hex_rgb(value.strip_prefix('#').unwrap_or(value))
        .ok_or_else(|| format!("Invalid colour “{value}”: use six hexadecimal digits"))
}
pub fn encode(profiles: &[NamedProfile]) -> Result<Vec<u8>, String> {
    let mut entries = Vec::new();
    for p in profiles {
        p.profile.validate()?;
        let keys = KEY_NAMES
            .split_whitespace()
            .zip(ANSI_LED_SLOTS)
            .map(|(name, &(slot, _))| {
                let c = &p.profile.map.leds[slot];
                (
                    name.into(),
                    format!("{:02X}{:02X}{:02X}", c.red, c.green, c.blue),
                )
            })
            .collect();
        entries.push(PortableProfile {
            name: p.name.clone(),
            brightness: p.profile.brightness,
            base_color: black(),
            keys,
        });
    }
    serde_json::to_vec_pretty(&Bundle {
        format: "ak820-colour-profiles".into(),
        version: 1,
        keyboard: "ak820-pro-ansi".into(),
        profiles: entries,
    })
    .map_err(|e| e.to_string())
}
pub fn decode(bytes: &[u8]) -> Result<Vec<(String, Profile)>, String> {
    if bytes.len() > MAX_FILE {
        return Err("Colour profile files must be smaller than 1 MB".into());
    }
    let bundle: Bundle =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid colour profile file: {e}"))?;
    if bundle.format != "ak820-colour-profiles"
        || bundle.version != 1
        || bundle.keyboard != "ak820-pro-ansi"
    {
        return Err("Choose an AK820 Pro ANSI colour-profile file (format version 1)".into());
    }
    if bundle.profiles.is_empty() || bundle.profiles.len() > 100 {
        return Err("A file must contain 1–100 profiles".into());
    }
    let slots: BTreeMap<_, _> = KEY_NAMES
        .split_whitespace()
        .zip(ANSI_LED_SLOTS)
        .map(|(name, &(slot, _))| (name, slot))
        .collect();
    bundle
        .profiles
        .into_iter()
        .map(|p| {
            let name = p.name.trim().to_string();
            feature_rgb::validate_name(&name)?;
            let base = rgb(&p.base_color)?;
            let mut map = CustomLedMap::default();
            for &(slot, _) in ANSI_LED_SLOTS {
                map.set(slot, base.0, base.1, base.2);
            }
            for (key, colour) in p.keys {
                let slot = *slots
                    .get(key.as_str())
                    .ok_or_else(|| format!("Unknown ANSI key “{key}”"))?;
                let c = rgb(&colour)?;
                map.set(slot, c.0, c.1, c.2);
            }
            let profile = Profile {
                map,
                brightness: p.brightness,
            };
            profile.validate()?;
            Ok((name, profile))
        })
        .collect()
}
pub fn import_bytes(dir: &Path, bytes: &[u8]) -> Result<Vec<NamedProfile>, String> {
    let incoming = decode(bytes)?;
    let _guard = feature_rgb::LIBRARY_LOCK
        .lock()
        .map_err(|e| e.to_string())?;
    let mut list = feature_rgb::read_library(dir)?;
    if list.len() + incoming.len() > 100 {
        return Err("Import would exceed the 100-profile limit".into());
    }
    let mut id = list.iter().map(|p| p.id).max().unwrap_or(0);
    let mut added = Vec::new();
    for (name, profile) in incoming {
        let mut candidate = name.clone();
        let mut n = 1;
        while list
            .iter()
            .any(|p| p.name.to_lowercase() == candidate.to_lowercase())
        {
            let suffix = format!(" (imported {n})");
            candidate = format!(
                "{}{}",
                name.chars().take(64 - suffix.len()).collect::<String>(),
                suffix
            );
            n += 1;
        }
        id = id.checked_add(1).ok_or("Colour profile ID limit reached")?;
        let entry = NamedProfile {
            id,
            name: candidate,
            profile,
        };
        list.push(entry.clone());
        added.push(entry);
    }
    feature_rgb::write_library(dir, &list)?;
    Ok(added)
}

#[tauri::command]
pub async fn export_colour_profiles(
    app: AppHandle,
    id: Option<u64>,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let mut profiles = feature_rgb::list_profiles(&dir)?;
        if let Some(id) = id {
            profiles.retain(|p| p.id == id);
        }
        if profiles.is_empty() {
            return Err("Choose a saved profile to export".into());
        }
        let bytes = encode(&profiles)?;
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Export colour profiles")
            .add_filter("AK820 colour profiles", &["json"])
            .set_file_name("ak820-colours.json")
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
pub async fn import_colour_profiles(app: AppHandle) -> Result<Option<Vec<NamedProfile>>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("Import colour profiles")
            .add_filter("AK820 colour profiles", &["json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take((MAX_FILE + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        import_bytes(&dir, &bytes).map(Some)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prepared() -> Vec<u8> {
        br##"{"format":"ak820-colour-profiles","version":1,"keyboard":"ak820-pro-ansi","profiles":[{"name":"Gaming","brightness":3,"base_color":"7C5CFF","keys":{"W":"#FF0000","E":"00FFFF"}}]}"##.to_vec()
    }
    #[test]
    fn portable_roundtrip_and_prepared_base_colours() {
        assert_eq!(KEY_NAMES.split_whitespace().count(), ANSI_LED_SLOTS.len());
        let entries = decode(&prepared()).unwrap();
        let p = &entries[0].1;
        assert_eq!(p.map.leds[34].red, 255);
        assert_eq!(p.map.leds[35].blue, 255);
        assert_eq!(p.map.leds[49].red, 124);
        assert_eq!(p.brightness, 3);
        let data = encode(&[NamedProfile {
            id: 9,
            name: entries[0].0.clone(),
            profile: p.clone(),
        }])
        .unwrap();
        let copy = decode(&data).unwrap();
        assert_eq!(copy[0].1.map.leds, p.map.leds);
        assert_eq!(copy[0].1.brightness, 3);
    }
    #[test]
    fn invalid_files_are_rejected() {
        for (old, new) in [
            ("ansi", "iso"),
            ("\"version\":1", "\"version\":2"),
            ("\"W\"", "\"UnknownKey\""),
            ("FF0000", "FF00XX"),
            ("\"brightness\":3", "\"brightness\":6"),
        ] {
            let data = String::from_utf8(prepared()).unwrap().replace(old, new);
            assert!(decode(data.as_bytes()).is_err(), "{new}");
        }
        assert!(decode(&vec![b' '; MAX_FILE + 1]).is_err());
    }
    #[test]
    fn import_renames_duplicates_and_invalid_batch_changes_nothing() {
        let dir = std::env::temp_dir().join(format!("ak820-import-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = import_bytes(&dir, &prepared()).unwrap();
        let b = import_bytes(&dir, &prepared()).unwrap();
        assert_ne!(a[0].id, b[0].id);
        assert_eq!(b[0].name, "Gaming (imported 1)");
        let before = std::fs::read(dir.join("feature-rgb-profiles.json")).unwrap();
        let mut batch: serde_json::Value = serde_json::from_slice(&prepared()).unwrap();
        let mut bad = batch["profiles"][0].clone();
        bad["brightness"] = 9.into();
        batch["profiles"].as_array_mut().unwrap().push(bad);
        assert!(import_bytes(&dir, &serde_json::to_vec(&batch).unwrap()).is_err());
        assert_eq!(
            before,
            std::fs::read(dir.join("feature-rgb-profiles.json")).unwrap()
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}

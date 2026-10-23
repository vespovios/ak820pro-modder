//! Native device-scoped remapping, using Apple's IOHIDEventSystemClient API.
use super::RawMapping;
use core_foundation::{
    array::CFArray,
    base::{CFType, CFTypeRef, TCFType},
    dictionary::CFDictionary,
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    event::{CGEvent, CGEventFlags, CGEventTapLocation},
    event_source::{CGEventSource, CGEventSourceStateID},
};
use std::ffi::c_void;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDEventSystemClientCreateSimpleClient(allocator: *const c_void) -> CFTypeRef;
    fn IOHIDEventSystemClientCopyServices(client: CFTypeRef) -> core_foundation::array::CFArrayRef;
    fn IOHIDServiceClientCopyProperty(service: CFTypeRef, key: CFStringRef) -> CFTypeRef;
    fn IOHIDServiceClientSetProperty(service: CFTypeRef, key: CFStringRef, value: CFTypeRef) -> u8;
    fn IOHIDServiceClientConformsTo(service: CFTypeRef, page: u32, usage: u32) -> i32;
    fn IOHIDCheckAccess(request_type: i32) -> i32;
    fn IOHIDRequestAccess(request_type: i32) -> bool;
}
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef) -> u8;
}

pub fn accessibility() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}
pub fn input_monitoring() -> bool {
    unsafe { IOHIDCheckAccess(1) == 0 }
}
pub fn request_input_monitoring() {
    unsafe {
        IOHIDRequestAccess(1);
    }
}
pub fn request_accessibility() {
    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::new("AXTrustedCheckOptionPrompt"),
        core_foundation::boolean::CFBoolean::true_value(),
    )]);
    unsafe {
        AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef());
    }
}

fn property(service: CFTypeRef, key: &str) -> Option<CFType> {
    let value = unsafe {
        IOHIDServiceClientCopyProperty(service, CFString::new(key).as_concrete_TypeRef())
    };
    if value.is_null() {
        None
    } else {
        Some(unsafe { CFType::wrap_under_create_rule(value) })
    }
}
fn number(service: CFTypeRef, key: &str) -> Option<i64> {
    property(service, key)?.downcast::<CFNumber>()?.to_i64()
}

// A service retains a connection to its event-system client, but does not
// retain the client itself. Keep the parent alive throughout property I/O.
struct Services {
    entries: Vec<CFType>,
    _client: CFType,
}
impl std::ops::Deref for Services {
    type Target = Vec<CFType>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}
fn services(pids: &[u16]) -> Result<Services, String> {
    let client = unsafe { IOHIDEventSystemClientCreateSimpleClient(std::ptr::null()) };
    if client.is_null() {
        return Err("Cannot open the macOS HID event system".into());
    }
    let client = unsafe { CFType::wrap_under_create_rule(client) };
    let array = unsafe { IOHIDEventSystemClientCopyServices(client.as_CFTypeRef()) };
    if array.is_null() {
        return Err("Cannot enumerate keyboard services".into());
    }
    let array: CFArray<CFType> = unsafe { CFArray::wrap_under_create_rule(array) };
    let entries = array
        .iter()
        .filter(|s| {
            let ptr = s.as_CFTypeRef();
            number(ptr, "VendorID") == Some(0x0c45)
                && number(ptr, "ProductID").is_some_and(|pid| pids.contains(&(pid as u16)))
                && unsafe { IOHIDServiceClientConformsTo(ptr, 1, 6) != 0 }
        })
        .map(|s| (*s).clone())
        .collect();
    Ok(Services {
        entries,
        _client: client,
    })
}

fn dict_number(dict: &CFDictionary, key: &str) -> Result<u64, String> {
    let key = CFString::new(key);
    let raw = *dict
        .find(key.as_CFTypeRef())
        .ok_or("Invalid macOS key mapping: missing usage")?;
    let value = unsafe { CFType::wrap_under_get_rule(raw) };
    let value = value
        .downcast::<CFNumber>()
        .and_then(|n| n.to_i64())
        .ok_or("Invalid macOS key mapping: non-numeric usage")?;
    u64::try_from(value).map_err(|_| "Invalid macOS key mapping: negative usage".into())
}

fn read(service: CFTypeRef) -> Result<Vec<RawMapping>, String> {
    let Some(value) = property(service, "UserKeyMapping") else {
        return Ok(vec![]);
    };
    let array = value
        .downcast::<CFArray>()
        .ok_or("Invalid macOS UserKeyMapping property")?;
    let mut result = Vec::new();
    for raw in array.iter() {
        let value = unsafe { CFType::wrap_under_get_rule(*raw) };
        let dict = value
            .downcast::<CFDictionary>()
            .ok_or("Invalid macOS key mapping entry")?;
        result.push(RawMapping {
            source: dict_number(&dict, "HIDKeyboardModifierMappingSrc")?,
            target: dict_number(&dict, "HIDKeyboardModifierMappingDst")?,
        });
    }
    result.sort();
    Ok(result)
}

fn write(service: CFTypeRef, mappings: &[RawMapping]) -> Result<(), String> {
    let dicts: Vec<_> = mappings
        .iter()
        .map(|m| {
            CFDictionary::from_CFType_pairs(&[
                (
                    CFString::new("HIDKeyboardModifierMappingSrc"),
                    CFNumber::from(m.source as i64),
                ),
                (
                    CFString::new("HIDKeyboardModifierMappingDst"),
                    CFNumber::from(m.target as i64),
                ),
            ])
        })
        .collect();
    let array = CFArray::from_CFTypes(&dicts);
    let key = CFString::new("UserKeyMapping");
    if unsafe {
        IOHIDServiceClientSetProperty(service, key.as_concrete_TypeRef(), array.as_CFTypeRef())
    } == 0
    {
        return Err("macOS rejected the key mapping. Check Input Monitoring permission for the app if macOS requests it.".into());
    }
    let mut expected = mappings.to_vec();
    expected.sort();
    if read(service)? != expected {
        return Err(
            "macOS key mapping read-back did not match; the previous mapping was restored".into(),
        );
    }
    Ok(())
}

/// Compute every service's new map before changing any. Preserve unrelated
/// mappings, verify the write, and roll back all services on failure.
pub fn apply(pids: &[u16], previous: &[RawMapping], next: &[RawMapping]) -> Result<usize, String> {
    let services = services(pids)?;
    let plans = services
        .iter()
        .map(|s| {
            let old = read(s.as_CFTypeRef())?;
            let new = super::merge_mappings(&old, previous, next)?;
            Ok((s, old, new))
        })
        .collect::<Result<Vec<_>, String>>()?;
    for (index, (s, old, new)) in plans.iter().enumerate() {
        if old == new {
            continue;
        }
        if let Err(error) = write(s.as_CFTypeRef(), new) {
            let mut rollback_errors = Vec::new();
            for (service, original, _) in &plans[..=index] {
                if let Err(e) = write(service.as_CFTypeRef(), original) {
                    rollback_errors.push(e);
                }
            }
            return Err(format!(
                "{error}{}",
                if rollback_errors.is_empty() {
                    String::new()
                } else {
                    format!("; restore failed: {}", rollback_errors.join("; "))
                }
            ));
        }
    }
    Ok(services.len())
}

pub fn post_key(usage: u16, down: bool, flags: u64) -> Result<(), String> {
    let code = super::virtual_key(usage)
        .ok_or_else(|| format!("Macro key usage {usage} is unsupported on macOS"))?;
    let source = CGEventSource::new(CGEventSourceStateID::Private)
        .map_err(|_| "Cannot create a keyboard event source")?;
    let event = CGEvent::new_keyboard_event(source, code, down)
        .map_err(|_| "Cannot create a keyboard event")?;
    event.set_flags(CGEventFlags::from_bits_retain(flags));
    event.post(CGEventTapLocation::HID);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "temporarily changes an AK820 mapping, then restores it; run explicitly with keyboard connected"]
    fn native_mapping_write_readback_and_restore() {
        let pids = [0x800a];
        let targets = services(&pids).unwrap();
        assert!(!targets.is_empty());
        let originals: Vec<_> = targets
            .iter()
            .map(|s| read(s.as_CFTypeRef()).unwrap())
            .collect();
        // A normally unused F20 mapping, so ordinary typing is unaffected.
        let temporary = RawMapping {
            source: 0x70000006f,
            target: 0x70000006e,
        };
        apply(&pids, &[], &[temporary]).unwrap();
        let observed: Vec<_> = targets.iter().map(|s| read(s.as_CFTypeRef())).collect();
        let restored = apply(&pids, &[temporary], &[]);
        assert!(restored.is_ok());
        for (service, original) in targets.iter().zip(originals) {
            assert_eq!(read(service.as_CFTypeRef()).unwrap(), original);
        }
        assert!(observed
            .into_iter()
            .all(|r| r.unwrap().contains(&temporary)));
    }
}

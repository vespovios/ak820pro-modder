//! Wired AK820 feature-report protocol (0xFF13).
//!
//! Report ID 0 is an extra hidapi byte; 0x04 is DATA inside the 64-byte
//! report, not its report ID. Each write needs a 35 ms settle and feature
//! read. References: gusleig/ajazz-ak820-pro-mac (ak820/control.py),
//! kabelklaus/ajazz-ak820-pro (app.js), both MIT licensed.
use crate::{
    commands::lighting::{LightingConfig, Mode},
    error::*,
};
use hidapi::HidDevice;
use serde::{Deserialize, Serialize};

pub const USAGE_PAGE: u16 = 0xff13;
const START: u8 = 0x18;
const SAVE: u8 = 0x02;
const FINISH: u8 = 0xf0;

fn control(command: u8, sub: u8) -> [u8; 64] {
    let mut p = [0; 64];
    p[0] = 4;
    p[1] = command;
    p[2] = sub;
    p[8] = if command == SAVE { 0 } else { 1 };
    p
}

pub(crate) fn exchange(device: &HidDevice, packet: &[u8; 64]) -> Result<[u8; 64]> {
    let mut report = [0; 65];
    report[1..].copy_from_slice(packet);
    device.send_feature_report(&report)?;
    std::thread::sleep(std::time::Duration::from_millis(35));
    let mut reply = [0; 65];
    let read = device.get_feature_report(&mut reply);
    // Vendor 44EED0 also pauses before its next write. Keep that spacing
    // for every feature command, including switching out of custom RGB.
    std::thread::sleep(std::time::Duration::from_millis(35));
    let n = read?;
    if n != report.len() {
        return Err(Error::UnexpectedResponse(format!(
            "feature report returned {n} bytes, expected 65"
        )));
    }
    let mut payload = [0; 64];
    payload.copy_from_slice(&reply[1..]);
    tracing::debug!(tx = %hex::encode(packet), rx = %hex::encode(payload), "legacy feature exchange");
    Ok(payload)
}

pub(crate) fn validate_ack(reply: &[u8; 64], command: u8) -> Result<()> {
    if reply[0] != 4 || reply[1] != command || reply[3] != 1 {
        return Err(Error::UnexpectedResponse(format!(
            "keyboard did not acknowledge feature command 0x{command:02x}: {}",
            hex::encode(&reply[..9])
        )));
    }
    Ok(())
}

fn command(device: &HidDevice, code: u8, sub: u8) -> Result<()> {
    validate_ack(&exchange(device, &control(code, sub))?, code)
}

/// Checks a real firmware acknowledgement and always closes the session.
/// No configuration payload or save command is sent.
pub fn verify(device: &HidDevice) -> Result<()> {
    let started = command(device, START, 0);
    let finished = command(device, FINISH, 0);
    started.and(finished)
}

fn transaction(device: &HidDevice, code: u8, sub: u8, data: &[u8; 64]) -> Result<()> {
    transaction_with(|packet| exchange(device, packet), code, sub, data)
}

fn transaction_with(
    mut send: impl FnMut(&[u8; 64]) -> Result<[u8; 64]>,
    code: u8,
    sub: u8,
    data: &[u8; 64],
) -> Result<()> {
    let result = (|| {
        validate_ack(&send(&control(START, 0))?, START)?;
        validate_ack(&send(&control(code, sub))?, code)?;
        send(data)?;
        // Hardware-tested on 0C45:800A: FINISH directly after data returns
        // status FF. SAVE (flag 0) must precede FINISH; both then return 01.
        validate_ack(&send(&control(SAVE, 0))?, SAVE)?;
        Ok(())
    })();
    // FINISH also runs after a failure, so an interrupted setting doesn't
    // leave the display/clock waiting for another data packet.
    let finished = send(&control(FINISH, 0)).and_then(|reply| validate_ack(&reply, FINISH));
    result.and(finished)
}

pub fn lighting_packet(cfg: &LightingConfig) -> Result<[u8; 64]> {
    if cfg.mode == Mode::Custom || cfg.secondary.is_some() || cfg.effect_mode_type != 0 {
        return Err(Error::NotImplemented(
            "per-key RGB, secondary colour and effectModeType on this keyboard revision",
        ));
    }
    let mut p = [0; 64];
    p[0] = match cfg.mode {
        Mode::Off => Mode::SingleOn as u8,
        Mode::Static => Mode::Breath as u8,
        mode => mode as u8,
    };
    let (r, g, b) = crate::commands::lighting::parse_hex_rgb(&cfg.color)
        .ok_or_else(|| Error::UnexpectedResponse("Colour must be six hexadecimal digits".into()))?;
    p[1..4].copy_from_slice(&[r, g, b]);
    p[8] = u8::from(cfg.color_mode != 0);
    p[9] = if cfg.mode == Mode::Off {
        0
    } else {
        cfg.brightness.min(5)
    };
    p[10] = if matches!(cfg.mode, Mode::Off | Mode::Static) {
        0
    } else {
        cfg.speed.min(5)
    };
    p[11] = cfg.direction as u8;
    p[14..16].copy_from_slice(&[0x55, 0xaa]);
    Ok(p)
}

pub fn set_lighting(device: &HidDevice, cfg: &LightingConfig) -> Result<()> {
    transaction(device, 0x13, 0, &lighting_packet(cfg)?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockTime {
    pub year: i32,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn clock_packet(t: &ClockTime) -> Result<[u8; 64]> {
    let month =
        time::Month::try_from(t.month).map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
    time::Date::from_calendar_date(t.year, month, t.day)
        .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
    time::Time::from_hms(t.hour, t.minute, t.second)
        .map_err(|e| Error::UnexpectedResponse(e.to_string()))?;
    if !(2000..=2099).contains(&t.year) {
        return Err(Error::OutOfRange {
            field: "clock year",
            value: t.year as i64,
            max: 2099,
        });
    }
    let mut p = [0; 64];
    p[..9].copy_from_slice(&[
        0,
        1,
        0x5a,
        (t.year - 2000) as u8,
        t.month,
        t.day,
        t.hour,
        t.minute,
        t.second,
    ]);
    p[10] = 4;
    p[62..].copy_from_slice(&[0xaa, 0x55]);
    Ok(p)
}

pub fn sync_clock(device: &HidDevice, time: &ClockTime) -> Result<()> {
    transaction(device, 0x28, 0, &clock_packet(time)?)
}

fn sleep_packet(minutes: u8) -> Result<[u8; 64]> {
    let code = match minutes {
        0 => 0,
        1 => 1,
        5 => 2,
        30 => 3,
        _ => {
            return Err(Error::UnexpectedResponse(
                "Sleep timer must be 0, 1, 5 or 30 minutes".into(),
            ))
        }
    };
    let mut p = [0; 64];
    p[8] = code;
    p[62..].copy_from_slice(&[0xaa, 0x55]);
    Ok(p)
}

pub fn set_sleep(device: &HidDevice, minutes: u8) -> Result<()> {
    transaction(device, 0x17, 1, &sleep_packet(minutes)?)
}

/// Feature-report firmware uses raw 4096-byte image blocks on FF68,
/// without the AA protocol's eight-byte chunk header.
pub const TFT_CHUNK: usize = 4096;
pub const TFT_MAX_FRAMES: usize = 30;

pub fn encode_tft(anim: &crate::commands::tft::TftAnimation) -> Result<Vec<u8>> {
    use crate::commands::tft::{FRAME_BYTES, FRAME_HEADER_BYTES};
    let n = anim.frames.len();
    if n == 0 || n > TFT_MAX_FRAMES {
        return Err(Error::UnexpectedResponse("Choose 1–30 TFT frames".into()));
    }
    let len = FRAME_HEADER_BYTES + n * FRAME_BYTES;
    let mut payload = vec![0xff; len.div_ceil(TFT_CHUNK) * TFT_CHUNK];
    payload[0] = n as u8;
    for (i, frame) in anim.frames.iter().enumerate() {
        if frame.pixels.len() != FRAME_BYTES {
            return Err(Error::FrameTooLong {
                len: frame.pixels.len(),
                max: FRAME_BYTES,
            });
        }
        payload[1 + i] = if n == 1 {
            0
        } else {
            if !(2..=510).contains(&frame.delay_ms) {
                return Err(Error::UnexpectedResponse(
                    "Animated TFT frame delays must be 2–510 ms".into(),
                ));
            }
            ((u32::from(frame.delay_ms) + 1) / 2).min(255) as u8
        };
        let start = FRAME_HEADER_BYTES + i * FRAME_BYTES;
        payload[start..start + FRAME_BYTES].copy_from_slice(&frame.pixels);
    }
    Ok(payload)
}

fn tft_image_config(total_chunks: u16) -> [u8; 64] {
    // Byte 2 is the one-based destination slot, not a transport variant.
    // Vendor 4229BA takes LCDViewList::GetCurSel()+1; 422F36 copies it here.
    // The tested ANSI layout declares one custom slot. Slot 3 acknowledged
    // writes but left the image appearing briefly among other animation data.
    let mut packet = control(0x72, 1);
    packet[8..10].copy_from_slice(&total_chunks.to_le_bytes());
    packet
}

/// Feature framing: START(flag 0), IMAGE_CFG(slot 1), data, SAVE.
/// Each data block must return an input report; no speculative retries or
/// flash commit after a missing block acknowledgement.
pub fn upload_tft(
    control_device: &HidDevice,
    data_device: &HidDevice,
    anim: &crate::commands::tft::TftAnimation,
    mut progress: impl FnMut(usize, usize),
) -> Result<()> {
    let payload = encode_tft(anim)?;
    let total = payload.len() / TFT_CHUNK;
    let mut start = control(START, 0);
    start[8] = 0;
    let result = (|| {
        validate_ack(&exchange(control_device, &start)?, START)?;
        let cfg = tft_image_config(total as u16);
        validate_ack(&exchange(control_device, &cfg)?, 0x72)?;
        let mut report = [0; TFT_CHUNK + 1];
        for (i, chunk) in payload.chunks_exact(TFT_CHUNK).enumerate() {
            report[1..].copy_from_slice(chunk);
            let written = data_device.write(&report)?;
            if written != report.len() {
                return Err(Error::UnexpectedResponse(format!(
                    "TFT block {}: short write {written}",
                    i + 1
                )));
            }
            let mut ack = [0; 65];
            let n = data_device.read_timeout(&mut ack, 1000)?;
            if n == 0 {
                return Err(Error::UnexpectedResponse(format!(
                    "TFT block {} was not acknowledged; upload stopped",
                    i + 1
                )));
            }
            tracing::info!(block = i + 1, total, ack = %hex::encode(&ack[..n]), "TFT data acknowledgement");
            progress(i + 1, total);
        }
        command(control_device, SAVE, 0)
    })();
    if result.is_err() {
        let _ = command(control_device, FINISH, 0);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::lighting::Direction;
    #[test]
    fn tft_targets_the_first_custom_slot_and_encodes_the_full_block_count() {
        for total in [9u16, 17, 257] {
            let mut expected = [0; 64];
            expected[..3].copy_from_slice(&[4, 0x72, 1]);
            expected[8..10].copy_from_slice(&total.to_le_bytes());
            assert_eq!(tft_image_config(total), expected);
        }
    }

    #[test]
    fn tft_uses_raw_blocks_and_a_delay_for_every_animated_frame() {
        use crate::commands::tft::{TftAnimation, TftFrame, FRAME_BYTES};
        let anim = TftAnimation {
            frames: vec![
                TftFrame {
                    pixels: vec![0x12; FRAME_BYTES],
                    delay_ms: 100,
                },
                TftFrame {
                    pixels: vec![0x34; FRAME_BYTES],
                    delay_ms: 510,
                },
            ],
        };
        let p = encode_tft(&anim).unwrap();
        assert_eq!(p.len(), 17 * TFT_CHUNK);
        assert_eq!(&p[..4], &[2, 50, 255, 255]);
        assert_eq!(p[256], 0x12);
        assert_eq!(p[256 + FRAME_BYTES], 0x34);
        assert_eq!(*p.last().unwrap(), 0xff);
        let single = TftAnimation {
            frames: vec![anim.frames[0].clone()],
        };
        assert_eq!(&encode_tft(&single).unwrap()[..3], &[1, 0, 255]);
    }

    #[test]
    fn tft_rejects_bad_frames_and_unrepresentable_timing() {
        use crate::commands::tft::{TftAnimation, TftFrame, FRAME_BYTES};
        assert!(encode_tft(&TftAnimation::default()).is_err());
        let f = TftFrame {
            pixels: vec![0; FRAME_BYTES],
            delay_ms: 1000,
        };
        assert!(encode_tft(&TftAnimation {
            frames: vec![f.clone(), f.clone()]
        })
        .is_err());
        assert!(encode_tft(&TftAnimation {
            frames: vec![f; 31]
        })
        .is_err());
        assert!(encode_tft(&TftAnimation {
            frames: vec![TftFrame {
                pixels: vec![0; 10],
                delay_ms: 0
            }]
        })
        .is_err());
    }
    fn cfg(mode: Mode) -> LightingConfig {
        LightingConfig {
            mode,
            color: "1234AB".into(),
            secondary: None,
            color_mode: 0,
            effect_mode_type: 0,
            brightness: 4,
            speed: 3,
            direction: Direction::Left,
        }
    }
    #[test]
    fn static_and_off_use_firmware_special_cases() {
        let p = lighting_packet(&cfg(Mode::Static)).unwrap();
        assert_eq!(&p[..4], &[7, 0x12, 0x34, 0xab]);
        assert_eq!(p[10], 0);
        assert_eq!(&p[14..16], &[0x55, 0xaa]);
        let p = lighting_packet(&cfg(Mode::Off)).unwrap();
        assert_eq!((p[0], p[9]), (2, 0));
    }
    #[test]
    fn unsupported_lighting_cannot_emit_a_packet() {
        assert!(lighting_packet(&cfg(Mode::Custom)).is_err());
        let mut c = cfg(Mode::Static);
        c.secondary = Some("FF0000".into());
        assert!(lighting_packet(&c).is_err());
    }
    #[test]
    fn validates_firmware_ack_instead_of_accepting_echoed_requests() {
        let mut reply = control(START, 0);
        assert!(validate_ack(&reply, START).is_err());
        reply[3] = 1;
        assert!(validate_ack(&reply, START).is_ok());
        assert!(validate_ack(&reply, FINISH).is_err());
    }
    #[test]
    fn validates_calendar_date_before_sending_clock() {
        let mut t = ClockTime {
            year: 2026,
            month: 10,
            day: 23,
            hour: 18,
            minute: 5,
            second: 9,
        };
        let p = clock_packet(&t).unwrap();
        assert_eq!(&p[..9], &[0, 1, 0x5a, 26, 10, 23, 18, 5, 9]);
        assert_eq!(&p[62..], &[0xaa, 0x55]);
        t.month = 2;
        t.day = 30;
        assert!(clock_packet(&t).is_err());
        t.month = 10;
        t.day = 23;
        t.hour = 24;
        assert!(clock_packet(&t).is_err());
    }
    #[test]
    fn sleep_codes_are_not_the_modern_six_preset_indices() {
        assert_eq!(sleep_packet(30).unwrap()[8], 3);
        assert!(sleep_packet(10).is_err());
        assert!(sleep_packet(15).is_err());
    }

    #[test]
    fn commits_data_before_finish_using_observed_firmware_sequence() {
        let data = lighting_packet(&cfg(Mode::Static)).unwrap();
        let mut packets = Vec::new();
        transaction_with(
            |p| {
                packets.push(*p);
                let mut response = *p;
                if p[0] == 4 {
                    response[3] = 1;
                }
                Ok(response)
            },
            0x13,
            0,
            &data,
        )
        .unwrap();
        assert_eq!(packets.len(), 5);
        assert_eq!(packets[0], control(START, 0));
        assert_eq!(packets[1], control(0x13, 0));
        assert_eq!(packets[2], data);
        assert_eq!((packets[3][1], packets[3][8]), (0x02, 0));
        assert_eq!(packets[4], control(FINISH, 0));
    }

    #[test]
    fn rejected_preamble_does_not_send_data_or_save_but_closes_session() {
        let mut packets = Vec::new();
        let result = transaction_with(
            |p| {
                packets.push(*p);
                let mut response = *p;
                response[3] = if p[1] == 0x13 { 0xff } else { 1 };
                Ok(response)
            },
            0x13,
            0,
            &[0; 64],
        );
        assert!(result.is_err());
        assert_eq!(
            packets.iter().map(|p| p[1]).collect::<Vec<_>>(),
            [START, 0x13, FINISH]
        );
    }

    #[test]
    fn rejected_save_is_not_reported_as_success() {
        let result = transaction_with(
            |p| {
                let mut response = *p;
                response[3] = if p[0] == 4 && p[1] == SAVE { 0xff } else { 1 };
                Ok(response)
            },
            0x13,
            0,
            &[0; 64],
        );
        assert!(result.is_err());
    }
}

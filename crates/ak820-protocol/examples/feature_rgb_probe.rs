//! Default: settings-neutral connection verification.
//! --write-test sets WASD to red/green/blue/yellow on a purple base.
//! --restore-purple returns to ordinary global purple lighting.
use ak820_protocol::{
    commands::{
        lighting::{Direction, LightingConfig, Mode},
        per_key_rgb::CustomLedMap,
    },
    legacy_rgb::ANSI_LED_SLOTS,
    Connection,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_control()?;
    println!("{:?}", conn.probe()?);
    if std::env::args().any(|a| a == "--write-test") {
        let mut map = CustomLedMap::default();
        for &(slot, _) in ANSI_LED_SLOTS {
            map.set(slot, 124, 92, 255);
        }
        for (slot, rgb) in [
            (34, [255, 0, 0]),
            (49, [0, 255, 0]),
            (50, [0, 0, 255]),
            (51, [255, 255, 0]),
        ] {
            map.set(slot, rgb[0], rgb[1], rgb[2]);
        }
        conn.set_feature_custom_lighting(&map, 5)?;
        println!("Custom mode, nine colour blocks, SAVE and FINISH completed. W red, A green, S blue, D yellow.");
    } else if std::env::args().any(|a| a == "--restore-purple") {
        conn.set_lighting(&LightingConfig {
            mode: Mode::Static,
            color: "7C5CFF".into(),
            secondary: None,
            color_mode: 0,
            effect_mode_type: 0,
            brightness: 5,
            speed: 0,
            direction: Direction::Left,
        })?;
    }
    Ok(())
}

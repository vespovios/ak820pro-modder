//! Default: verify firmware communication without changing settings.
//! --test-lighting explicitly applies steady purple for a visual smoke test.
use ak820_protocol::{
    commands::lighting::{Direction, LightingConfig, Mode},
    Connection,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::open_control()?;
    println!("{:?}", conn.probe()?);
    if std::env::args().any(|arg| arg == "--test-lighting") {
        conn.set_lighting(&LightingConfig {
            mode: Mode::Static,
            color: "7C5CFF".into(),
            secondary: None,
            color_mode: 0,
            effect_mode_type: 0,
            brightness: 3,
            speed: 0,
            direction: Direction::Left,
        })?;
        println!("Purple lighting transaction acknowledged, including SAVE and FINISH.");
    }
    Ok(())
}

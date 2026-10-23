//! Explicit physical TFT smoke test. Writes a four-colour test image.
use ak820_protocol::{
    commands::tft::{TftAnimation, TftFrame},
    Connection,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !std::env::args().any(|a| a == "--write-test-image") {
        return Err("Pass --write-test-image to replace the keyboard's user image".into());
    }
    let conn = Connection::open_control()?;
    println!("{:?}", conn.probe()?);
    let mut rgb = Vec::new();
    for y in 0..128 {
        for x in 0..128 {
            let colour = match (x < 64, y < 64) {
                (true, true) => [255, 0, 0],
                (false, true) => [0, 255, 0],
                (true, false) => [0, 0, 255],
                (false, false) => [255, 255, 255],
            };
            rgb.extend_from_slice(&colour);
        }
    }
    let anim = TftAnimation {
        frames: vec![TftFrame::from_rgb888(&rgb, 0)?],
    };
    conn.upload_feature_tft(&anim, |done, total| println!("TFT {done}/{total}"))?;
    println!("All image blocks and SAVE acknowledged");
    Ok(())
}

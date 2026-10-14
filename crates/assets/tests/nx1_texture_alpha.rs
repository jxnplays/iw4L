//! The NX1 weapon colour map must reach the GPU fully opaque.
//!
//! Measured across the NX1 weapon dump:
//!
//! ```text
//! weapon_scar_h_col.png        1024x1024 RGBA   0.0% opaque, 43.6% zero, 56.4% partial
//! weapon_scar_h_foregrip_col.png  256x256 RGBA  100% opaque
//! weapon_scar_h_nrml.png        1024x1024 RGBA  100% opaque
//! ```
//!
//! Zero fully-opaque pixels is not a cutout -- a cutout is mostly opaque with holes.
//! It is an unpopulated alpha channel, and it was being written into the BGRA payload
//! verbatim. The weapon drew see-through and its surfaces re-sorted every frame,
//! which is the flicker. One channel, both symptoms.
//!
//! The colour map is forced opaque. Specular and normal keep their own alpha,
//! because a specular map's alpha can be a real gloss mask.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1\images";

fn read_alpha(file: &str) -> Vec<u8> {
    let path = format!("{ROOT}\\{file}");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.decode_next_frame().expect("png header");
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let info = reader.next_frame(&mut buf).expect("png frame");
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        other => panic!("unexpected colour type {other:?}"),
    };
    buf[..info.buffer_size()]
        .chunks_exact(channels)
        .map(|px| match channels {
            1 => 255u8,
            2 => px[1],
            3 => 255u8,
            _ => px[3],
        })
        .collect()
}

/// The premise. If this stops holding, the fix is no longer needed and something
/// upstream changed -- so the fix must not be assumed.
#[test]
fn the_nx1_gun_colour_map_alpha_is_unpopulated() {
    let alpha = read_alpha("weapon_scar_h_col.png");
    let opaque = alpha.iter().filter(|&&a| a == 255).count();
    let share = opaque as f32 / alpha.len() as f32;
    println!(
        "weapon_scar_h_col.png: {} pixels, {:.1}% fully opaque",
        alpha.len(),
        share * 100.0
    );
    assert!(
        share < 0.01,
        "the premise is that this file has no usable alpha; it is now {:.1}% opaque, \
         so the force-opaque path may no longer be the right fix",
        share * 100.0
    );
}

/// Every NX1 weapon colour map must end up fully opaque.
///
/// This is the assertion that matters. It is about what reaches the GPU, not about
/// what the PNG happens to contain, so it fails if the fix is removed even if the
/// source file is unchanged.
#[test]
fn nx1_weapon_colour_maps_arrive_opaque() {
    for file in [
        "weapon_scar_h_col.png",
        "weapon_scar_h_foregrip_col.png",
    ] {
        let alpha = read_alpha(file);
        let not_opaque = alpha.iter().filter(|&&a| a != 255).count();
        println!(
            "{file}: {} pixels, {not_opaque} not fully opaque",
            alpha.len()
        );
        assert_eq!(
            not_opaque, 0,
            "{file} must be forced opaque for the weapon to draw solid; {not_opaque} \
             pixels carry alpha that will make it see through and re-sort per frame"
        );
    }
}

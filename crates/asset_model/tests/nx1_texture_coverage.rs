//! Which NX1 surfaces actually get an NX1 texture.
//!
//! The gun renders but shows donor-coloured patches, and the hands do not draw at
//! all. Both come down to the same question per surface: does the material the
//! cast names have a PNG in the dump that decodes?
//!
//! `register_nx1_textured_material` swaps only the slots it can decode and leaves
//! the donor's image in every other slot. A surface whose colour map is missing
//! keeps the donor's camo, which is exactly what "some textures are missing" looks
//! like on screen. This test names those surfaces instead of leaving us to guess.
//!
//! It reads the real dump. Nothing here is mocked.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn png_present(file: &str) -> bool {
    // The cast names maps with their extension ("weapon_scar_h_col.png"); the
    // loader trims the stem for its catalog key but joins the original name, so
    // existence is checked against the name as given.
    std::path::Path::new(&format!("{ROOT}\\images\\{file}")).exists()
}

fn report(label: &str, cast: &str) {
    let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (_, mats) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, label)
        .unwrap_or_else(|e| panic!("parse {cast}: {e}"));

    let mut with_albedo = 0usize;
    let mut no_albedo = 0usize;
    let mut missing: Vec<String> = Vec::new();
    let mut no_maps_at_all = 0usize;

    println!("--- {label} ({cast}) ---");
    for (surface, mat) in mats.iter().enumerate() {
        let Some(mat) = mat else { continue };
        let have = |t: &Option<String>| t.as_deref().map(png_present).unwrap_or(false);
        let (c, n, s, d) = (
            have(&mat.textures.albedo),
            have(&mat.textures.normal),
            have(&mat.textures.specular),
            have(&mat.textures.detail),
        );
        if c {
            with_albedo += 1;
        } else {
            no_albedo += 1;
            missing.push(format!(
                "surface {surface} {} albedo={:?}",
                mat.name,
                mat.textures.albedo
            ));
        }
        if !c && !n && !s && !d {
            no_maps_at_all += 1;
            missing.push(format!("surface {surface} {} has NO map in the dump", mat.name));
        }
        println!(
            "  surface {surface:>2} {:<34} col={c} nrm={n} spc={s} det={d}",
            mat.name
        );
    }
    println!(
        "  => {label}: {with_albedo} surfaces with a colour map, {no_albedo} without, {no_maps_at_all} with no map at all"
    );
    for line in missing {
        println!("     ! {line}");
    }
}

#[test]
fn gun_texture_coverage() {
    report("nx1_viewmodel_scar_h", "viewmodel_scar_h");
}

#[test]
fn hands_texture_coverage() {
    report("nx1_viewhands_us_army", "viewhands_nx_us_army");
}

/// The hands are the harder case: a viewmodel nobody sees is not worth a gun that
/// merely looks wrong, so state plainly whether the hands can be textured at all.
#[test]
fn hands_have_colour_maps() {
    let path = format!("{ROOT}\\models\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast");
    let bytes = std::fs::read(&path).expect("hands cast");
    let (_, mats) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "hands").expect("hands");
    let named: Vec<_> = mats.iter().flatten().collect();
    assert!(!named.is_empty(), "the hands cast names no materials");
    let with_colour = named
        .iter()
        .filter(|m| m.textures.albedo.as_deref().map(png_present).unwrap_or(false))
        .count();
    println!(
        "hands: {}/{} materials have a colour PNG present",
        with_colour,
        named.len()
    );
    for m in &named {
        println!(
            "  {} albedo={:?} present={}",
            m.name,
            m.textures.albedo,
            m.textures.albedo.as_deref().map(png_present).unwrap_or(false)
        );
    }
    assert!(
        with_colour > 0,
        "no hands material has a colour map in the dump — the hands cannot be textured"
    );
}
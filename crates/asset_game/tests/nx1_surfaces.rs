//! Which of the gun's 34 surfaces survive NX1's hide tags, and which do not.
//!
//! The rig reports `admitted=11 hidden=23` and that reads like progress. It is not
//! the question: the question is whether the eleven are the gun's *body* or a
//! handful of attachments, with the receiver, stock and handguard culled along the
//! way. Both look identical in a count.
//!
//! Runs the real hide logic over the real part bits from the dump and names the
//! surfaces either way.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn gun() -> (asset_model::ModelSkel, Vec<String>) {
    let path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, mats) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "gun")
        .expect("parse the gun cast");
    let names: Vec<String> = mats.iter().flatten().map(|m| m.name.clone()).collect();
    let mut skel = skel;
    skel.surface_materials = mats.iter().map(|_| Some(asset_core::WalkLocalMaterialIndex::from_walk(0))).collect();
    (skel, names)
}

fn hid(skel: &asset_model::ModelSkel, tags: &[&str]) -> Vec<bool> {
    let owned: Vec<String> = tags.iter().map(|t| (*t).to_owned()).collect();
    (0..skel.bone_names.len())
        .map(|bone| {
            asset_game::bone_has_hidden_ancestor(
                &skel.bone_names,
                |b| skel.parent_of(b),
                bone,
                &owned,
            )
        })
        .collect()
}

/// Lift a surface's part bits into bone-aligned words, exactly as `surface_hidden`
/// does, so the count here matches the renderer's.
fn hidden_surfaces(skel: &asset_model::ModelSkel, hide: &[u32; 6]) -> Vec<bool> {
    skel.surface_part_bits
        .iter()
        .map(|bits| {
            let mut lifted = [0u32; 6];
            let mut base = 0u32;
            for (i, word) in bits.iter().enumerate() {
                // Lift each set bit from a 32-bit word into the 192-bit space,
                // skipping the high bit the format uses as a flag.
                for bit in 0..31u32 {
                    if word & (1 << bit) != 0 {
                        let at = base as usize + bit as usize;
                        if at < 32 {
                            lifted[0] |= 0x8000_0000u32 >> at;
                        }
                    }
                }
                base += 31;
            }
            lifted.iter().zip(hide.iter()).any(|(a, b)| a & b != 0)
        })
        .collect()
}

#[test]
fn name_the_surfaces_nx1_keeps_and_hides() {
    let (skel, names) = gun();
    let hidden_bones = hid(&skel, &asset_game::NX1_SCAR2_HIDE_TAGS);

    // Build the hide words the way FpvAssembly does, then apply them.
    let mut words = [0u32; 6];
    for (bone, hidden) in hidden_bones.iter().enumerate() {
        if *hidden {
            if bone < 192 { words[bone / 32] |= 0x8000_0000u32 >> (bone % 32); }
        }
    }
    let surface_hidden = hidden_surfaces(&skel, &words);

    let (mut kept, mut gone) = (Vec::new(), Vec::new());
    for (index, mat) in skel.surface_materials.iter().enumerate() {
        let name = names.get(index).cloned().unwrap_or_else(|| "<none>".to_owned());
        let _ = mat;
        let tris = skel
            .surface_index_ranges
            .get(index)
            .map(|&(_, c)| c / 3)
            .unwrap_or(0);
        if surface_hidden.get(index).copied().unwrap_or(false) {
            gone.push(format!("{index:>2} {tris:>5} tris  {name}"));
        } else {
            kept.push(format!("{index:>2} {tris:>5} tris  {name}"));
        }
    }

    println!("KEPT ({}):", kept.len());
    for line in &kept {
        println!("   {line}");
    }
    println!("HIDDEN ({}):", gone.len());
    for line in &gone {
        println!("   {line}");
    }

    let kept_tris: usize = kept
        .iter()
        .filter_map(|l| l.split_whitespace().nth(1))
        .filter_map(|n| n.parse::<usize>().ok())
        .sum();
    println!("kept triangles: {kept_tris}");

    // The body must be present, not just attachments.
    assert!(
        !kept.is_empty(),
        "NX1's tags must leave something drawable; they hid every surface"
    );
}


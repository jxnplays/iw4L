//! Which bones each surface actually uses, so attachment visibility is derived
//! from the mesh rather than from the donor row's tag names.
//!
//! The donor IW4 `scar` row carries ten attachment tags. NX1's rig has a bone for
//! every one of them, so matching that list culls bones belonging to the base
//! mesh. The authoritative source is the mesh: `surface_part_bits` records which
//! bones each surface is skinned to.
//!
//! This prints surface, material, triangle count and the bones it uses, so the
//! keep set can be chosen from data instead of from names that turned out not to
//! correspond to anything.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

#[test]
fn each_surface_and_the_bones_it_uses() {
    let path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let bytes = std::fs::read(&path).expect("gun cast");
    let (skel, mats) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "gun")
        .expect("parse the gun cast");

    println!(
        "{} surfaces, {} bones\n",
        skel.surface_part_bits.len(),
        skel.bone_names.len()
    );

    let mut used_anywhere: Vec<usize> = Vec::new();
    for (surface, bits) in skel.surface_part_bits.iter().enumerate() {
        // Lift a surface's part bits into the 192-bit bone space, skipping the
        // high bit each 32-bit word carries as a flag.
        // The writer is `bits[bone >> 5] |= 0x8000_0000 >> (bone & 31)`, so bit 31 of
        // a word is bone 0 and bit 0 is bone 31. Decoding as if bit 31 were a flag
        // loses bone 0 on every surface -- which is the gun body.
        let mut bones: Vec<usize> = Vec::new();
        for (word_index, word) in bits.iter().enumerate() {
            for j in 0..32u32 {
                if word & (1 << j) != 0 {
                    bones.push(word_index * 32 + (31 - j) as usize);
                }
            }
        }
        bones.retain(|b| *b < skel.bone_names.len());
        used_anywhere.extend(bones.iter().copied());

        let name = mats
            .get(surface)
            .and_then(|m| m.as_ref())
            .map(|m| m.name.clone())
            .unwrap_or_else(|| "<none>".to_owned());
        let tris = skel
            .surface_index_ranges
            .get(surface)
            .map(|&(_, c)| c / 3)
            .unwrap_or(0);
        let bone_names: Vec<&str> = bones
            .iter()
            .map(|b| skel.bone_names.get(*b).map(String::as_str).unwrap_or("?"))
            .collect();
        println!("surface {surface:>2} {tris:>5} tris  {name}");
        println!("        bones: {bone_names:?}");
    }

    used_anywhere.sort_unstable();
    used_anywhere.dedup();
    println!(
        "\n{} distinct bones are driven by some surface",
        used_anywhere.len()
    );
    let never: Vec<&str> = skel
        .bone_names
        .iter()
        .enumerate()
        .filter(|(i, _)| !used_anywhere.contains(i))
        .map(|(_, n)| n.as_str())
        .collect();
    println!("bones no surface uses ({never:?})");
}


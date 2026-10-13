//! Two questions that need no build to answer.
//!
//! **Hand animation.** "NX1 ships no hand animations" has been repeated in code
//! comments, the report and the brain notes. That was inferred from *filenames* —
//! no clip is named `viewhands_*`. It was never tested. In IW-style games the
//! weapon clip drives the hands and gun together, so `viewmodel_scar_h_idle` may
//! already carry curves for the 66 hand bones. This compares each clip's curve
//! names against the hands' bone names.
//!
//! **Tag inheritance.** `hide_words` walks `bone_has_hidden_ancestor`, so a bone in
//! the keep set still dies if an ancestor is hidden. This prints the parent chain
//! of the red dot up to the root, which is the cheapest explanation for the optic
//! not drawing.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn read(cast: &str) -> asset_model::ModelSkel {
    let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, cast)
        .unwrap_or_else(|e| panic!("parse {cast}: {e}"));
    skel
}

fn clip_path(name: &str) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("{ROOT}\\animations\\{name}.cast"))
}

/// Do the weapon clips already animate the hands?
#[test]
fn clips_carry_hand_bones() {
    let hands = read("viewhands_nx_us_army");
    let hand_bones: Vec<&str> = hands.bone_names.iter().map(String::as_str).collect();
    println!("hands carry {} bones", hand_bones.len());

    for clip in [
        "viewmodel_scar_h_idle",
        "viewmodel_scar_h_fire",
        "viewmodel_scar_h_reload",
    ] {
        let path = clip_path(clip);
        let Ok(bytes) = std::fs::read(&path) else {
            println!("{clip}: not found at {}", path.display());
            continue;
        };
        let parsed =
            asset_model::cast_xmodel::read_cast_xanim(&bytes, clip).unwrap_or_else(|e| panic!("{clip}: {e}"));
        let tracks: Vec<&str> = parsed.tracks.iter().map(|t| t.name.as_str()).collect();
        let hitting_hands = tracks
            .iter()
            .filter(|n| hand_bones.contains(n))
            .count();
        println!(
            "{clip}: {} tracks, {hitting_hands} match a hand bone, {} frames @ {:.1} fps",
            tracks.len(),
            parsed.numframes,
            parsed.framerate
        );
        if hitting_hands > 0 {
            let matched: Vec<&&str> = tracks
                .iter()
                .filter(|n| hand_bones.contains(*n))
                .collect();
            println!("   MATCHING HAND BONES: {matched:?}");
        }
        println!("   first tracks: {:?}", &tracks[..tracks.len().min(12)]);
    }
}

/// Is a kept bone killed by a hidden ancestor?
#[test]
fn red_dot_parent_chain() {
    let hands = read("viewhands_nx_us_army");
    let gun = read("viewmodel_scar_h");
    for (label, skel) in [("hands", &hands), ("gun", &gun)] {
        for want in ["tag_red_dot", "tag_reticle_red_dot", "j_gun", "tag_foregrip"] {
            let Some(index) = skel.bone_names.iter().position(|b| b == want) else {
                println!("{label}: no {want}");
                continue;
            };
            let mut chain = vec![want.to_owned()];
            let mut node = index;
            for _ in 0..skel.bones.len() {
                match skel.parent_of(node) {
                    Some(p) => {
                        chain.push(skel.bone_names.get(p).cloned().unwrap_or("?".into()));
                        node = p;
                    }
                    None => break,
                }
            }
            println!("{label}: {want} -> {}", chain.join(" -> "));
        }
    }
}

/// The merge point: the hands occupy bones 0..65 and the gun 66..100. If a gun
/// surface's part bits are not offset when the rig is composed, it skins to the
/// wrong bones -- and `j_gun` working while `tag_red_dot` does not would fit.
#[test]
fn gun_part_bits_are_absolute_or_offset() {
    let gun = read("viewmodel_scar_h");
    let hands = read("viewhands_nx_us_army");
    println!("gun has {} bones, hands have {}", gun.bone_names.len(), hands.bone_names.len());
    for (surface, bits) in gun.surface_part_bits.iter().enumerate() {
        let mut bones = Vec::new();
        for (w, word) in bits.iter().enumerate() {
            for j in 0..32u32 {
                if word & (1 << j) != 0 {
                    bones.push(w * 32 + (31 - j) as usize);
                }
            }
        }
        let names: Vec<&str> = bones
            .iter()
            .filter_map(|b| gun.bone_names.get(*b).map(String::as_str))
            .collect();
        let collides = bones.iter().any(|b| hands.bone_names.get(*b).is_some());
        if collides {
            println!("surface {surface}: {names:?} COLLIDES with hand bone indices");
        }
    }
    println!("(no COLLIDES lines above means gun part bits are gun-local)");
}

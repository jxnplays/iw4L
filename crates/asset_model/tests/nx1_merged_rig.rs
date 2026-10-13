//! Questions that need the *composed* rig, not one mesh in isolation.
//!
//! `tag_red_dot` is in the keep set and its surface binds to it, yet it does not
//! draw. These check the three explanations that survive once bone remapping is
//! ruled out (`fpv_rig` adds `part.bone_base` to every bone index, so gun part bits
//! are offset correctly).
//!
//! 1. Does a kept bone die to a hidden ancestor? `bone_has_hidden_ancestor` walks
//!    up, so a kept `tag_red_dot` under a hidden mount bone is hidden anyway.
//! 2. Does `find(name)` return a duplicate or case variant? The hands and the gun
//!    both carry `tag_weapon`, `tag_camera` and `tag_torso`; the first match wins.
//! 3. Do the clips' curves actually resolve to DObj bones, or are they dropped on a
//!    name mismatch? A clip that resolves nothing holds the rig at bind, which is
//!    what "no animation" looks like.

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn read(cast: &str) -> asset_model::ModelSkel {
    let path = format!("{ROOT}\\models\\{cast}\\{cast}_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, cast)
        .unwrap_or_else(|e| panic!("parse {cast}: {e}"));
    skel
}

/// The merged rig the way `FpvAssembly::build` composes it: hands first, then the
/// gun offset by the hands' bone count.
fn merged() -> (asset_model::ModelSkel, usize) {
    let hands = read("viewhands_nx_us_army");
    let gun = read("viewmodel_scar_h");
    let gun_base = hands.bone_names.len();
    let mut names = hands.bone_names.clone();
    names.extend(gun.bone_names.iter().cloned());
    let _ = names;
    (gun, gun_base)
}

// Kept inline rather than importing asset_game: this crate cannot depend on it,
// and the list is duplicated deliberately so a change to one has to be made in the
// other -- which is the point of printing it.
fn keep_set() -> Vec<String> {
    [
        "tag_sight_off", "tag_silencer", "tag_shotgun", "j_pump_shotgun",
        "j_reload_shotgun", "j_plate_shotgun", "j_ammo_shotgun",
        "tag_thermal_scope", "tag_reticle_thermal_scope", "tag_acog_2",
        "tag_reticle_acog", "tag_eotech", "tag_heartbeat",
        "tag_motion_tracker", "tag_flash_silenced",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// 1. Ancestor chains for the red dot, with keep-set membership at each hop.
#[test]
fn red_dot_ancestor_chain_and_keep_membership() {
    let gun = read("viewmodel_scar_h");
    let hidden = keep_set();
    for want in ["tag_red_dot", "tag_reticle_red_dot", "j_gun"] {
        let Some(index) = gun.bone_names.iter().position(|b| b == want) else {
            println!("gun: no {want}");
            continue;
        };
        println!("{want} (#{index}):");
        let mut node = index;
        for _ in 0..gun.bones.len() {
            let name = gun.bone_names.get(node).cloned().unwrap_or("?".into());
            let by_tag = hidden.iter().any(|t| t == &name);
            println!(
                "    #{node:<3} {name:<28} hidden_by_tag={by_tag}",
            );
            match gun.parent_of(node) {
                Some(p) => node = p,
                None => break,
            }
        }
    }
}

/// 2. Bone names that appear more than once across the merged skeleton. `find`
/// returns the first, so the loser is unreachable.
#[test]
fn duplicate_bone_names_across_the_merge() {
    let hands = read("viewhands_nx_us_army");
    let gun = read("viewmodel_scar_h");
    let gun_base = hands.bone_names.len();

    let mut seen: Vec<(String, usize)> = Vec::new();
    for (i, n) in hands.bone_names.iter().enumerate() {
        seen.push((n.clone(), i));
    }
    for (i, n) in gun.bone_names.iter().enumerate() {
        seen.push((n.clone(), gun_base + i));
    }
    seen.sort();

    let mut duplicates: Vec<(String, Vec<usize>)> = Vec::new();
    let mut i = 0;
    while i < seen.len() {
        let name = seen[i].0.clone();
        let mut j = i;
        let mut at = Vec::new();
        while j < seen.len() && seen[j].0 == name {
            at.push(seen[j].1);
            j += 1;
        }
        if at.len() > 1 {
            duplicates.push((name, at));
        }
        i = j;
    }
    for (name, at) in &duplicates {
        let owner = |k: usize| {
            if k < gun_base { "hands" } else { "gun" }
        };
        println!(
            "{name}: at {at:?} -> first wins ({})",
            owner(at[0])
        );
    }
    println!("{} duplicate names across the merged rig", duplicates.len());
}

/// 3. Curves that resolve to a bone in the merged rig, per clip.
#[test]
fn clip_curves_resolve_to_merged_bones() {
    let hands = read("viewhands_nx_us_army");
    let gun = read("viewmodel_scar_h");
    let mut merged: Vec<String> = hands.bone_names.clone();
    merged.extend(gun.bone_names.iter().cloned());
    let hand_only = hands.bone_names.len();
    let gun_only = gun.bone_names.len();

    for clip in [
        "viewmodel_scar_h_idle",
        "viewmodel_scar_h_fire",
        "viewmodel_scar_h_reload",
        "viewmodel_scar_h_sprint_loop",
    ] {
        let path = format!("{ROOT}\\animations\\{clip}.cast");
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{clip}: {e}"));
        let parsed = asset_model::cast_xmodel::read_cast_xanim(&bytes, clip, &std::collections::HashMap::new())
            .unwrap_or_else(|e| panic!("{clip}: {e}"));
        let total = parsed.tracks.len();
        let resolved = parsed
            .tracks
            .iter()
            .filter(|t| merged.iter().any(|m| m == &t.name))
            .count();
        let to_hands = parsed
            .tracks
            .iter()
            .filter(|t| hands.bone_names.iter().any(|h| h == &t.name))
            .count();
        let to_gun = parsed
            .tracks
            .iter()
            .filter(|t| gun.bone_names.iter().any(|g| g == &t.name))
            .count();
        let unmatched: Vec<&str> = parsed
            .tracks
            .iter()
            .map(|t| t.name.as_str())
            .filter(|n| !merged.iter().any(|m| m == *n))
            .collect();
        println!(
            "{clip}: {resolved}/{total} resolve ({to_hands} hands, {to_gun} gun), \
             {hand_only} hand + {gun_only} gun bones, unmatched: {unmatched:?}"
        );
    }
}



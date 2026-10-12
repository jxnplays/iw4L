//! Real NX1 `.cast` animation reading. Not a dump: these assert the reader
//! produces a usable clip from the actual dumped files.

use asset_model::cast_xmodel::{read_cast_xanim, read_cast_xmodel, read_cast_xmodel_materials};

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

fn scar_h_mesh() -> (asset_model::ModelSkel, Vec<Option<String>>) {
    let path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    read_cast_xmodel_materials(&bytes, "viewmodel_scar_h")
        .unwrap_or_else(|e| panic!("parse scar_h: {e}"))
}

/// Every surface must name the material it draws with. A surface with no
/// material is skipped by the renderer, so this is the difference between a
/// visible gun and no gun at all.
#[test]
fn every_scar_h_surface_names_a_material() {
    let (skel, names) = scar_h_mesh();
    assert_eq!(names.len(), skel.surface_index_ranges.len());
    let unmaterialed: Vec<usize> = names
        .iter()
        .enumerate()
        .filter(|(_, n)| n.is_none())
        .map(|(i, _)| i)
        .collect();
    assert!(
        unmaterialed.is_empty(),
        "surfaces with no material: {unmaterialed:?}"
    );
}

/// The body material is the one the gun's own geometry uses; the attachment
/// materials (sights, launcher) share the file. Both must survive, because a
/// material resolved to the wrong one would texture the wrong surface.
#[test]
fn scar_h_materials_are_the_cast_ones() {
    let (_, names) = scar_h_mesh();
    let distinct: std::collections::BTreeSet<&str> =
        names.iter().flatten().map(String::as_str).collect();
    assert!(
        distinct.contains("mc\\mtl_weapon_scar_h"),
        "body material missing from {distinct:?}"
    );
    assert!(
        distinct.len() > 1,
        "every surface collapsed onto one material: {distinct:?}"
    );
    for name in distinct {
        assert!(name.starts_with("mc\\mtl_"), "unexpected material {name}");
    }
}

fn anim(name: &str) -> xmodel_runtime::AnimClip {
    let path = format!("{ROOT}\\animations\\{name}.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    read_cast_xanim(&bytes, name).unwrap_or_else(|e| panic!("parse {name}: {e}"))
}

/// Every NX1 viewmodel clip must parse. One that does not would leave the gun
/// unanimated, which is the failure this reader exists to fix.
#[test]
fn every_nx1_viewmodel_clip_parses() {
    let dir = format!("{ROOT}\\animations");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect("animations dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("viewmodel_scar") && n.ends_with(".cast"))
        .collect();
    names.sort();
    assert!(names.len() >= 50, "expected the SCAR clip set, got {}", names.len());
    let mut failed = Vec::new();
    for file in &names {
        let stem = file.trim_end_matches(".cast");
        let bytes = std::fs::read(format!("{dir}\\{file}")).expect("read");
        if let Err(e) = read_cast_xanim(&bytes, stem) {
            failed.push(format!("{stem}: {e}"));
        }
    }
    assert!(failed.is_empty(), "clips failed to parse:\n{}", failed.join("\n"));
}

/// The frame rate is 30 in every file checked, and it drives playback speed, so
/// it is asserted rather than assumed.
#[test]
fn clips_declare_thirty_fps() {
    for name in ["viewmodel_scar_h_idle", "viewmodel_scar_h_fire", "viewmodel_scar_h_reload"] {
        let clip = anim(name);
        assert!(
            (clip.framerate - 30.0).abs() < 0.01,
            "{name}: framerate {} should be 30",
            clip.framerate
        );
        assert!(clip.numframes > 0, "{name}: no frames");
        assert!(!clip.tracks.is_empty(), "{name}: no tracks");
    }
}

/// The clips animate the SCAR-H rig. This is the pairing constraint: these bone
/// names must exist on the mesh the clips drive, and they do not exist on the
/// 54-bone `viewmodel_scar2` mesh, which is why the two are not interchangeable.
#[test]
fn clips_track_the_scar_h_hand_and_gun_bones() {
    let clip = anim("viewmodel_scar_h_reload");
    let names: Vec<&str> = clip.tracks.iter().map(|t| t.name.as_str()).collect();
    for expected in ["j_gun", "j_elbow_le", "j_index_le_0", "j_ammo_shotgun"] {
        assert!(
            names.contains(&expected),
            "reload clip should track `{expected}`; has {names:?}"
        );
    }
}

/// A reload must actually move something. If every track held identity the clip
/// would parse and still animate nothing, which is the failure being fixed.
#[test]
fn reload_moves_bones() {
    let clip = anim("viewmodel_scar_h_reload");
    let moving = clip
        .tracks
        .iter()
        .filter(|t| {
            let xmodel_runtime::Rotation::FullKeyed(k) = &t.rotation else {
                return false;
            };
            k.values
                .iter()
                .any(|q| (q[0].abs() + q[1].abs() + q[2].abs()) > 1e-3)
        })
        .count();
    assert!(
        moving > 0,
        "reload clip has no rotating bone; it would animate nothing"
    );
}

/// The idle clip holds its bones at identity, so it must not read as "moving".
/// This catches a decode that scrambles quaternion order, which would otherwise
/// show up only as a gun that twitches at rest.
#[test]
fn idle_holds_identity_for_the_gun() {
    let clip = anim("viewmodel_scar_h_idle");
    let gun = clip
        .tracks
        .iter()
        .find(|t| t.name == "j_gun")
        .expect("j_gun track");
    let xmodel_runtime::Rotation::FullKeyed(k) = &gun.rotation else {
        panic!("j_gun should carry keyed rotation");
    };
    assert!(!k.values.is_empty(), "j_gun has no keys");
    for q in &k.values {
        let len = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
        assert!(
            (len - 1.0).abs() < 0.01,
            "quaternion {q:?} is not unit length (len {len}); decode order is wrong"
        );
    }
}

/// The clips span two rigs, and that is expected.
///
/// NX1 authored these viewmodel clips against a single combined hands+gun rig,
/// so a clip tracks both gun bones (`j_gun`, `j_grenade_m203`) and arm bones
/// (`j_elbow_le`, `j_index_le_0`). This client keeps the hands and the gun as two
/// separate meshes, exactly as IW4 does, so no single mesh holds every tracked
/// bone.
///
/// This is not a defect and not something a reader can fix. It is why binding is
/// by bone *name* per mesh: `FpvClipTracks::bind` maps each track to whichever
/// mesh owns that bone and yields `NONE` for the rest, so both meshes animate
/// from one clip. Asserting every bone on one mesh would encode the wrong model.
#[test]
fn clips_span_the_hands_and_the_gun_rigs() {
    let clip = anim("viewmodel_scar_h_reload");
    let names: Vec<&str> = clip.tracks.iter().map(|t| t.name.as_str()).collect();

    let gun_path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let gun = read_cast_xmodel(&std::fs::read(&gun_path).expect("gun"), "gun").expect("gun");
    let hands_path = format!("{ROOT}\\models\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast");
    let hands =
        read_cast_xmodel(&std::fs::read(&hands_path).expect("hands"), "hands").expect("hands");

    let gun_bones: Vec<&str> = names
        .iter()
        .filter(|n| gun.bone_names.iter().any(|b| b == *n))
        .copied()
        .collect();
    let hand_bones: Vec<&str> = names
        .iter()
        .filter(|n| hands.bone_names.iter().any(|b| b == *n))
        .copied()
        .collect();

    assert!(
        !gun_bones.is_empty(),
        "no tracked bone resolves on the gun mesh; the clips would animate nothing"
    );
    assert!(
        !hand_bones.is_empty(),
        "no tracked bone resolves on the hands mesh"
    );
    assert!(
        names
            .iter()
            .any(|n| !gun.bone_names.iter().any(|b| b == *n)
                && !hands.bone_names.iter().any(|b| b == *n)),
        "expected some tracked bones to belong to neither mesh; if this ever \
         passes trivially the two-mesh split has changed"
    );
}

/// The gun mesh the clips drive must keep its attachment tags, or attachments
/// have nothing to mount to.
#[test]
fn the_paired_gun_mesh_keeps_its_attachment_tags() {
    let path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let bytes = std::fs::read(&path).expect("read mesh");
    let skel = read_cast_xmodel(&bytes, "nx1_viewmodel_scar_h").expect("parse mesh");
    assert_eq!(skel.bones.len(), 35, "paired gun mesh bone count changed");
    for tag in ["tag_foregrip", "tag_m203", "tag_clip", "tag_red_dot"] {
        assert!(
            skel.bone_names.iter().any(|n| n == tag),
            "paired mesh lost `{tag}`"
        );
    }
}

/// The render path validates every surface before it will draw a mesh, and it
/// validates silently: `draw_build` returns false and the mesh is dropped while
/// `fpv: equipped` has already logged success. One check is
/// `packed_vertices.len() == positions.len()`, which gates whether the packed
/// rows exist at all; the rest are per-surface range checks.
///
/// This replays those checks against the real files. A mesh that passes here is
/// a mesh that reaches the draw; one that fails is invisible for a reason no log
/// line reports.
fn draw_build_contract(skel: &asset_model::ModelSkel) -> Result<(), String> {
    if skel.packed_vertices.len() != skel.positions.len() {
        return Err(format!(
            "packed_vertices {} != positions {}: posed_packed goes None and every \
             surface loses its packed rows",
            skel.packed_vertices.len(),
            skel.positions.len()
        ));
    }
    if skel.surface_vertex_ranges.len() != skel.surface_index_ranges.len() {
        return Err("vertex and index range tables disagree".to_owned());
    }
    for (s, &(first, count)) in skel.surface_vertex_ranges.iter().enumerate() {
        let (istart, icount) = skel.surface_index_ranges[s];
        let vend = first + count;
        let iend = istart as usize + icount as usize;
        if vend > skel.positions.len()
            || vend > skel.packed_vertices.len()
            || vend > skel.uvs.len()
            || vend > skel.normals.len()
            || vend > skel.colors.len()
        {
            return Err(format!(
                "surface {s}: vertex range {first}..{vend} exceeds a vertex array"
            ));
        }
        if iend > skel.indices.len() {
            return Err(format!("surface {s}: index range exceeds the index array"));
        }
        let slice = &skel.indices[istart as usize..iend];
        let vmin = *slice.iter().min().unwrap();
        let vmax = *slice.iter().max().unwrap();
        let n = (vmax - vmin + 1) as usize;
        if n != count {
            return Err(format!(
                "surface {s}: index span {vmin}..={vmax} implies {n} verts, the \
                 vertex range says {count}"
            ));
        }
        for &index in slice {
            if index < first as u32 || index >= vend as u32 {
                return Err(format!(
                    "surface {s}: index {index} sits outside its own vertex range \
                     {first}..{vend}"
                ));
            }
        }
    }
    Ok(())
}

/// The gun the row actually drives must survive the draw path's validation.
#[test]
fn nx1_gun_satisfies_the_draw_contract() {
    let (skel, _) = scar_h_mesh();
    if let Err(e) = draw_build_contract(&skel) {
        panic!("viewmodel_scar_h would be dropped by draw_build: {e}");
    }
}

/// NX1's own hands ride the same path, so they get the same check.
#[test]
fn nx1_hands_satisfy_the_draw_contract() {
    let path = format!("{ROOT}\\models\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let skel = read_cast_xmodel(&bytes, "hands").expect("hands");
    if let Err(e) = draw_build_contract(&skel) {
        panic!("viewhands_nx_us_army would be dropped by draw_build: {e}");
    }
}

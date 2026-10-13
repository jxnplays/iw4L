//! The NX1 SCAR Mod 2's hide tags, against NX1's real rig.
//!
//! `hide_words` matches the weapon row's tags against the *mesh's* bone names, so
//! it is rig-correct — but the tags themselves come from the row, and the donor IW4
//! `scar` row is a bare rifle whose ten tags all name bones NX1 happens to have. The
//! donor's list hid 28 of NX1's 35 bones and 29 of the gun's 34 surfaces.
//!
//! These tests read the real bone names out of the dump and check the consequence
//! directly, so the set cannot silently grow back into "hide everything".

const ROOT: &str = r"E:\Call of Duty Future Warfare\NX1\output\one-gun\tools\saluki\exported_files\nx1";

/// The bones NX1's gun actually has, read from the cast.
fn gun_skel() -> asset_model::ModelSkel {
    let path = format!("{ROOT}\\models\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
    let (skel, _) = asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, "gun")
        .expect("parse the gun cast");
    skel
}

fn gun_bones() -> Vec<String> {
    gun_skel().bone_names
}

/// How many bones a tag set actually hides.
///
/// This has to use the real `bone_has_hidden_ancestor`, not a flat name match:
/// hiding a tag hides its whole subtree, which is why ten donor tags took out 28 of
/// 35 bones. Counting names would have "proved" the donor set was harmless.
fn bones_hidden(skel: &asset_model::ModelSkel, tags: &[&str]) -> usize {
    hidden_names(skel, tags).len()
}

fn hidden_names(skel: &asset_model::ModelSkel, tags: &[&str]) -> Vec<String> {
    let owned: Vec<String> = tags.iter().map(|t| (*t).to_owned()).collect();
    (0..skel.bone_names.len())
        .filter(|bone| {
            asset_game::bone_has_hidden_ancestor(
                &skel.bone_names,
                |b| skel.parent_of(b),
                *bone,
                &owned,
            )
        })
        .map(|b| skel.bone_names[b].clone())
        .collect()
}

fn kept_names(skel: &asset_model::ModelSkel, tags: &[&str]) -> Vec<String> {
    let hidden = hidden_names(skel, tags);
    skel.bone_names
        .iter()
        .filter(|n| !hidden.contains(n))
        .cloned()
        .collect()
}

/// The donor's tags, as measured from the donor row before NX1 had its own.
const DONOR_TAGS: [&str; 10] = [
    "tag_foregrip",
    "tag_silencer",
    "tag_red_dot",
    "tag_thermal_scope",
    "tag_sight_off",
    "tag_acog_2",
    "tag_m203",
    "tag_shotgun",
    "tag_eotech",
    "tag_heartbeat",
];

/// Every donor tag matches a real NX1 bone. That is the whole problem: the list is
/// not wrong in the abstract, it is wrong *for this mesh*.
#[test]
fn the_donor_tags_all_match_this_rig() {
    let bones = gun_bones();
    let matched: Vec<&str> = DONOR_TAGS
        .iter()
        .copied()
        .filter(|tag| bones.iter().any(|b| b == tag))
        .collect();
    assert_eq!(
        matched.len(),
        DONOR_TAGS.len(),
        "expected every donor tag to match an NX1 bone; matched only {matched:?}"
    );
}

/// NX1's set must hide strictly less than the donor's, and must keep the parts
/// that make the gun look like a gun.
///
/// The count alone is not the contract: 23 hidden bones is *correct* here, because
/// every one of them belongs to an attachment we deliberately excluded (four
/// duplicate optics, the shotgun, the silencer, the thermal, the motion tracker).
/// What matters is which bones survive.
#[test]
fn nx1_keeps_the_gun_and_hides_only_unwanted_attachments() {
    let skel = gun_skel();
    let donor = bones_hidden(&skel, &DONOR_TAGS);
    let nx1 = bones_hidden(&skel, &asset_game::NX1_SCAR2_HIDE_TAGS);
    let kept = kept_names(&skel, &asset_game::NX1_SCAR2_HIDE_TAGS);
    let hidden = hidden_names(&skel, &asset_game::NX1_SCAR2_HIDE_TAGS);

    println!("of {} bones: donor hides {donor}, nx1 hides {nx1}", skel.bone_names.len());
    println!("NX1 KEEPS: {kept:?}");
    println!("NX1 HIDES: {hidden:?}");

    assert!(nx1 < donor, "NX1 must hide fewer bones than the donor ({donor})");

    // The gun's own bones must survive. The donor's list took these with it, which
    // is why the gun drew as bare panels.
    for core in ["j_gun", "j_reload", "j_front_ring"] {
        assert!(kept.iter().any(|b| b == core), "{core} must stay visible");
    }

    // The chosen attachments survive with their moving parts.
    for wanted in [
        "tag_m203",
        "j_grenade_m203",
        "j_slider_m203",
        "tag_red_dot",
        "tag_foregrip",
    ] {
        assert!(kept.iter().any(|b| b == wanted), "{wanted} must stay visible");
    }

    // And everything hidden belongs to an attachment we excluded. Anything else is
    // geometry silently lost.
    let allowed: Vec<&str> = vec![
        "tag_acog_2",
        "tag_reticle_acog",
        "tag_eotech",
        "tag_eotech_reticle",
        "tag_heartbeat",
        "tag_motion_tracker",
        "tag_screen_bl",
        "tag_screen_br",
        "tag_screen_tl",
        "tag_screen_tr",
        "j_motion_tracker_roty",
        "j_motion_tracker_rotz",
        "tag_thermal_scope",
        "tag_reticle_thermal_scope",
        "tag_sight_off",
        "tag_shotgun",
        "j_ammo_shotgun",
        "j_plate_shotgun",
        "j_pump_shotgun",
        "j_reload_shotgun",
        "tag_silencer",
        "tag_flash_silenced",
        "tag_reticle_red_dot",
    ];
    let unexpected: Vec<&String> = hidden
        .iter()
        .filter(|b| !allowed.contains(&b.as_str()))
        .collect();
    assert!(
        unexpected.is_empty(),
        "these bones are hidden but belong to no excluded attachment: {unexpected:?}"
    );
}

/// The attachments a believable SCAR Mod 2 carries must stay visible. This is the
/// part that decides whether the gun looks like a rifle or like a bare frame.
#[test]
fn the_red_dot_m203_and_foregrip_stay_visible() {
    for keep in ["tag_red_dot", "tag_m203", "tag_foregrip"] {
        assert!(
            !asset_game::NX1_SCAR2_HIDE_TAGS.contains(&keep),
            "{keep} must stay visible: it is the optic, the launcher and the grip"
        );
        assert!(
            gun_bones().iter().any(|b| b == keep),
            "expected NX1's rig to carry {keep}"
        );
    }
}

/// The duplicate optics must go. Five optics on one rifle is the giveaway that a
/// mesh has every attachment baked in.
#[test]
fn the_duplicate_optics_are_hidden() {
    for hide in [
        "tag_acog_2",
        "tag_eotech",
        "tag_heartbeat",
        "tag_thermal_scope",
    ] {
        assert!(
            asset_game::NX1_SCAR2_HIDE_TAGS.contains(&hide),
            "{hide} is a duplicate optic and must be hidden"
        );
    }
}

/// A tag NX1's rig does not have is dead weight and hides nothing; it will mislead
/// whoever reads the set next.
#[test]
fn every_tag_exists_on_this_rig() {
    let bones = gun_bones();
    let dead: Vec<&&str> = asset_game::NX1_SCAR2_HIDE_TAGS
        .iter()
        .filter(|tag| !bones.iter().any(|b| *b == **tag))
        .collect();
    assert!(dead.is_empty(), "hide tags that match no NX1 bone: {dead:?}");
}

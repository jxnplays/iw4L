use super::*;

static NEXT_PRODUCTS_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Internal model names for the NX1 SCAR Mod 2 override.
///
/// These are the names the weapon row carries, so they must match the keys
/// `insert_in` files the cast under. They are not IW4 asset names and exist
/// nowhere in a zone file; the only producer is this insert.
pub(crate) const NX1_VIEWMODEL_NAME: &str = "nx1_viewmodel_scar2";
pub(crate) const NX1_HANDS_NAME: &str = "nx1_viewhands_us_army";
pub(crate) const NX1_WORLD_NAME: &str = "nx1_weapon_scar2";

/// NX1's own SCAR-H viewmodel.
///
/// This is the gun mesh the NX1 clips were authored against, not
/// `viewmodel_scar2`. The NX1 `.cast` animations track a 35-bone rig whose names
/// (`j_gun`, `j_grenade_m203`, `j_ammo_shotgun`, and the arm bones) are a subset
/// of this mesh's, plus the 66-bone viewhands mesh. `viewmodel_scar2` is a
/// different weapon with 54 bones and almost no matching names, so the clips
/// cannot drive it.
pub(crate) const NX1_VIEWMODEL_GUN_NAME: &str = "nx1_viewmodel_scar_h";

const NX1_EXPORT_ROOT: &str =
    "E:\\Call of Duty Future Warfare\\NX1\\output\\one-gun\\tools\\saluki\\exported_files\\nx1\\models";

const NX1_ANIM_ROOT: &str =
    "E:\\Call of Duty Future Warfare\\NX1\\output\\one-gun\\tools\\saluki\\exported_files\\nx1\\animations";

/// Where the cast export's PNG maps live, beside the meshes.
const NX1_IMAGES_ROOT: &str =
    "E:\\Call of Duty Future Warfare\\NX1\\output\\one-gun\\tools\\saluki\\exported_files\\nx1\\images";

use super::nx1_textures::Nx1MaterialTextures;

/// NX1 viewmodel clips for the SCAR Mod 2, published under `AssetNamespace::Nx1`.
///
/// These are the clip names the row's `sz_xanims` name. They collide by name with
/// the IW4 zone's own SCAR-H clips, which is why they are published under Nx1 and
/// the row's animation route points there: the namespace, not the name, decides
/// which one binds.
///
/// Every `viewmodel_scar_h_*.cast` in the dump is published, not just the ones the
/// row currently names, so an attachment variant or a later slot needs no new
/// export path.
fn insert_nx1_xanims(xanims: &mut asset_anim::XAnimBuild, report: &mut Vec<String>) {
    let namespace = asset_core::AssetNamespace::Nx1;
    let Ok(entries) = std::fs::read_dir(NX1_ANIM_ROOT) else {
        report.push(format!(
            "nx1 cast anims: cannot read {NX1_ANIM_ROOT}; NX1 clips stay unbound"
        ));
        return;
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("viewmodel_scar_h_") && name.ends_with(".cast"))
        .collect();
    names.sort();
    let mut inserted = 0usize;
    let mut refused: Vec<String> = Vec::new();
    for file in names {
        let stem = file.trim_end_matches(".cast").to_owned();
        let path = format!("{NX1_ANIM_ROOT}\\{file}");
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                refused.push(format!("{stem}: open {error}"));
                continue;
            }
        };
        match asset_model::cast_xmodel::read_cast_xanim(&bytes, &stem) {
            Ok(clip) => {
                xanims.insert_decoded(namespace, &stem, clip);
                inserted += 1;
            }
            Err(error) => refused.push(format!("{stem}: {error}")),
        }
    }
    report.push(format!(
        "nx1 cast anims: published {inserted} viewmodel_scar_h clips under nx1"
    ));
    for line in refused.iter().take(8) {
        report.push(format!("nx1 cast anims: refused {line}"));
    }
}

/// Insert the three NX1 cast meshes into the live builds, before publish.
///
/// `insert_in` is a method on the build, not on the published catalog, and the
/// build holds its catalog privately with no constructor from an existing one.
/// So this has to happen while the build is still live here, rather than by
/// extending an already-published `Arc`. That is the whole reason this sits in
/// the match walk: one line later `fpv_meshes.publish()` seals the catalog with
/// these entries in it, and `resolve_fpv_mesh_edges` binds the row's model edges
/// against them without either side being hand-wired.
///
/// Each name is inserted under the map's own namespace, because
/// `FpvHands::resolve` and `bind_fpv` both look the name up in the map
/// namespace and would miss an `Iw4`-namespaced entry.
///
/// Register one NX1 cast material and return its row.
///
/// A cast names its materials (`mc\mtl_weapon_scar_h` and the rest), but what
/// the renderer admits is an IW4 catalog row, and a surface's material edge
/// binds only when the row exists. A mesh read without this had correct
/// geometry, correct bones and correct animation and still drew nothing,
/// because every surface's material resolved to nothing and the renderer skips
/// an unmaterialed surface outright. That is the whole of the invisible gun.
fn nx1_placeholder_image(
    global: &mut asset_material::MaterialDefinitions,
    namespace: asset_core::AssetNamespace,
    name: &str,
    semantic: u8,
    srgb: bool,
) -> usize {
    if let Some(at) = global.image_index_by_key(namespace, name) {
        return at;
    }
    global.images.push(asset_material::AuthoredImage {
        namespace,
        name: asset_core::AssetRef::decode(name),
        map_type: 0,
        semantic,
        category: 0,
        use_srgb_reads: srgb,
        width: 0,
        height: 0,
        depth: 1,
        level_count: 1,
        format: 0,
        payload: std::sync::Arc::new(Vec::new()),
        decoded: None,
        common_owned: false,
        decoded_variant: None,
        decoded_by: None,
        pending_decode: None,
    });
    global.images.len() - 1
}

/// A material an already-rendering retail viewmodel uses.
///
/// Must both resolve (the FPV draw loop skips surfaces whose material edge is
/// absent) and admit (the render catalog requires a sorted ordinal and an
/// uploaded colour map). The surest source of one is a retail viewmodel gun that
/// is already on screen — so scan the FPV catalog for a mesh that already carries
/// a bound material key and reuse that exact authored row. Inventing a material
/// here cannot satisfy the second condition: a row written in this walk has no
/// decoded colour map (colour maps come from the zone image trees, which hold
/// none of the NX1 bytes) and its techset is not a compiled IW4 techset.
fn rendering_donor_material(
    fpv: &asset_model::FpvMeshBuild,
    global: &asset_material::MaterialDefinitions,
) -> Option<(usize, String)> {
    for index in 0..fpv.len() {
        let entry = fpv.get_at(index)?;
        let Some(Some(key)) = entry.material_keys.first() else {
            continue;
        };
        if key.name.is_empty() {
            continue;
        }
        if let Some(authored) = global.material_index_by_key(key) {
            return Some((authored.order(), key.name.clone()));
        }
    }
    None
}

fn register_nx1_material(
    global: &mut asset_material::MaterialDefinitions,
    namespace: asset_core::AssetNamespace,
    material: &str,
) -> usize {
    const TECHSET: &str = "mc_l_sm_r0c0d0n0s0p0";
    if let Some(index) = global.material_index_by_ns(namespace, material) {
        return index.order();
    }
    // The colour and specular images are placeholders with empty payloads: the
    // cast's own textures are PNGs and this catalog admits DXT, so they are
    // wired in a later pass. Until then a surface draws with its material's
    // shading and no texture — the line between an untextured gun and no gun.
    let color = nx1_placeholder_image(
        global,
        namespace,
        &format!("{material}_col"),
        asset_material::TS_COLOR_MAP,
        true,
    );
    let specular = nx1_placeholder_image(
        global,
        namespace,
        &format!("{material}_spc"),
        asset_material::TS_SPECULAR_MAP,
        false,
    );
    let index = global.materials.len();
    global.materials.push(asset_material::AuthoredMaterial {
        name: asset_core::AssetRef::decode(material),
        namespace,
        technique_set: asset_core::AssetRef::decode(TECHSET),
        technique_set_edge: asset_core::AssetEdge::Absent,
        draw_surf: 0,
        sort_key: 0,
        info_game_flags: 0,
        texture_atlas: None,
        surface_type_bits: None,
        t5_layered_surface_types: None,
        state_flags: 0,
        camera_region: 0,
        state_bits: Vec::new(),
        state_bits_entry: None,
        t5_state_bits_entry: None,
        iw5_state_bits_entry: None,
        technique_table: None,
        route: None,
        textures: vec![
            asset_material::MaterialTextureBinding {
                name_hash: 0,
                name_start: 0,
                name_end: 0,
                sampler_state: 0,
                semantic: asset_material::TS_COLOR_MAP,
                image: Some(color),
            },
            asset_material::MaterialTextureBinding {
                name_hash: 0,
                name_start: 0,
                name_end: 0,
                sampler_state: 0,
                semantic: asset_material::TS_SPECULAR_MAP,
                image: Some(specular),
            },
        ],
        constants: Vec::new(),
        zone: asset_core::ZoneOwner::default(),
    });
    index
}

fn insert_nx1_cast_meshes(
    fpv: &mut asset_model::FpvMeshBuild,
    world: &mut asset_model::WorldWeaponBuild,
    defs: &mut asset_material::MaterialDefinitions,
    report: &mut Vec<String>,
) {
    let Some(ns) = fpv.map_namespace else {
        report.push("nx1 cast insert: the fpv catalog has no map namespace, nothing inserted".to_owned());
        return;
    };
    report.push(format!("nx1 cast insert: map namespace is {}", ns.as_str()));

    // Read one cast file, register every material its surfaces name, and stamp
    // the resulting catalog rows onto the skel so the mesh binds against them.
    // The reader hands back names, not rows, because it holds no catalog; the
    // row a material ends up in is only knowable here.
    let read_and_stamp = |label: &str,
                       name: &str,
                       path: String,
                       donor: Option<&(usize, String)>,
                       defs: &mut asset_material::MaterialDefinitions,
                       report: &mut Vec<String>| {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                report.push(format!("nx1 cast insert: {label} open failed: {path}: {error}"));
                return None;
            }
        };
        let (mut skel, mats) =
            match asset_model::cast_xmodel::read_cast_xmodel_materials(&bytes, name) {
                Ok(parsed) => parsed,
                Err(error) => {
                    report.push(format!("nx1 cast insert: {label} parse failed: {path}: {error}"));
                    return None;
                }
            };
        let mut bound = 0usize;
        // The cast names each surface's material, and `register_nx1_material`
        // writes a real row for every one of them — but a row invented here has
        // no decoded colour map (material colour maps are decoded out of the
        // zone image trees, and these bytes are not in them), so it resolves and
        // then draws nothing.
        //
        // Rather than bind them and have every surface draw blank, the surfaces
        // are left unbound: `fpv_material`'s pass-material path then hands each
        // one a real material the session already holds, which is what puts the
        // gun on screen at all. The names are still reported so the mapping from
        // surface to cast material is not lost; feeding NX1's own PNG maps
        // through the image trees replaces this wholesale.
        // Bind every surface to a real, admitting material.
        //
        // A surface only reaches the draw when its material *edge* is bound:
        // the FPV draw loop skips any surface whose edge is absent. The rows
        // `register_nx1_material` writes cannot serve here — a material invented
        // in this walk has no decoded colour map (colour maps are decoded from
        // the zone image trees, which hold none of the NX1 bytes) and its
        // techset is not a compiled IW4 techset, so it never earns a sorted
        // ordinal and never admits.
        //
        // So the surfaces bind a material the session already holds — one with a
        // real texture and a real technique, which admits and draws. This puts
        // the gun on screen with a borrowed texture; wiring NX1's own PNG maps
        // through the image trees replaces the texture while keeping each
        // surface's own cast material.
        // Bind each surface to its OWN NX1 material, carrying NX1's own decoded
        // PNG textures.
        //
        // A surface draws only when its material edge is both bound and admitting.
        // A material row invented in this walk cannot admit on its own — a colour
        // map is normally decoded out of the zone image trees, which hold none of
        // the NX1 bytes, so the row has no texture and no sorted ordinal. So each
        // NX1 material is a clone of a real, admitting retail material (donor)
        // with its colour/normal/specular bindings repointed at NX1's own decoded
        // maps: real techset (admits) + real NX1 pixels (looks like a SCAR-H).
        // A material whose PNGs cannot be decoded falls back to the donor so the
        // surface still draws rather than vanishing.
        let donor_index = donor.map(|(authored, _)| *authored);
        let mut textured = 0usize;
        let named = mats.iter().flatten().count();
        skel.surface_materials = mats
            .iter()
            .map(|mat| {
                let Some(mat) = mat else { return None };
                let textures = Nx1MaterialTextures {
                    specular: mat.textures.specular.clone(),
                    normal: mat.textures.normal.clone(),
                    albedo: mat.textures.albedo.clone(),
                    detail: mat.textures.detail.clone(),
                };
                let authored = donor_index.and_then(|donor| {
                    nx1_textures::register_nx1_textured_material(
                        defs,
                        ns,
                        &mat.name,
                        &textures,
                        donor,
                        std::path::Path::new(NX1_IMAGES_ROOT),
                    )
                });
                if authored.is_some() {
                    textured += 1;
                }
                Some(asset_core::WalkLocalMaterialIndex::from_walk(
                    authored.or(donor_index)?,
                ))
            })
            .collect();
        report.push(format!(
            "nx1 cast insert: {label} ok name={name} bones={} surfaces={} cast_materials={named} textured={textured} donor={donor_index:?}",
            skel.bones.len(),
            skel.surface_index_ranges.len()
        ));
        Some(skel)
    };

    let donor = rendering_donor_material(fpv, defs);
    if let Some((_, name)) = &donor {
        report.push(format!("nx1 donor material: reusing `{name}`"));
    }
    for (label, name, path) in [
        (
            "viewmodel",
            NX1_VIEWMODEL_NAME,
            format!("{NX1_EXPORT_ROOT}\\viewmodel_scar2\\viewmodel_scar2_LOD0.cast"),
        ),
        // The gun mesh the NX1 clips actually drive. `viewmodel_scar2` is kept
        // inserted because the world model and any future direct reference
        // still name it, but the row's `gun_xmodel` points here.
        (
            "viewmodel_scar_h",
            NX1_VIEWMODEL_GUN_NAME,
            format!("{NX1_EXPORT_ROOT}\\viewmodel_scar_h\\viewmodel_scar_h_LOD0.cast"),
        ),
        (
            "hands",
            NX1_HANDS_NAME,
            format!("{NX1_EXPORT_ROOT}\\viewhands_nx_us_army\\viewhands_nx_us_army_LOD0.cast"),
        ),
    ] {
        if let Some(skel) = read_and_stamp(label, name, path, donor.as_ref(), defs, report) {
            fpv.insert_in_defs(ns, skel, defs);
        }
    }

    let world_path = format!("{NX1_EXPORT_ROOT}\\weapon_scar2\\weapon_scar2_LOD0.cast");
    if let Some(skel) = read_and_stamp("world", NX1_WORLD_NAME, world_path, donor.as_ref(), defs, report) {
        world.insert_in_defs(ns, skel, defs);
    }
}

pub(super) async fn walk_prepared_match(
    zone_ff: Result<PathBuf, String>,
    common_mp: Result<PathBuf, String>,
    progress: LoadProgress,
) -> (MatchLoadOutcome, Option<Arc<CommonSet>>) {
    let mut image_trees = games_root_from_env()
        .map(|root| asset_transport::NamespaceTrees::discover(&root))
        .unwrap_or_default();
    if let Ok(path) = &common_mp {
        image_trees.adopt_zone(path);
    }
    if let Ok(path) = &zone_ff {
        image_trees.adopt_zone(path);
    }
    let zone_name = zone_ff
        .as_ref()
        .ok()
        .and_then(|path| path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();

    let pool = load_pool();
    let map_open = {
        let zone_ff = zone_ff.clone();
        let progress = progress.clone();
        pool.spawn(async move {
            let path = match zone_ff {
                Ok(path) => path,
                Err(error) => {
                    progress.record_skipped_scoped(StageId::MapAssets, "open");
                    return Err(format!("zone not found: {error}"));
                }
            };
            let stage = progress.begin_scoped(StageId::MapAssets, "open", None);
            progress.begin_zone_open();
            let opened = open_zone_shared(&path);
            finish_zone_open(stage, &opened);
            let image = opened.map_err(|error| format!("open zone: {error}"))?;
            progress.record_zone_image_bytes(image.bytes.len());
            Ok::<_, String>((path, image))
        })
    };

    let mut donor_report = Vec::new();
    let key = CommonKey::for_match(
        zone_ff.as_ref().ok().map(PathBuf::as_path),
        common_mp.as_ref().ok().map(PathBuf::as_path),
        &mut donor_report,
    );
    let waiting = progress.begin_scoped(StageId::CommonAssets, "shared common", None);
    let (common, reach) = ensure_common(key).await;
    waiting.done();
    if progress.is_canceled() {
        diag::info!(
            World,
            "match walk: canceled while the common set was prepared — load retargeted"
        );
        return (MatchLoadOutcome::Canceled, None);
    }

    let cloning = std::time::Instant::now();
    let CommonProducts {
        scripts: mut iw4_scripts,
        mut t5_scene_models,
        material_seed,
        shared_surfaces,
        scene_models: mut common_scene_models,
        light_defs: common_light_defs,
        pen_table: common_pen_table,
        pen_table_loaded: common_pen_loaded,
        lochit_table: common_lochit_table,
        tracers: mut common_tracers,
        xmodel_walk,
        s1_common_bytes,
        teamsets,
        film_visions: common_film_visions,
        mut weapons,
        mut fpv_meshes,
        mut world_weapons,
        mut projectile_meshes,
        mut xanims,
        mut player_anim_sources,
        fx: common_fx,
        fx_models: common_fx_models,
        impact_fx: common_impact,
        t5_xanims,
        t5_fx,
        t5_impact_fx,
        iw5_materials,
        mut iw5_scene_models,
        iw5_shared_surfaces,
        strings,
        counts:
            CommonCounts {
                startup_count,
                t5_mat_count,
                t5_reuse_mat,
                t5_xanim_n,
                foreign_count,
                seed_mat,
                seed_img,
                common_reuse_mat,
                common_reuse_img,
            },
        report: mut common_report,
        localize_report,
    } = common.products.clone();
    let clone_ms = cloning.elapsed().as_secs_f32() * 1000.0;
    let iw5_mat_n = iw5_materials.materials.len();
    common_report.append(&mut donor_report);
    common_report.push(format!(
        "common set: {reach} for {} (prepared in {:.0}ms, {:.1}s ago); match copy {clone_ms:.0}ms; {} donor image batches and {} kept payloads ({:.1}MiB) shared, not decoded again",
        common.key,
        common.prepared_ms,
        common.ready_at.elapsed().as_secs_f32(),
        common.donor_batches(),
        common.retained_payloads(),
        common.retained_bytes() as f64 / (1024.0 * 1024.0),
    ));
    let common_images = hold_image_plan(
        "common_mp FPV",
        common.fpv_plan.clone(),
        load_jobs::open(JobKind::ImageDecode).namespace("iw4"),
    );

    let opened_map = map_open.await;
    if progress.is_canceled() {
        drop(opened_map);
        diag::info!(
            World,
            "match walk: canceled before the map walk — load retargeted"
        );
        return (MatchLoadOutcome::Canceled, None);
    }
    let (loaded, map_namespace) = match opened_map {
        Ok((path, image)) => {
            let game = image.game;
            let shared_surfaces = match game {
                asset_core::ZoneGame::Iw5 => iw5_shared_surfaces,
                _ => shared_surfaces,
            };
            (
                lane(game).load_world(
                    &path,
                    &image,
                    &progress,
                    shared_surfaces,
                    material_seed,
                    &common_film_visions,
                ),
                Some(asset_core::AssetNamespace::from_zone_game(game)),
            )
        }
        Err(gap) => {
            return (MatchLoadOutcome::Refused(gap), Some(common));
        }
    };

    let Some(map_family) = map_namespace else {
        return (
            MatchLoadOutcome::Refused("map family missing".into()),
            Some(common),
        );
    };
    let LoadedWorld {
        scripts: map_scripts,
        mut world,
        mut materials,
        collision: clip,
        spawns: dm_spawns,
        mut bodies,
        fpv_meshes: map_fpv,
        xanims: map_xanims,
        mut facts,
        arena_bytes: s1_map_bytes,
        sound,
        mut report,
        gaps,
    } = loaded;
    world.source_namespace = map_namespace;
    report.append(&mut common_report);
    report.extend(image_trees.report_lines());

    if facts.team_settings.allies.is_none() && facts.team_settings.axis.is_none() {
        if let Some(name) = facts.t5_teamset.as_ref() {
            if let Some(icons) = teamsets.get(name) {
                facts.team_settings = icons.clone();
            }
        }
    }
    if facts.t5_teamset.is_some() {
        facts.script_sound.attackers = facts
            .script_sound
            .attackers
            .or_else(|| facts.team_settings.attackers.clone());
        facts.script_sound.defenders = facts
            .script_sound
            .defenders
            .or_else(|| facts.team_settings.defenders.clone());
    }
    if map_namespace == Some(asset_core::AssetNamespace::T5) {
        facts.objective_visuals = asset_game::ObjectiveVisuals::t5(
            facts.team_settings.allies_charset.as_deref(),
            facts.team_settings.axis_charset.as_deref(),
        );
    }
    match (
        facts.t5_teamset.as_deref(),
        facts.team_settings.allies.as_ref(),
        facts.team_settings.axis.as_ref(),
    ) {
        (Some(ts), Some(a), Some(x)) => {
            report.push(format!("team icons: teamset={ts} allies={a} axis={x}"));
        }
        (Some(ts), ..) => {
            report.push(format!(
                "team icons gap: teamset={ts} but common_mp had no matching _teamset_*.gsc icons"
            ));
        }
        _ => {}
    }

    if matches!(
        map_namespace,
        Some(
            asset_core::AssetNamespace::T5
                | asset_core::AssetNamespace::Iw5
                | asset_core::AssetNamespace::T6
        )
    ) && !foreign_faction_rows(&mut iw4_scripts, &facts.team_settings)
    {
        report.push(format!(
            "team rows gap: {FACTION_TABLE} lacks {IW4_ALLIES_CHARSET}/{IW4_AXIS_CHARSET}; IW4 scripts show IW4 teams"
        ));
    }
    let map_scripts = match map_namespace {
        Some(asset_core::AssetNamespace::T5 | asset_core::AssetNamespace::T6) => {
            let scripts = t5_map_under_iw4_rules(&map_scripts, &zone_name, &facts);
            report.push(format!(
                "map script: maps/mp/{zone_name} written from the T5 map's declarations; T5 map scripts left out"
            ));
            scripts
        }
        _ => map_scripts,
    };
    let mut scripts = iw4_scripts;
    scripts.overlay(map_scripts);
    scripts.insert_source(
        "iw4l_t6/equipment",
        crate::map_scripts::T6_EQUIPMENT.to_owned(),
    );
    report.push(format!(
        "GSC source assets: {} (map overrides common_mp)",
        scripts.len()
    ));

    report.push(format!(
        "map teams: allies={:?} axis={:?} attackers={:?} defenders={:?}",
        facts.team_settings.allies_name,
        facts.team_settings.axis_name,
        facts.script_sound.attackers,
        facts.script_sound.defenders,
    ));

    report.push(format!(
        "s1 pool walked: common={s1_common_bytes} map={s1_map_bytes} total={} rss={}",
        s1_common_bytes.saturating_add(s1_map_bytes),
        asset_transport::process_resident_bytes().unwrap_or(0),
    ));

    let common_xanim_count = xanims.len();
    let map_xanim_count = map_xanims.len();
    let t5_xanim_added = xanims.absorb(t5_xanims);
    xanims.absorb_local(map_xanims);
    // Before the edges resolve: the NX1 clips have to be in the catalog for the
    // scar2 row's animation route to bind them rather than the identically-named
    // IW4 zone clips.
    insert_nx1_xanims(&mut xanims, &mut report);
    weapons.resolve_sz_xanim_edges(&xanims);
    let weapon_clip_indices = weapons.bound_weapon_xanim_indices();
    let clip_prewarm_started = std::time::Instant::now();
    let failed_weapon_clips: Vec<_> = weapon_clip_indices
        .iter()
        .copied()
        .filter(|&index| xanims.clip_at(index).is_none())
        .collect();
    report.push(format!(
        "weapon XAnim CPU prewarm: linked={} decoded={} failed={} elapsed_ms={:.1}",
        weapon_clip_indices.len(),
        weapon_clip_indices.len() - failed_weapon_clips.len(),
        failed_weapon_clips.len(),
        clip_prewarm_started.elapsed().as_secs_f64() * 1000.0,
    ));
    for index in failed_weapon_clips.iter().take(16) {
        report.push(format!(
            "weapon XAnim decode gap: index={index} name={}",
            xanims.name_at(*index).unwrap_or("<unknown>")
        ));
    }
    let t6_alt_raises = weapons.time_t6_alternate_raises(&xanims);
    report.push(format!(
        "T6 alternate raises timed by their clips: {t6_alt_raises}"
    ));
    let (note_actions, inline_note_actions) = weapons.resolve_notetrack_actions(&xanims);
    report.push(format!(
        "weapon notetrack actions linked: {note_actions} ({inline_note_actions} T5 inline)"
    ));
    let sz_xanims = weapons.sz_xanim_edge_census();
    report.push(format!(
        "XAnim generation: common={common_xanim_count} t5={t5_xanim_n} t5_keys={t5_xanim_added} collide={} map={map_xanim_count} merged={}",
        xanims.collide_name_count(),
        xanims.len()
    ));
    report.push(format!(
        "weapon szXAnims after absorb: bound={} unresolved={} absent={}",
        sz_xanims.bound, sz_xanims.unresolved, sz_xanims.absent,
    ));
    player_anim_sources.bind_leaves(&xanims);
    report.push(player_anim_sources.bind_report_line());
    let gap_lines: Vec<String> = gaps
        .iter()
        .map(|gap| match gap.addr {
            Some(addr) => format!("lane gap [{addr}]: {}", gap.reason),
            None => format!("lane gap: {}", gap.reason),
        })
        .collect();
    report.extend(gap_lines.iter().cloned());
    report.extend(bodies.report_lines());
    let map_fpv_n = map_fpv.len();
    let map_fpv_added = fpv_meshes.absorb(map_fpv);
    fpv_meshes.set_map_namespace(map_namespace);

    world_weapons.seal_identity();
    weapons.resolve_world_model_edges(&world_weapons);
    let world_model_edges = weapons.world_model_edge_census();
    report.push(format!(
        "world gun generation: catalog={}; worldModel edges bound={} unresolved={} absent={}",
        world_weapons.len(),
        world_model_edges.bound,
        world_model_edges.unresolved,
        world_model_edges.absent,
    ));
    report.push(format!(
        "FPV map fanout: map={map_fpv_n} added={map_fpv_added} merged={} map_ns={map_namespace:?}",
        fpv_meshes.len()
    ));

    let before_unrouted = materials.unrouted_material_count();
    let promoted = materials.promote_iw5_fallback_tables();
    let t5_alias_map = materials.absorb_t5_feature_token_donors();
    let stub_routed = materials.reroute_stub_materials();
    let after_unrouted = materials.unrouted_material_count();
    let takes_ml = materials
        .materials
        .iter()
        .filter(|m| materials.takes_model_lighting(m) == Some(true))
        .count();
    let t5_fb = materials
        .technique_set_facts()
        .iter()
        .filter(|facts| facts.t5_fallback_table.is_some())
        .count();
    report.push(format!(
        "material route: iw5_promoted={promoted} \
             t5_tech_alias={t5_alias_map} t5_fallback={t5_fb} stub_routed={stub_routed} \
             unrouted {before_unrouted}→{after_unrouted}; takes_model_lighting={takes_ml}"
    ));

    let map_reuse_mat = materials.link_reused_materials;
    let map_reuse_img = materials.link_reused_images;
    let pool_mat = materials.materials.len();
    let pool_img = materials.images.len();
    report.push(format!(
            "s2 pool: seed_mat={seed_mat} seed_img={seed_img} common_reuse_mat={common_reuse_mat} common_reuse_img={common_reuse_img} map_reuse_mat={map_reuse_mat} map_reuse_img={map_reuse_img} pool_mat={pool_mat} pool_img={pool_img}"
        ));
    let map_new = pool_mat.saturating_sub(seed_mat);

    let mut global = materials;
    let iw5_linked = global.absorb_asset_population_host_materials_win(iw5_materials);
    let provisional_map_ids: Vec<usize> = (0..global.materials.len()).collect();
    if iw5_mat_n > 0 {
        iw5_scene_models.remap_walk_materials(&iw5_linked);
    }
    let mut scene_names = scripts.asset_names();
    scene_names.extend(facts.objective_visuals.model_names().map(str::to_owned));
    common_scene_models.retain_names(&scene_names);
    t5_scene_models.retain_names(&scene_names);
    iw5_scene_models.retain_names(&scene_names);
    match map_namespace {
        Some(asset_core::AssetNamespace::T5) => world
            .map_xmodel_scene_assets
            .absorb_captured(t5_scene_models),
        Some(asset_core::AssetNamespace::Iw5) => world
            .map_xmodel_scene_assets
            .absorb_captured(iw5_scene_models),
        _ => {}
    }
    world
        .map_xmodel_scene_assets
        .absorb_captured(common_scene_models);
    report.push(format!(
        "iw5 leftover materials: donor={iw5_mat_n} linked={} pool={}",
        iw5_linked.len(),
        global.materials.len(),
    ));
    let promoted = global.promote_iw5_fallback_tables();
    let t5_alias = global.absorb_t5_feature_token_donors();
    global.reroute_stub_materials();
    let mat_refs = global.material_ref_census();
    let img_refs = global.image_ref_census();
    let shader_refs = global.shader_ref_census();
    let decl_refs = global.vertex_decl_ref_census();
    report.push(format!(
            "asset_ref before finalize: materials n={} real={} reference={}; images n={} real={} reference={}; shaders n={} real={} reference={}; decls n={} real={} reference={}",
            mat_refs.n, mat_refs.real, mat_refs.reference,
            img_refs.n, img_refs.real, img_refs.reference,
            shader_refs.n, shader_refs.real, shader_refs.reference,
            decl_refs.n, decl_refs.real, decl_refs.reference,
        ));
    let (mut global, finalized_ids) = global.publish();
    let map_ids = provisional_map_ids
        .into_iter()
        .map(|id| finalized_ids.get(id).copied().flatten())
        .collect::<Vec<_>>();
    let t5_fb = global
        .technique_set_facts()
        .iter()
        .filter(|facts| facts.t5_fallback_table.is_some())
        .count();
    report.push(format!(
            "material generation: startup={startup_count} t5={t5_mat_count} t5_reuse={t5_reuse_mat} foreign={foreign_count} common_pool={seed_mat} map_new={map_new} pooled={pool_mat} global={} common_reuse={common_reuse_mat} map_reuse={map_reuse_mat} unresolved_aliases={} iw5_promoted={promoted} t5_tech_alias={t5_alias} t5_fallback={t5_fb}",
            global.materials.len(),
            finalized_ids.iter().filter(|id| id.is_none()).count(),
        ));
    let mat_refs = global.material_ref_census();
    let img_refs = global.image_ref_census();
    let shader_refs = global.shader_ref_census();
    let decl_refs = global.vertex_decl_ref_census();
    report.push(format!(
            "asset_ref after finalize: materials n={} real={} reference={}; images n={} real={} reference={}; shaders n={} real={} reference={}; decls n={} real={} reference={}",
            mat_refs.n, mat_refs.real, mat_refs.reference,
            img_refs.n, img_refs.real, img_refs.reference,
            shader_refs.n, shader_refs.real, shader_refs.reference,
            decl_refs.n, decl_refs.real, decl_refs.reference,
        ));

    let global_memory = global.image_memory();
    report.push(global_memory.report_row("image memory global generation"));
    report.push(format!(
        "canonical materials after absorb: n={} images={} decoded={}",
        global.materials.len(),
        global_memory.images,
        global_memory.decoded_images,
    ));
    let shader_census = global.shader_source_census();
    report.push(format!(
        "shader source corpus: programs={} unresolved_aliases={} byteless={}",
        shader_census.programs, shader_census.unresolved_aliases, shader_census.byteless,
    ));
    let decl_streams = global.vertex_decl_stream_census();
    report.push(format!(
        "vertex decls: n={} stream0={} ppcc0t0t0nn n={} streams={}",
        decl_streams.n,
        decl_streams.stream0,
        decl_streams.ppcc_n,
        decl_streams
            .ppcc_stream_count
            .map(|n| n.to_string())
            .unwrap_or_else(|| "missing".into()),
    ));

    // The donors first, and only then the plan that claims the same names
    // they do. Its claims are resolved against the catalog they leave behind
    // rather than against the one it was built from, which is why it waited.
    for kept in common.donor_images().await {
        let job = load_jobs::open(JobKind::ImageDecode)
            .namespace(kept.namespace)
            .canonical(kept.label)
            .depends_on(kept.job)
            .plan_ready()
            .enqueued()
            .started()
            .finished()
            .joined();
        merge_image_batch(
            &mut global,
            kept.label,
            kept.batch.share(),
            job,
            &mut report,
        );
    }
    if let Some(held) = common_images
        && let Some(pending) = held.prune_then_enqueue(&mut global, &progress, &mut report)
    {
        let job = pending.job;
        let (label, batch) = pending.join().await;
        let kept = common.retain(&batch);
        report.push(format!(
            "{label} payloads kept for the next map: +{:.1}MiB (common set holds {} payloads, {:.1}MiB)",
            kept as f64 / (1024.0 * 1024.0),
            common.retained_payloads(),
            common.retained_bytes() as f64 / (1024.0 * 1024.0),
        ));
        merge_image_batch(&mut global, label, batch, job, &mut report);
    }
    if zone_ff.is_ok() {
        let stage = progress.begin_scoped(StageId::Images, "merged", None);
        let decoded = asset_material::decode_material_color_maps(
            &image_trees,
            &mut global,
            &stage,
            load_pool(),
        );
        stage.finish_from(&decoded);
        match decoded {
            Ok(stats) => {
                report.push(format!(
                    "merged material images: {}/{} decoded, {} missing, {} unsupported",
                    stats.decoded, stats.requested, stats.missing, stats.unsupported
                ));
                if let Some(gap) = stats.first_gap {
                    report.push(format!("merged material image missing: {gap}"));
                }
                if let Some(gap) = stats.first_unsupported {
                    report.push(format!("merged material image unsupported: {gap}"));
                }
            }
            Err(error) => report.push(format!("merged material images gap: {error}")),
        }
    }
    if let Some(draw) = world.draw.as_mut() {
        world.dynamic_light = resolve_world_lights(
            draw,
            &mut global,
            &common_light_defs,
            map_namespace,
            &image_trees,
            &progress,
            &mut report,
        );
    }
    let builtins = asset_material::decode_in_zone_builtin_images(&mut global);
    if builtins != 0 {
        report.push(format!(
            "in-zone builtin images: decoded {builtins} leftover $ 2D loadDefs after absorb"
        ));
    }
    if zone_ff.is_ok() {
        let stage = progress.begin_scoped(StageId::Images, "tracers", None);
        let decoded = asset_material::material_images::decode_images_for_keys(
            &image_trees,
            &mut global,
            common_tracers.material_keys(),
            &stage,
            load_pool(),
        );
        stage.finish_from(&decoded);
        match decoded {
            Ok(n) => report.push(format!("tracer beam images after absorb: {n} decoded")),
            Err(error) => report.push(format!("tracer beam images after absorb: {error}")),
        }
    }
    let unique: std::collections::BTreeSet<String> = common_tracers
        .named_materials()
        .map(str::to_owned)
        .collect();
    for name in &unique {
        let bind = asset_game::material_bind_name(name);
        let twins: Vec<&str> = global
            .materials
            .iter()
            .filter(|m| m.name.as_str() == bind)
            .map(|m| m.name.as_str())
            .collect();
        report.push(format!("tracer material `{name}` global twins={twins:?}"));
    }
    report.push(format!(
        "tracer color maps after absorb: Bound into global ({} unique names; no clone sidecar)",
        unique.len()
    ));

    world.fx.absorb(common_fx);
    let common_fx_model_n = common_fx_models.len();
    let map_fx_model_n = world.fx_models.len();
    let common_fx_model_added = world.fx_models.absorb(common_fx_models);
    let leftover_t5_fx_n = t5_fx.len();
    let leftover_t5_fx_gaps = t5_fx.capture_gaps;
    world.fx.absorb_missing(t5_fx);
    report.push(format!(
        "leftover t5 fx absorb_missing: donor={leftover_t5_fx_n} gaps={leftover_t5_fx_gaps} host now {}",
        world.fx.len()
    ));
    let fx_model_walked_n = world.fx_models.len();
    world.fx_models.keep_referenced(&world.fx.model_hints());
    world.fx.resolve_model_edges(&world.fx_models);
    let fx_model_edges = world.fx.model_edge_census();
    report.push(format!(
        "FX model generation: map={map_fx_model_n} common={common_fx_model_n} added={common_fx_model_added} walked={fx_model_walked_n} retained={} edges bound={} unresolved={} absent={}",
        world.fx_models.len(),
        fx_model_edges.bound,
        fx_model_edges.unresolved,
        fx_model_edges.absent,
    ));
    weapons.resolve_combat_fx(&world.fx, &common_tracers);
    weapons.resolve_projectile_fx_edges(&world.fx);
    let projectile_fx = weapons.projectile_fx_edge_census();
    report.push(format!(
        "weapon projectile FX after absorb: bound={} unresolved={} absent={}",
        projectile_fx.bound, projectile_fx.unresolved, projectile_fx.absent,
    ));
    link_materials_after_absorb(
        &global,
        &mut world,
        &mut weapons,
        &mut common_tracers,
        &mut bodies,
        &mut world_weapons,
        &mut fpv_meshes,
        &mut projectile_meshes,
        &mut report,
    );
    // Bind rigs and tracks to the finished mesh publication, after material linking.
    insert_nx1_cast_meshes(&mut fpv_meshes, &mut world_weapons, &mut global, &mut report);
    // Stamp the NX1 meshes' material edges now that they are in the catalog.
    //
    // `link_materials_after_absorb` (above) already ran `resolve_materials` over
    // every mesh present at that moment, but the NX1 meshes are inserted after
    // it. So they keep the `Unresolved` edges they were captured with and never
    // bind, and a viewmodel surface only draws when its edge is both bound and
    // admitting — which is why the gun composes, admits its geometry, and still
    // renders nothing. Re-run the same pass now that they are present.
    fpv_meshes.resolve_materials(&global);
    let fpv_meshes = Arc::new(fpv_meshes.publish());
    // The match-walk registry is separate from the common set the front menu
    // reads, so `scar2` has to be registered in both. The idempotence guard
    // makes this one registration per registry, not a duplicate: it returns
    // early if the row is already present.
    match weapons.override_nx1_scar2(
        NX1_VIEWMODEL_GUN_NAME,
        NX1_HANDS_NAME,
        NX1_WORLD_NAME,
    ) {
        Some(index) => report.push(format!("nx1: scar2 row {index} registered in the match registry")),
        None => report.push(
            "nx1: scar2 not registered in the match registry (already present, or no iw4 `scar` donor)"
                .to_owned(),
        ),
    }
    // The row's animation edges have to be resolved *after* it is created.
    //
    // `resolve_sz_xanim_edges` ran much earlier in this walk, over the rows the
    // capture produced. The scar2 row is added afterwards, so its `sz_xanims`
    // were never bound and the view reported `no szXAnims[IDLE]`. Re-running the
    // pass here binds the NX1 clips the row now names. It is a full rebuild, so
    // the rows resolved earlier are simply resolved again to the same answer.
    weapons.resolve_sz_xanim_edges(&xanims);
    weapons.resolve_fpv_mesh_edges(&fpv_meshes);
    let bodies = Arc::new(bodies.publish());
    let xanims = Arc::new(xanims.publish());
    let soldiers = asset_game::SoldierPresentations::prepare(
        &bodies,
        &fpv_meshes,
        &player_anim_sources,
        &xanims,
    );
    weapons.resolve_fpv_soldiers(&fpv_meshes, &soldiers);
    let assembly_started = std::time::Instant::now();
    let assemblies = weapons.resolve_fpv_assemblies(&fpv_meshes, &xanims);
    report.push(format!(
        "FPV assemblies: built={} kit sides linked={} refused={} clip track tables={} elapsed_ms={:.1}",
        assemblies.built,
        assemblies.linked,
        assemblies.refused,
        assemblies.clip_tables,
        assembly_started.elapsed().as_secs_f64() * 1000.0,
    ));
    let dependency_gaps = weapons.dependency_gaps();
    let selectable_gap_ids: std::collections::BTreeSet<_> = dependency_gaps
        .iter()
        .map(|gap| gap.id)
        .filter(|&id| weapons.describe_configuration(id).is_some())
        .collect();
    report.push(format!(
        "weapon dependency audit: {} gaps in {} definitions; selectable={}",
        dependency_gaps.len(),
        dependency_gaps
            .iter()
            .map(|gap| gap.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        selectable_gap_ids.len(),
    ));
    for gap in &dependency_gaps {
        report.push(format!(
            "weapon dependency gap: {} {} `{}`",
            weapons.name_of(gap.id),
            gap.kind,
            gap.name
        ));
    }
    report.push(
        "fx color maps handoff: n=0 bytes=0 (Bound GPU bind at spawn; no CPU clone sidecar; stub_aliases=0)"
            .into(),
    );
    decode_fx_colour_maps(&world.fx, &mut global, &image_trees, &progress, &mut report);
    if world.impact_fx.is_none() {
        world.impact_fx = if map_namespace == Some(asset_core::AssetNamespace::T5) {
            t5_impact_fx.or(common_impact)
        } else {
            common_impact
        };
    } else if let Some(common_table) = common_impact {
        report.push(format!(
            "impactfx: map table kept; common_mp table `{}` discarded",
            common_table.name
        ));
    }
    if let Some(ref table) = world.impact_fx {
        report.push(format!(
            "impactfx handoff: {:?}:`{}` rows={} (fx catalog now {} defs)",
            table.namespace(),
            table.name,
            table.row_count(),
            world.fx.len()
        ));
    } else {
        report.push("impactfx handoff: missing — combat play_oriented will miss cells".into());
    }
    report.push(format!(
        "tracer catalog handoff: {} named ({} bound, {} unresolved)",
        common_tracers.len(),
        common_tracers.bound_count(),
        common_tracers.unresolved_count()
    ));

    let prepared_map = PreparedMap {
        zone: zone_name,
        namespace: map_namespace,
        spawns: dm_spawns,
        facts,
        gaps: PreparedGaps { lines: gap_lines },
    };
    if prepared_map.facts.minimap_corners.is_some() {
        report.push("compass: minimap_corner pair from MapEnts".into());
    } else {
        report.push("compass gap: minimap_corner missing — no world-to-map frame".into());
    }
    match prepared_map.facts.north_yaw {
        Some(yaw) => report.push(format!("compass: worldspawn northyaw {yaw}")),
        None => {
            report.push("compass gap: worldspawn has no northyaw — map is drawn north-up".into())
        }
    }

    report.extend(localize_report);
    let (directory_ms, opens, inflate_ms) = asset_material::iwd_read_cost();
    report.push(format!(
        "IWD read cost: central_dir={directory_ms:.0}ms over {opens} opens, inflate={inflate_ms:.0}ms (summed over worker threads, not wall)"
    ));
    let (mip_hit, mip_miss, mip_io_ms) = asset_material::mip_cache_cost();
    report.push(format!(
        "mip cache: hit={mip_hit} miss={mip_miss} io={mip_io_ms:.0}ms"
    ));
    let (payload_reads, header_reads) = asset_material::iwd_entry_reads();
    report.push(format!(
        "IWD entry reads: payload={payload_reads} header-only={header_reads} (a header answers whether an image is a cubemap; a payload read is the whole entry inflated)"
    ));
    if let Some(draw) = world.draw.as_ref() {
        census_image_working_set(
            draw,
            &world,
            &global,
            &map_ids,
            &fpv_meshes,
            &common_tracers,
            &mut report,
        );
    }
    let mut fx = std::mem::take(&mut world.fx).publish();
    fx.set_map_namespace(map_family);
    let destructible_death =
        crate::stamp_match_destructible_death(&xanims, &world.map_xmodel_scene_assets);
    for row in &destructible_death {
        report.push(format!(
            "destructible death {}: clip={} husk={}",
            row.kind,
            row.clip.edge_kind(),
            row.husk.edge_kind()
        ));
    }
    let prepared = PreparedMatch {
        scripts,
        fx,
        world,
        materials: crate::MatchMaterials {
            population: Arc::new(global),
            map_ids,
            common_profile_id: common.id,
            products_id: NEXT_PRODUCTS_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        },
        clip: clip.map(Arc::new),
        weapons: Arc::new(weapons.publish()),
        fpv_meshes,
        bodies,
        soldiers,
        world_weapons: world_weapons.publish(),
        projectile_meshes: projectile_meshes.publish(),
        xanims,
        destructible_death,
        player_anim_sources,
        tracers: common_tracers.publish(),
        strings,
        report,
        prepared_map,
        pen_table: common_pen_table,
        pen_table_loaded: common_pen_loaded,
        lochit_table: common_lochit_table,
        xmodel_walk,
        sound,
        sound_gaps: 0,
        script_sound_aliases: None,
    };
    (MatchLoadOutcome::Ready(prepared), Some(common))
}

fn resolve_world_lights(
    draw: &mut WorldDraw,
    global: &mut asset_material::MaterialDefinitions,
    common_light_defs: &[asset_world::CapturedLightDef],
    map_namespace: Option<asset_core::AssetNamespace>,
    image_trees: &asset_transport::NamespaceTrees,
    progress: &LoadProgress,
    report: &mut Vec<String>,
) -> Option<asset_world::ResolvedLightDef> {
    asset_world::resolve_primary_light_attenuation(draw, global, common_light_defs);
    let dynamic_light_name =
        (map_namespace == Some(asset_core::AssetNamespace::Iw4)).then_some("light_dynamic");
    let dynamic_light = dynamic_light_name.and_then(|name| {
        asset_world::resolve_named_light_def(name, &draw.light_defs, common_light_defs, global)
    });
    let (ordinal, source) = asset_world::resolve_outdoor_image(
        draw.outdoor_image_name.as_deref(),
        map_namespace?,
        global,
    );
    draw.outdoor_image = ordinal;
    if source == "$outdoor" && draw.outdoor_image_name.is_none() {
        draw.outdoor_image_name = Some("$outdoor".into());
    }
    report.push(format!(
        "outdoorImage: source={source} name={} global_ordinal={} lookup_m00={:.6e} lookup_m30={:.4}",
        draw.outdoor_image_name.as_deref().unwrap_or("-"),
        draw.outdoor_image
            .map(|i| i.to_string())
            .unwrap_or_else(|| "NONE".into()),
        f32::from_bits(draw.outdoor_lookup[0]),
        f32::from_bits(draw.outdoor_lookup[12]),
    ));
    let named = draw
        .primary_lights
        .iter()
        .filter(|light| light.def_name.as_ref().is_some_and(|name| !name.is_empty()))
        .count();
    let atten = draw
        .primary_lights
        .iter()
        .filter(|light| light.attenuation_image.is_some())
        .count();
    report.push(format!(
        "GfxLightDef resolve: common_defs={} map_defs={} named_lights={named} atten_image={atten} (light-def name → global catalog; missing stays None)",
        common_light_defs.len(),
        draw.light_defs.len(),
    ));
    report.push(format!(
        "GfxLightDef names: common={:?} map={:?} lights={:?} images_common={:?} images_map={:?}",
        common_light_defs
            .iter()
            .map(|def| def.name.as_str())
            .collect::<Vec<_>>(),
        draw.light_defs
            .iter()
            .map(|def| def.name.as_str())
            .collect::<Vec<_>>(),
        draw.primary_lights
            .iter()
            .filter_map(|light| light.def_name.as_deref())
            .collect::<Vec<_>>(),
        common_light_defs
            .iter()
            .map(|def| def.attenuation_image_name.as_deref())
            .collect::<Vec<_>>(),
        draw.light_defs
            .iter()
            .map(|def| def.attenuation_image_name.as_deref())
            .collect::<Vec<_>>(),
    ));
    {
        let mut requested: Vec<(usize, u8)> = draw
            .primary_lights
            .iter()
            .filter_map(|light| Some((light.attenuation_image?, light.attenuation_sampler)))
            .collect();
        if let Some(dynamic) = dynamic_light
            && let Some(image) = dynamic.attenuation_image
        {
            requested.push((image, dynamic.attenuation_sampler));
        }
        let want: std::collections::BTreeSet<usize> =
            requested.iter().map(|(index, _)| *index).collect();
        let want = want.len();
        let stage = progress.begin_scoped(StageId::Images, "attenuation", None);
        let decoded = asset_material::decode_catalog_images_from_iwd(
            image_trees,
            global,
            requested,
            &stage,
            load_pool(),
        );
        stage.finish_from(&decoded);
        match decoded {
            Ok(n) => report.push(format!(
                "IWD light attenuation: decoded {n} of {want} GfxLightDef images (empty payload is not a host ramp)"
            )),
            Err(error) => report.push(format!("IWD light attenuation: {error}")),
        }
    }

    asset_world::resolve_primary_light_attenuation(draw, global, common_light_defs);
    let resolved_dynamic = dynamic_light_name.and_then(|name| {
        asset_world::resolve_named_light_def(name, &draw.light_defs, common_light_defs, global)
    });
    let dynamic_decoded = resolved_dynamic
        .and_then(|light| light.attenuation_image)
        .is_some_and(|index| {
            global
                .images
                .get(index)
                .is_some_and(|image| image.decoded.is_some())
        });
    report.push(format!(
        "FX light_dynamic: image={:?} decoded={} width={:?} sampler={}",
        resolved_dynamic.and_then(|light| light.attenuation_image),
        dynamic_decoded,
        resolved_dynamic.and_then(|light| light.falloff_image_width),
        resolved_dynamic.map_or(0, |light| light.attenuation_sampler),
    ));
    let dynamic_light =
        resolved_dynamic.filter(|light| dynamic_decoded && light.falloff_image_width.is_some());
    if dynamic_light_name.is_some() && dynamic_light.is_none() {
        report.push(
            "FX light_dynamic GAP: light definition or decoded attenuation image missing; additional FX lights unavailable"
                .into(),
        );
    }
    dynamic_light
}

#[allow(clippy::too_many_arguments)]
fn link_materials_after_absorb(
    materials: &asset_material::MaterialDefinitions,
    world: &mut PreparedWorld,
    weapons: &mut WeaponBuild,
    tracers: &mut asset_game::TracerCatalog,
    bodies: &mut asset_model::BodyMeshBuild,
    world_weapons: &mut WorldWeaponBuild,
    fpv_meshes: &mut FpvMeshBuild,
    projectile_meshes: &mut ProjectileMeshBuild,
    report: &mut Vec<String>,
) {
    let fx_model_materials = world.fx_models.resolve_materials(materials);
    report.push(format!(
        "FX model materialHandles after absorb: bound={} unresolved={} absent={}",
        fx_model_materials.bound, fx_model_materials.unresolved, fx_model_materials.absent,
    ));
    weapons.resolve_hud_material_edges(materials);
    if let Some(glass) = world.fx_glass.as_mut() {
        glass.resolve_material_edges(materials);
        let census = glass.material_edge_census();
        report.push(format!(
            "glass Material* after absorb: bound={} unresolved={} absent={}",
            census.bound, census.unresolved, census.absent
        ));
    }
    let hud_materials = weapons.hud_material_edge_census();
    let technique_sets = materials.technique_set_edge_census();
    report.push(format!(
        "material pointer graph after finalize: weapon_hud bound={} unresolved={} absent={}; technique_set bound={} unresolved={} absent={}",
        hud_materials.bound,
        hud_materials.unresolved,
        hud_materials.absent,
        technique_sets.bound,
        technique_sets.unresolved,
        technique_sets.absent,
    ));
    let graph = crate::resolve_after_absorb(
        materials,
        tracers,
        &mut world.fx,
        None,
        Some(bodies),
        Some(world_weapons),
        None,
        Some(fpv_meshes),
        Some(projectile_meshes),
    );
    report.push(format!(
        "asset graph after absorb: tracer_mat bound={} unresolved={} absent={}; fx_elem bound={} unresolved={} absent={}; fx_child bound={} unresolved={} absent={}; fx_runner bound={} unresolved={} absent={}; body materialHandles bound={} unresolved={} absent={}; world-gun materialHandles bound={} unresolved={} absent={}; FPV materialHandles bound={} unresolved={} absent={} (WeaponDef.tracerType stamped at common_mp walk)",
        graph.tracer_materials.bound,
        graph.tracer_materials.unresolved,
        graph.tracer_materials.absent,
        graph.fx_elem_materials.bound,
        graph.fx_elem_materials.unresolved,
        graph.fx_elem_materials.absent,
        graph.fx_nested_children.bound,
        graph.fx_nested_children.unresolved,
        graph.fx_nested_children.absent,
        graph.fx_runner_children.bound,
        graph.fx_runner_children.unresolved,
        graph.fx_runner_children.absent,
        graph.xmodel_body_materials.bound,
        graph.xmodel_body_materials.unresolved,
        graph.xmodel_body_materials.absent,
        graph.xmodel_gun_materials.bound,
        graph.xmodel_gun_materials.unresolved,
        graph.xmodel_gun_materials.absent,
        graph.xmodel_fpv_materials.bound,
        graph.xmodel_fpv_materials.unresolved,
        graph.xmodel_fpv_materials.absent,
    ));
    report.push(format!(
        "Material stamp: iw4={} t5={} iw5={} (name-link identity; colliding T5 names are the later-zone row)",
        materials.namespace_count(asset_core::AssetNamespace::Iw4),
        materials.namespace_count(asset_core::AssetNamespace::T5),
        materials.namespace_count(asset_core::AssetNamespace::Iw5),
    ));
    report.push(format!(
        "fx elem material edges after absorb: {} bound ({} unique), {} unresolved (temp={}, catalog_miss={}), {} absent of {} material visuals; {} decal mark arms ({} Bound slots, {} unresolved, {} temp, {} array-unpatched)",
        world.fx.material_visual_bound_count(),
        world.fx.material_visual_unique_bound_count(),
        world.fx.material_visual_unresolved_count(),
        world.fx.material_visual_unresolved_temp_count(),
        world.fx.material_visual_unresolved_miss_count(),
        world.fx.material_visual_absent_count(),
        world.fx.material_visual_count(),
        world.fx.material_visual_decal_count(),
        world.fx.decal_mark_bound_slot_count(),
        world.fx.decal_mark_unresolved_slot_count(),
        world.fx.decal_mark_temp_slot_count(),
        world.fx.material_visual_decal_unpatched_count()
    ));
    report.push(format!(
        "fx elem decal unique Bound after absorb: {} (mc={}, wc={}); decoded color/2D in catalog {} of {} (not sprite Plan)",
        world.fx.unique_decal_mark_count(),
        world.fx.unique_decal_mark_mc_count(),
        world.fx.unique_decal_mark_wc_count(),
        world.fx.unique_decal_mark_decoded_color_count(materials),
        world.fx.unique_decal_mark_count()
    ));
    let miss = world
        .fx
        .unique_decal_mark_decoded_miss_samples(materials, 8);
    if !miss.is_empty() {
        report.push(format!(
            "fx elem decal catalog nocolor sample: {}",
            miss.join("; ")
        ));
    }
    let samples = world.fx.unresolved_material_samples(8);
    if !samples.is_empty() {
        report.push(format!(
            "fx elem unresolved Material* sample: {}",
            samples.join("; ")
        ));
    }
    let mark_samples = world.fx.decal_mark_samples(8);
    if !mark_samples.is_empty() {
        report.push(format!(
            "fx elem decal mark sample: {}",
            mark_samples.join("; ")
        ));
    }
}

fn decode_fx_colour_maps(
    fx: &FxCatalog,
    global: &mut asset_material::MaterialDefinitions,
    image_trees: &asset_transport::NamespaceTrees,
    progress: &LoadProgress,
    report: &mut Vec<String>,
) {
    let missing: Vec<asset_core::MaterialKey> = fx
        .unique_bound_hints()
        .into_iter()
        .chain(fx.unique_decal_mark_hints())
        .filter(|(index, _)| !asset_game::color_decoded_in_catalog(global, *index))
        .filter_map(|(index, _)| {
            let material = global.materials.get(index)?;
            Some(asset_core::MaterialKey {
                namespace: material.namespace,
                name: material.name.as_str().to_owned(),
            })
        })
        .collect();
    if !missing.is_empty() {
        let stage = progress.begin_scoped(StageId::Images, "fx_elem", None);
        let decoded = asset_material::material_images::decode_images_for_keys(
            image_trees,
            global,
            missing,
            &stage,
            load_pool(),
        );
        stage.finish_from(&decoded);
        match decoded {
            Ok(n) => report.push(format!("fx elem 2d images after absorb: {n} decoded")),
            Err(error) => report.push(format!("fx elem 2d images after absorb: {error}")),
        }
    }
    let nocolor: Vec<(usize, String)> = fx
        .unique_bound_hints()
        .into_iter()
        .filter(|(index, _)| !asset_game::color_decoded_in_catalog(global, *index))
        .map(|(index, hint)| (index, hint.to_owned()))
        .collect();
    if !nocolor.is_empty() {
        let (distortion, other): (Vec<_>, Vec<_>) = nocolor
            .into_iter()
            .partition(|(_, hint)| hint.contains("distortion"));
        report.push(format!(
            "fx elem Bound without decoded color: {} of {} unique (distortion={}, other={}) other_sample: {}",
            distortion.len() + other.len(),
            fx.material_visual_unique_bound_count(),
            distortion.len(),
            other.len(),
            if other.is_empty() {
                "-".to_string()
            } else {
                other
                    .iter()
                    .map(|(index, hint)| format!("{index}:{hint}"))
                    .collect::<Vec<_>>()
                    .join("; ")
            }
        ));
        for (index, hint) in &other {
            let Some(mat) = global.materials.get(*index) else {
                report.push(format!(
                    "fx elem Bound `{index}:{hint}` has no global material row"
                ));
                continue;
            };
            let sem: Vec<u8> = mat.textures.iter().map(|t| t.semantic).collect();
            let decoded = mat.textures.iter().any(|t| {
                t.image
                    .and_then(|i| global.images.get(i))
                    .is_some_and(|img| img.decoded.is_some())
            });
            report.push(format!(
                "fx elem Bound `{index}:{}` techset={} camera_region={} tex={} sem={sem:?} decoded={decoded}",
                mat.name,
                mat.technique_set,
                mat.camera_region,
                mat.textures.len(),
            ));
        }
    }
}

fn census_image_working_set(
    draw: &WorldDraw,
    world: &PreparedWorld,
    global: &asset_material::MaterialDefinitions,
    map_ids: &[Option<usize>],
    fpv_meshes: &asset_model::FpvMeshCatalog,
    tracers: &asset_game::TracerCatalog,
    report: &mut Vec<String>,
) {
    let world_mats = draw.batches.iter().filter_map(|batch| {
        let local = batch.material?;
        map_ids.get(local).copied().flatten().or(Some(local))
    });

    let smodel_mats = world.static_model_meshes.iter().flat_map(|mesh| {
        mesh.lod_surfaces.iter().flatten().filter_map(|surface| {
            let local = surface.material?;
            map_ids.get(local).copied().flatten()
        })
    });
    let fpv_mats = fpv_meshes.bound_material_indices();
    let fx_mats = world
        .fx
        .unique_bound_hints()
        .into_iter()
        .map(|(index, _)| index)
        .chain(
            world
                .fx
                .unique_decal_mark_hints()
                .into_iter()
                .map(|(index, _)| index),
        )
        .chain(tracers.defs().filter_map(|def| def.material.bound_index()));
    let mut set = asset_material::material_images::census_image_working_set(
        global,
        world_mats,
        smodel_mats,
        fpv_mats,
        fx_mats,
    );
    let (probe_n, probe_bytes) = asset_material::material_images::cpu_image_census(
        world.reflection_probe_images.iter().flatten(),
    );
    let (lightmap_n, lightmap_bytes) = match &draw.lightmap {
        Ok(pages) => asset_material::material_images::cpu_image_census(
            pages.iter().flatten().flat_map(|page| {
                [
                    page.primary_image.as_ref(),
                    page.secondary_image.as_ref(),
                    Some(&page.ambient_image),
                    Some(&page.directional_image),
                    Some(&page.sun_mask_image),
                ]
                .into_iter()
                .flatten()
            }),
        ),
        Err(_) => (0, 0),
    };
    set.probe_n = probe_n;
    set.probe_bytes = probe_bytes;
    set.lightmap_n = lightmap_n;
    set.lightmap_bytes = lightmap_bytes;
    asset_material::material_images::store_image_working_set(set);
    report.push(format!(
        "image working set: decoded={} ({:.1}MiB) world-batch={} ({:.1}MiB) smodel={} ({:.1}MiB) fpv={} ({:.1}MiB) probe={} ({:.1}MiB) lightmap={} ({:.1}MiB) fx={} ({:.1}MiB); subsets, not a skipped load",
        set.decoded_n,
        set.decoded_bytes as f64 / (1024.0 * 1024.0),
        set.world_n,
        set.world_bytes as f64 / (1024.0 * 1024.0),
        set.smodel_n,
        set.smodel_bytes as f64 / (1024.0 * 1024.0),
        set.fpv_n,
        set.fpv_bytes as f64 / (1024.0 * 1024.0),
        set.probe_n,
        set.probe_bytes as f64 / (1024.0 * 1024.0),
        set.lightmap_n,
        set.lightmap_bytes as f64 / (1024.0 * 1024.0),
        set.fx_n,
        set.fx_bytes as f64 / (1024.0 * 1024.0),
    ));
}

const FACTION_TABLE: &str = "mp/factionTable.csv";
const IW4_ALLIES_CHARSET: &str = "us_army";
const IW4_AXIS_CHARSET: &str = "opforce_composite";

fn foreign_faction_rows(
    scripts: &mut crate::ScriptSources,
    teams: &asset_game::MapTeamSettings,
) -> bool {
    let allies = faction_cells(
        &teams.allies_strings,
        teams.allies_name.as_ref(),
        teams.allies.as_ref(),
        teams.allies_color,
    );
    let axis = faction_cells(
        &teams.axis_strings,
        teams.axis_name.as_ref(),
        teams.axis.as_ref(),
        teams.axis_color,
    );
    scripts.set_table_cells(FACTION_TABLE, IW4_ALLIES_CHARSET, &allies)
        & scripts.set_table_cells(FACTION_TABLE, IW4_AXIS_CHARSET, &axis)
}

fn faction_cells(
    strings: &asset_game::FactionStrings,
    short_name: Option<&asset_core::AssetKey>,
    icon: Option<&asset_core::AssetKey>,
    color: Option<[f32; 3]>,
) -> Vec<(usize, String)> {
    let mut cells: Vec<(usize, String)> = [
        (1, strings.name.clone()),
        (2, short_name.map(|k| k.logical_name().to_owned())),
        (3, strings.eliminated.clone()),
        (4, strings.forfeited.clone()),
        (5, icon.map(asset_core::AssetKey::display)),
    ]
    .into_iter()
    .filter_map(|(column, value)| Some((column, value?)))
    .collect();
    if let Some(rgb) = color {
        cells.extend((14..).zip(rgb.map(|c| c.to_string())));
    }
    cells
}

fn t5_map_under_iw4_rules(
    map_scripts: &crate::ScriptSources,
    zone_name: &str,
    facts: &crate::MapFacts,
) -> crate::ScriptSources {
    let module = format!("maps/mp/{zone_name}");
    let main = map_scripts
        .read(&module)
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    let mut declarations = asset_world::MapDeclarations::from_t5_map_main(&main);
    declarations.set_teams(
        facts.script_sound.attackers.as_deref().unwrap_or("allies"),
        facts.script_sound.defenders.as_deref().unwrap_or("axis"),
    );
    let entities = map_scripts
        .entities()
        .map(asset_world::t5_entities_for_iw4_rules)
        .unwrap_or_default();
    let mut scripts = crate::ScriptSources::default();
    let mut map_main = declarations.map_script(&entities);
    let end = map_main.rfind('}').expect("generated map main");
    map_main.insert_str(end, "\tthread iw4l_maps\\destructibles::main();\n");
    scripts.insert_source(
        "iw4l_maps/destructibles",
        crate::map_scripts::DESTRUCTIBLES.to_owned(),
    );
    if zone_name == "mp_radiation" {
        let end = map_main.rfind('}').expect("generated map main");
        map_main.insert_str(end, "\tthread iw4l_maps\\radiation::main();\n");
        scripts.insert_source(
            "iw4l_maps/radiation",
            crate::map_scripts::RADIATION.to_owned(),
        );
    }
    scripts.insert_source(&module, map_main);
    scripts.set_entities(entities);
    scripts
}

use std::sync::Arc;

use anim_iw4::{DOBJ_RADIUS_PARENT_ROOT, compute_bounds_radius};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use frame::{LifeFrontPublished, PresentedPublished, ViewSubject, WorkerCmdSet};
use math_iw4::vec3_length;
use net::{ClientSet, FrameClock, LocalPresentClient, PresentedSnapshot, ViewweaponAim};
use render_scene::{SCENE_VIEWMODEL_ENTNUM, SCENE_VIEWMODEL_FX_FLAGS, SCENE_VIEWMODEL_LEFT_ENTNUM};

use crate::anim::fpv::{
    AuthorityFpvCues, EquippedFpv, FpvAuthoritySample, is_predicted_fire_weap_anim,
    local_shot_identity,
};
use crate::anim::fpv_host::{FpvGenerateArgs, FpvPoseKind, FpvPoseRefuse, generate_fpv_pose};
use crate::anim::fpv_prepared::{
    FpvOwnerInputs, FpvWeaponSlot, FpvWeaponTable, FpvWeaponView, PreparedFpv,
};
use crate::anim::fpv_rig::PreparedFpvRig;
use crate::anim::scene_submission::{AnimDObjSceneSubmission, AnimSceneSubmit};
use crate::anim::viewmodel_controller::ViewmodelController;
use crate::gaps::{RenderGap, RenderGapCause, RenderPresentationGaps};
use crate::occupancy::remote_body::RemotePlayer;
use crate::occupancy::third_person::presented_is_third_person;
use crate::occupancy::view_kick::{
    GunOffset, PendingViewHurt, SessionViewKick, apply_cg_gun_offset_view,
    apply_viewweapon_land_view, iw_view_placement_to_bevy_camera_local,
    reset_view_kick_on_life_started, sync_camera_from_presented, tick_session_view_kick,
};
use crate::{fpv_dobj_skel_radii, viewmodel_lighting_origin};
use hud_iw4::{
    WeaponAdsOverlayFacts, calc_crosshair_position, get_weap_reticle_zoom, tan_half_fov,
    viewweapon_drawgun, viewweapon_drawgun_skip,
};
use math_iw4::angle_vectors;
use playerstate_iw4::PlayerState;
use render_material::RuntimeMaterialCatalog;
use render_scene::WorldScriptModelInstance;
use render_scene::{FlyCamera, FpvLens};
use render_scene::{HostGfxScene, scene_quat_from_viewmodel_axes};
use weapon_iw4::{
    GunRecoilResponse, PLACEMENT_ASSEMBLE_STEP_COUNT, StanceTransitionFadeGlobals, WeaponBobInputs,
    WeaponBobWaveformInputs, WeaponMovementKinematics, WeaponPlacementAssembleStep,
    WeaponPlacementPsInputs, WeaponPlacementState, WeaponStanceStaticOfsInputs,
    calculate_weapon_movement_bob_waveform, clip_table_key, dual_wield_view_model_origin_add,
    get_clip_for_hand, get_viewmodel_weapon_index, viewmodel_rocket_should_be_attached,
    viewweapon_iron_ads_saves_composed_axis, viewweapon_save_gun_pitch_yaw,
    viewweapon_view_to_world_delta, weapon_placement_assemble,
};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FpvPlacementSet;

/// The first-person vertices for this frame are written. The merge downstream
/// orders itself after this set.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FpvGeometrySet;

#[derive(Component)]
pub struct FpvViewmodel;

#[derive(Component)]
pub struct FpvPlacementRoot;

pub use crate::anim::fpv_host::{
    FpvBoltTargets, FpvHeldLife, FpvHeldSettled, FpvPoseProduct, FpvPresentCursor,
    PendingFpvNotetracks, PendingFpvSpawn, PendingFpvSpawnRequest,
};

#[derive(Resource, Default)]
pub struct SessionViewmodel(pub Option<SessionFpvMeshesHandles>);

pub struct SessionFpvMeshesHandles {
    pub weapon_id: u32,
    pub parent_weapon: u32,
    pub catalog_id: u64,
    pub axis: bool,
    pub view: Arc<FpvWeaponView>,
    pub(crate) table: Arc<FpvWeaponTable>,
    pub fpv: EquippedFpv,
    pub(crate) active_rig: Option<Arc<PreparedFpvRig>>,
    pub(crate) material_catalog: Arc<RuntimeMaterialCatalog>,
}

fn same_clips(a: &asset_game::WeaponAnimations, b: &asset_game::WeaponAnimations) -> bool {
    (0..asset_game::WEAPON_ANIM_SLOTS).all(|slot| match (a.clip_at(slot), b.clip_at(slot)) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    })
}

fn same_compositions(a: &asset_game::FpvSideAssemblies, b: &asset_game::FpvSideAssemblies) -> bool {
    Arc::ptr_eq(&a.bare, &b.bare)
        && match (&a.melee, &b.melee) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
        && match (&a.rocket, &b.rocket) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
        && match (&a.jammed, &b.jammed) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        }
}

fn same_material_catalog(
    owned: &Arc<RuntimeMaterialCatalog>,
    current: Option<&render_scene::TessMaterials>,
) -> bool {
    current.is_some_and(|current| Arc::ptr_eq(owned, &current.catalog()))
}

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct FpvStatusGap(pub Option<FpvState>);

#[derive(Clone, Debug, PartialEq)]
pub enum FpvState {
    ClearedNotAlive,

    ClearedNoWeapon,

    Queued,

    Drawn { idle_sampled: bool },

    Blocked(RenderGapCause),
}

impl FpvState {
    pub fn label(&self) -> &'static str {
        match self {
            FpvState::ClearedNotAlive => "not Alive — FPV cleared",
            FpvState::ClearedNoWeapon => "held weapon 0 — FPV cleared",
            FpvState::Queued => "held weapon changed; FPV queued (linked gunXModel)",
            FpvState::Drawn { idle_sampled: true } => {
                "spawned after Equip; idle-sampled eye-posed hands+gun; retained FPV + ModelLightingCache"
            }
            FpvState::Drawn {
                idle_sampled: false,
            } => {
                "spawned after Equip; bind-pose eye-posed hands+gun; idle sample gap; retained FPV"
            }
            FpvState::Blocked(cause) => cause.label(),
        }
    }

    pub fn cause(&self) -> Option<&RenderGapCause> {
        match self {
            FpvState::Blocked(cause) => Some(cause),
            _ => None,
        }
    }
}

impl core::fmt::Display for FpvState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FpvState::Blocked(cause) => write!(f, "{cause}"),
            other => f.write_str(other.label()),
        }
    }
}

pub(crate) fn fpv_viewmodel_weapon(ps: &PlayerState, weapons: &FpvWeaponTable) -> u32 {
    let viewmodel = get_viewmodel_weapon_index(ps);
    let stabbing = matches!(
        weapon_iw4::WeaponState::from_i32(ps.weaponstate_primary),
        Ok(weapon_iw4::WeaponState::MeleeInit | weapon_iw4::WeaponState::MeleeFire)
    );
    if stabbing && viewmodel == ps.weapon {
        weapons.melee_weapon_of(viewmodel)
    } else {
        viewmodel
    }
}

fn fpv_rocket_should_attach(
    weapons: &FpvWeaponTable,
    weapon_id: u32,
    ps: Option<&PlayerState>,
) -> bool {
    let Some(ps) = ps else {
        return true;
    };
    let Some(facts) = weapons.facts_of(weapon_id) else {
        return false;
    };
    let viewmodel = fpv_viewmodel_weapon(ps, weapons);
    let clip_key = clip_table_key(facts.clip_index, viewmodel);
    let clip = get_clip_for_hand(&ps.ammoclip, clip_key, 0);
    viewmodel_rocket_should_be_attached(
        clip,
        ps.weaponstate_primary,
        ps.weapon_time,
        facts.reload_time_ms,
        facts.reload_show_rocket_time_ms,
    )
}

#[derive(SystemParam)]
pub struct SpawnPendingFpvInputs<'w> {
    prepared: Res<'w, PreparedFpv>,
    owners: FpvOwnerInputs<'w>,
    tess: Res<'w, render_scene::TessMaterials>,
    presented: Res<'w, PresentedSnapshot>,
    local: Res<'w, LocalPresentClient>,
}

/// Catalog name of the NX1 viewmodel. The held view draws this when the
/// equipped class's payload base is `scar2`. The weapon row is not rewritten.
const NX1_HELD_VIEW: &str = "nx1_viewmodel_scar2";
const NX1_VIEW_BONES: usize = 54;
const NX1_VIEW_SURFACES: usize = 30;

fn class_payload_base_is_scar2(primary: Option<&str>) -> bool {
    primary
        .and_then(asset_game::FamilyKey::parse)
        .is_some_and(|key| key.base == "scar2")
}

/// The prepared view for the `scar2` row, if its gun resolved to the NX1 mesh.
///
/// This is the held view's mesh lookup. Row 591 keeps `viewmodel_m4`.
pub(crate) fn scar2_held_view(
    primary: Option<&str>,
    owners: &FpvOwnerInputs,
    table: &FpvWeaponTable,
    epoch: Option<u32>,
    parent: u32,
    axis: bool,
) -> Option<Arc<FpvWeaponView>> {
    if !class_payload_base_is_scar2(primary) {
        return None;
    }
    let _ = (epoch, parent, axis);
    let catalog = &owners.meshes.as_ref()?.0;
    let order = catalog.parsed_model_order(NX1_HELD_VIEW, NX1_VIEW_BONES, NX1_VIEW_SURFACES)?;
    // Name-only lookup can land on a slot whose surfaces are still the M4.
    // The posed view has to be the one whose gun index is the parsed cast.
    let view = table.view_for_order(order)?;
    let submitted = view.rigs.pick(false, false, false, false, false)?;
    let (bones, surfaces) = submitted.gun_mesh_shape()?;
    (bones == NX1_VIEW_BONES && surfaces == NX1_VIEW_SURFACES).then_some(view)
}

pub(crate) fn held_view_gun_index(
    primary: Option<&str>,
    owners: &FpvOwnerInputs,
    table: &FpvWeaponTable,
    epoch: Option<u32>,
    weapon: u32,
    parent: u32,
) -> Option<assets::FpvMeshIndex> {
    scar2_held_view(primary, owners, table, epoch, parent, false)
        .map(|view| view.gun_index)
        .or_else(|| table.gun_index(weapon))
}

pub fn spawn_pending_fpv(
    mut commands: Commands,
    mut pending: ResMut<PendingFpvSpawn>,
    inputs: SpawnPendingFpvInputs,
    mut session_vm: ResMut<SessionViewmodel>,
    mut cursor: ResMut<FpvPresentCursor>,
    mut fpv_plan: ResMut<crate::FpvDrawPlan>,
    cameras: Query<Entity, With<FlyCamera>>,
    existing_fpv: Query<Entity, With<FpvPlacementRoot>>,
    mut status: ResMut<FpvStatusGap>,
    gaps: Res<RenderPresentationGaps>,
) {
    let SpawnPendingFpvInputs {
        prepared,
        owners,
        tess,
        presented,
        local,
    } = inputs;
    let Some(request) = pending.0.take() else {
        return;
    };
    let Some(table) = prepared
        .table()
        .filter(|table| same_material_catalog(table.material_catalog(), Some(&*tess)))
    else {
        pending.0 = Some(request);
        return;
    };
    let Some(bound_table) = owners.bind(table) else {
        return;
    };
    if owners.handles(
        presented.weapon_epoch(),
        request.weapon_id,
        request.parent_weapon,
    ) != Some((request.weapon_handle, request.parent_handle))
    {
        return;
    }
    let instance_started = std::time::Instant::now();
    cursor.0.forget_weap_anim();
    for entity in &existing_fpv {
        commands.entity(entity).try_despawn();
    }
    session_vm.0 = None;

    let expected_gun = held_view_gun_index(
        owners
            .classes
            .as_ref()
            .and_then(|c| c.equipped_primary.as_deref()),
        &owners,
        table,
        presented.weapon_epoch(),
        request.weapon_id,
        request.parent_weapon,
    );
    if table.catalog_id() != request.catalog_id || expected_gun != Some(request.gun_index) {
        let cause = RenderGapCause::FpvGunXModelUnresolved {
            weapon_id: request.weapon_id,
        };
        gaps.raise(cause.clone());
        status.0 = Some(FpvState::Blocked(cause));
        return;
    }
    let meta = presented
        .snapshot()
        .and_then(|snap| snap.meta.for_client(local.0));
    let ffa_team = meta.and_then(|m| m.ffa_team);
    let client_state_team = meta.map(|m| m.client_state_team).unwrap_or(0);
    let axis = asset_model::kit_assignment_is_axis(client_state_team, ffa_team);
    let view = match bound_table.slot(request.weapon_handle, request.parent_handle, axis) {
        Ok(FpvWeaponSlot::Ready(view)) => Arc::clone(view),
        Ok(FpvWeaponSlot::Refused(cause)) => {
            gaps.raise(cause.clone());
            status.0 = Some(FpvState::Blocked(cause.clone()));
            return;
        }
        Ok(FpvWeaponSlot::Absent) | Err(_) => {
            let cause = RenderGapCause::FpvGunXModelUnresolved {
                weapon_id: request.weapon_id,
            };
            gaps.raise(cause.clone());
            status.0 = Some(FpvState::Blocked(cause));
            return;
        }
    };
    // Held view only. The weapon row and its table entry stay on viewmodel_m4.
    let view = scar2_held_view(
        owners
            .classes
            .as_ref()
            .and_then(|c| c.equipped_primary.as_deref()),
        &owners,
        table,
        presented.weapon_epoch(),
        request.parent_weapon,
        axis,
    )
    .unwrap_or(view);
    let Ok(host) = cameras.single() else {
        diag::info!(
            Fpv,
            "fpv: no FlyCamera to parent viewmodel under — re-queue"
        );
        pending.0 = Some(request);
        gaps.raise(RenderGapCause::FpvNoCamera);
        status.0 = Some(FpvState::Blocked(RenderGapCause::FpvNoCamera));
        return;
    };

    commands.entity(host).with_children(|parent| {
        parent.spawn((
            FpvViewmodel,
            FpvPlacementRoot,
            Transform::IDENTITY,
            Visibility::Visible,
        ));
    });

    crate::clear_fpv_draw_plan(&mut fpv_plan, 0);
    let census = &view.census;
    fpv_plan.gun_colormap_skip_n = Some(census.gun_colormap_skip_n);
    fpv_plan.gun_ordinal_skip_n = Some(0);
    fpv_plan.gun_colormap_skip_names = (!census.gun_colormap_skip_names.is_empty())
        .then(|| census.gun_colormap_skip_names.join(","));
    fpv_plan.plan_mat_hints = (!census.mat_hints.is_empty()).then(|| census.mat_hints.join(","));

    let controller = ViewmodelController::new(view.right.clone());
    let left = view.left.clone().map(ViewmodelController::new);
    diag::info!(
        Fpv,
        "fpv: viewmodel controller `{}` — {} clips{}, fireTime={}ms raiseTime={}ms (action from weapAnim)",
        view.right.name,
        view.right.resolved_count(),
        view.left
            .as_ref()
            .map(|left| format!(" R, {} clips L (dual DObj)", left.resolved_count()))
            .unwrap_or_else(String::new),
        view.right.fire_time_ms,
        view.right.raise_time_ms,
    );
    let idle_kind = match view.idle_name.as_deref() {
        Some(name) => format!("idle `{name}` (szXAnims[IDLE])"),
        None => "no szXAnims[IDLE] — guess forbidden".to_owned(),
    };
    let idle_sampled = view.idle_name.is_some();
    diag::info!(
        Fpv,
        "fpv: equipped `{}` ({}; prepared before Ready) hands={:?} weapon_family={} — instance {:.2}ms",
        view.gun_name,
        idle_kind,
        view.hands,
        view.namespace.as_str(),
        instance_started.elapsed().as_secs_f64() * 1000.0,
    );
    session_vm.0 = Some(SessionFpvMeshesHandles {
        weapon_id: request.weapon_id,
        parent_weapon: request.parent_weapon,
        catalog_id: request.catalog_id,
        table: Arc::clone(table),
        axis,
        fpv: EquippedFpv::new(
            view.gun_name.clone(),
            view.gun_index,
            view.hands_index,
            view.namespace,
            view.hands.clone(),
            controller,
            left,
        ),
        view,
        active_rig: None,
        material_catalog: Arc::clone(table.material_catalog()),
    });

    gaps.clear(RenderGap::FpvViewmodel);
    status.0 = Some(FpvState::Drawn { idle_sampled });
}

fn viewweapon_drawgun_admit(
    ps: &PlayerState,
    weapons: Option<&FpvWeaponTable>,
    b_position_to_ads: bool,
) -> Option<(bool, Option<&'static str>)> {
    let reg = weapons?;
    let viewmodel = fpv_viewmodel_weapon(ps, reg);
    let facts = reg.facts_of(viewmodel)?;

    let hud_iris = reg.overlay_is_hud_iris(viewmodel);
    let weap = WeaponAdsOverlayFacts {
        ads_zoom_in_frac: facts.ads_zoom_in_frac,
        ads_zoom_out_frac: facts.ads_zoom_out_frac,
        overlay_material: u32::from(hud_iris),
        overlay_reticle: facts.overlay_reticle,
        ads_overlay_width: facts.ads_overlay_width,
        ads_overlay_height: facts.ads_overlay_height,
        ..WeaponAdsOverlayFacts::default()
    };
    let iris = get_weap_reticle_zoom(ps.f_weapon_pos_frac, b_position_to_ads, &weap);
    Some((
        viewweapon_drawgun(false, true, iris),
        viewweapon_drawgun_skip(false, true, iris),
    ))
}

fn viewweapon_drawgun_value(
    ps: &PlayerState,
    weapons: Option<&FpvWeaponTable>,
    b_position_to_ads: bool,
) -> Option<i32> {
    let (admit, _skip) = viewweapon_drawgun_admit(ps, weapons, b_position_to_ads)?;
    Some(i32::from(admit))
}

fn fpv_occupy_submission(
    lighting: [f32; 3],
    radius: Option<f32>,
    entnum: u32,
    model_n: u8,
) -> AnimDObjSceneSubmission {
    AnimDObjSceneSubmission {
        render_fx_flags: SCENE_VIEWMODEL_FX_FLAGS,
        has_tree: true,
        origin: lighting,
        lighting_origin: lighting,
        radius,
        entnum,
        quat: None,
        occupy_model_n: model_n,
        models: Vec::new(),
        hide_part_bits: [0; 6],
        store_skin: false,
    }
}

pub fn occupy_fpv_scene(
    mut submissions: MessageWriter<AnimDObjSceneSubmission>,
    presented: Res<PresentedSnapshot>,
    local: Res<LocalPresentClient>,
    view_settings: (Res<ViewSubject>, Res<frame::GameSettings>),
    prepared: Res<PreparedFpv>,
    kick: Option<Res<SessionViewKick>>,
    session_vm: Option<Res<SessionViewmodel>>,
    tess: Option<Res<render_scene::TessMaterials>>,
    owners: FpvOwnerInputs,
) {
    let (view, settings) = view_settings;
    let fpv_meshes = owners.meshes.as_ref();
    if presented.viewweapon_player(local.0).is_none()
        || presented_is_third_person(
            &presented,
            local.0,
            view.in_killcam(),
            settings.third_person,
        )
    {
        return;
    }
    let Some(ps) = presented.viewweapon_player(local.0) else {
        return;
    };
    let admit = viewweapon_drawgun_admit(
        ps,
        prepared.table().map(|table| &**table),
        kick.as_ref().map(|k| k.b_position_to_ads).unwrap_or(true),
    );
    if !admit.is_some_and(|(ok, _)| ok) {
        return;
    }
    let Some(session) = session_vm.as_ref().and_then(|s| s.0.as_ref()) else {
        return;
    };
    if !same_material_catalog(&session.material_catalog, tess.as_deref())
        || owners.bind(&session.table).is_none()
    {
        return;
    }
    let lighting = viewmodel_lighting_origin(
        ps.origin,
        ps.view_height_current,
        ps.viewangles[1],
        ps.leanf,
    );
    let radius = fpv_meshes.as_ref().and_then(|cat| {
        let (hands, gun) =
            fpv_dobj_skel_radii(&cat.0, session.fpv.hands_index, session.fpv.gun_index);
        match (hands, gun) {
            (Some(h), Some(g)) => Some(compute_bounds_radius(
                &[h, g],
                &[DOBJ_RADIUS_PARENT_ROOT, 0],
            )),
            (Some(r), None) | (None, Some(r)) => Some(r),
            (None, None) => None,
        }
    });
    let model_n: u8 = if session.fpv.gun_xmodel.is_empty() {
        1
    } else {
        2
    };
    submissions.write(fpv_occupy_submission(
        lighting,
        radius,
        SCENE_VIEWMODEL_ENTNUM,
        model_n,
    ));
    if ps.last_weapon_hand == 1 {
        submissions.write(fpv_occupy_submission(
            lighting,
            radius,
            SCENE_VIEWMODEL_LEFT_ENTNUM,
            model_n,
        ));
    }
}

fn refuse_gap_cause(refuse: FpvPoseRefuse) -> RenderGapCause {
    match refuse {
        FpvPoseRefuse::CatalogMissing => RenderGapCause::FpvCatalogMissing,
        FpvPoseRefuse::NoActiveClips => RenderGapCause::FpvNoActiveClips,
        FpvPoseRefuse::EyePoseFailed { gun_xmodel } => {
            RenderGapCause::FpvEyePoseFailed { gun_xmodel }
        }
        FpvPoseRefuse::DependencyUnresolved {
            weapon_id,
            role,
            name,
        } => RenderGapCause::FpvDependencyUnresolved {
            weapon_id,
            role,
            name,
        },
    }
}

pub fn tick_fpv_viewmodel(
    time: Res<Time>,
    (presented, local, generation, events): (
        Res<PresentedSnapshot>,
        Res<LocalPresentClient>,
        Res<frame::WorldGeneration>,
        Res<net::EntityEventCursor>,
    ),
    mut cursor: ResMut<FpvPresentCursor>,
    prepared: Res<PreparedFpv>,
    owners: FpvOwnerInputs,
    mut session_vm: ResMut<SessionViewmodel>,
    tess: Option<Res<render_scene::TessMaterials>>,
    mut settled: ResMut<FpvHeldSettled>,
    mut pending: ResMut<PendingFpvSpawn>,
    mut product: ResMut<FpvPoseProduct>,
    mut pending_notes: ResMut<PendingFpvNotetracks>,
    mut bolts: ResMut<FpvBoltTargets>,
    kick: Option<Res<SessionViewKick>>,
    view_settings: (Res<ViewSubject>, Res<frame::GameSettings>),
) {
    let (view, settings) = view_settings;
    let fpv_meshes = owners.meshes.as_ref();
    let table = prepared.table().map(|table| &**table);
    pending_notes.batch = None;
    bolts.clear();
    *product = FpvPoseProduct::default();
    if let Some(ps) = presented.viewweapon_player(local.0) {
        if !presented_is_third_person(
            &presented,
            local.0,
            view.in_killcam(),
            settings.third_person,
        ) {
            product.drawgun = viewweapon_drawgun_value(
                ps,
                table,
                kick.as_ref().map(|k| k.b_position_to_ads).unwrap_or(true),
            );
        }
    }
    let Some(session) = session_vm.0.as_mut() else {
        product.kind = FpvPoseKind::Hide;
        return;
    };
    if !same_material_catalog(&session.material_catalog, tess.as_deref())
        || owners.bind(&session.table).is_none()
    {
        diag::warn!(
            Fpv,
            "fpv: material catalog changed; retiring stale rig and bindings"
        );
        session_vm.0 = None;
        settled.0 = None;
        pending.0 = None;
        product.kind = FpvPoseKind::Hide;
        return;
    }
    if fpv_meshes
        .as_ref()
        .is_none_or(|fpv| fpv.0.identity() != session.catalog_id)
    {
        session_vm.0 = None;
        settled.0 = None;
        pending.0 = None;
        product.kind = FpvPoseKind::Hide;
        return;
    }
    if presented_is_third_person(
        &presented,
        local.0,
        view.in_killcam(),
        settings.third_person,
    ) {
        product.kind = FpvPoseKind::Hide;
        return;
    }
    let Some(fpv) = fpv_meshes.as_ref() else {
        product.kind = FpvPoseKind::Refuse(FpvPoseRefuse::CatalogMissing);
        return;
    };

    if let Some(ps) = presented.viewweapon_player(local.0) {
        let weapon = table.map_or(get_viewmodel_weapon_index(ps), |t| {
            fpv_viewmodel_weapon(ps, t)
        });
        if weapon != 0
            && (weapon != session.weapon_id || ps.weapon_primary != session.parent_weapon)
        {
            // Do not write weapon 591's slot back over the scar2 view in this frame.
            if scar2_held_view(
                owners
                    .classes
                    .as_ref()
                    .and_then(|c| c.equipped_primary.as_deref()),
                &owners,
                table.unwrap_or(&session.table),
                presented.weapon_epoch(),
                ps.weapon_primary,
                session.axis,
            )
            .is_some()
            {
                session.weapon_id = weapon;
                session.parent_weapon = ps.weapon_primary;
            } else if let Some(table) = table {
                match held_view_gun_index(
                    owners
            .classes
            .as_ref()
            .and_then(|c| c.equipped_primary.as_deref()),
                    &owners,
                    table,
                    presented.weapon_epoch(),
                    weapon,
                    ps.weapon_primary,
                ) {
                    Some(gun_index) if gun_index == session.fpv.gun_index => {
                        if scar2_held_view(
                            owners
            .classes
            .as_ref()
            .and_then(|c| c.equipped_primary.as_deref()),
                            &owners,
                            table,
                            presented.weapon_epoch(),
                            ps.weapon_primary,
                            session.axis,
                        )
                        .is_some_and(|view| view.gun_index == gun_index)
                        {
                            session.weapon_id = weapon;
                            session.parent_weapon = ps.weapon_primary;
                        } else {
                        let handles =
                            owners.handles(presented.weapon_epoch(), weapon, ps.weapon_primary);
                        let next = owners.bind(table).and_then(|bound| {
                            handles.and_then(|(weapon, parent)| {
                                bound.slot(weapon, parent, session.axis).ok()
                            })
                        });
                        let same = match next {
                            Some(FpvWeaponSlot::Ready(next)) => {
                                same_compositions(&next.assemblies, &session.view.assemblies)
                                    && same_clips(&next.right, &session.view.right)
                                    && match (&next.left, &session.view.left) {
                                        (None, None) => true,
                                        (Some(a), Some(b)) => same_clips(a, b),
                                        _ => false,
                                    }
                            }
                            _ => false,
                        };
                        if same {
                            if let Some(FpvWeaponSlot::Ready(next)) = next {
                                session.view = Arc::clone(next);
                            }
                            session.weapon_id = weapon;
                            session.parent_weapon = ps.weapon_primary;
                        } else {
                            let Some((weapon_handle, parent_handle)) = handles else {
                                product.kind = FpvPoseKind::Hide;
                                return;
                            };
                            pending.0 = Some(PendingFpvSpawnRequest {
                                weapon_handle,
                                parent_handle,
                                gun_index,
                                catalog_id: fpv.0.identity(),
                                weapon_id: weapon,
                                parent_weapon: ps.weapon_primary,
                            });
                            product.kind = FpvPoseKind::Hide;
                            return;
                        }
                        }
                    }
                    Some(gun_index) => {
                        let Some((weapon_handle, parent_handle)) =
                            owners.handles(presented.weapon_epoch(), weapon, ps.weapon_primary)
                        else {
                            product.kind = FpvPoseKind::Hide;
                            return;
                        };
                        pending.0 = Some(PendingFpvSpawnRequest {
                            weapon_handle,
                            parent_handle,
                            gun_index,
                            catalog_id: fpv.0.identity(),
                            weapon_id: weapon,
                            parent_weapon: ps.weapon_primary,
                        });
                        diag::info!(
                            Fpv,
                            "fpv: weapon id → {weapon}; re-queue FPV for new gunXModel"
                        );
                        product.kind = FpvPoseKind::Hide;
                        return;
                    }
                    None => {
                        product.kind = FpvPoseKind::Hide;
                        return;
                    }
                }
            } else {
                product.kind = FpvPoseKind::Refuse(FpvPoseRefuse::CatalogMissing);
                return;
            }
        }
    }

    let dt = time.delta_secs();
    let (sample, predicted_fire) = match presented.snapshot() {
        Some(snap) => {
            let ps = presented.player(local.0);
            let ws = ps.map(|p| p.weaponstate_primary).unwrap_or(0);
            let sprinting = ps
                .map(|p| (p.pm_flags & playerstate_iw4::pm_flags::SPRINTING) != 0)
                .unwrap_or(false);
            let ads_frac = ps.map(|p| p.f_weapon_pos_frac).unwrap_or(0.0);
            let weap_anim = ps.map(|p| p.weap_anim).unwrap_or(0);
            let clip_ammo = |hand| match (ps, table) {
                (Some(ps), Some(table)) => {
                    let viewmodel = fpv_viewmodel_weapon(ps, table);
                    table.facts_of(viewmodel).map(|facts| {
                        let key = clip_table_key(facts.clip_index, viewmodel);
                        get_clip_for_hand(&ps.ammoclip, key, hand)
                    })
                }
                _ => None,
            };
            let cues = presented.fpv_cues(local.0);
            let predicted_fire = if let Some(ps) = ps {
                let life = snap
                    .meta
                    .for_client(local.0)
                    .map(|m| m.life_sequence.0)
                    .unwrap_or(0);
                let id = local_shot_identity(life, 0, ps.weapon_shot_count, ps.weap_anim);
                let edged = cursor.0.local_shot.observe(id);
                let masked = ps.weap_anim as u32 & weapon_iw4::WEAP_ANIM_EVENT_MASK;
                edged && is_predicted_fire_weap_anim(masked)
            } else {
                false
            };
            (
                Some(FpvAuthoritySample {
                    tick: snap.tick.0,
                    weaponstate: ws,
                    cues: AuthorityFpvCues {
                        shot_accepted: false,
                        attack_released: cues.attack_released,
                        spawned: cues.spawned,
                    },
                    sprinting,
                    ads_frac,
                    weap_anim,
                    weap_anim_secondary: ps.map(|p| p.weap_anim_secondary).unwrap_or(0),
                    last_weapon_hand: ps
                        .map(|p| {
                            if table
                                .and_then(|t| t.facts_of(session.weapon_id))
                                .is_some_and(|f| f.dual_wield)
                            {
                                1
                            } else {
                                p.last_weapon_hand
                            }
                        })
                        .unwrap_or(0),
                    perks0: ps.map(|p| p.perks[0]).unwrap_or(0),
                    clip_ammo: clip_ammo(0),
                    left_clip_ammo: clip_ammo(1),
                }),
                predicted_fire,
            )
        }
        None => (None, false),
    };
    let rocket_visible = table.is_some_and(|table| {
        fpv_rocket_should_attach(table, session.weapon_id, presented.player(local.0))
    });
    let dual = presented.viewweapon_player(local.0).is_some_and(|ps| {
        ps.last_weapon_hand == 1
            || table
                .and_then(|t| t.facts_of(session.weapon_id))
                .is_some_and(|f| f.dual_wield)
    });
    let dual_offset = if dual {
        presented.viewweapon_player(local.0).and_then(|ps| {
            table?
                .facts_of(fpv_viewmodel_weapon(ps, table?))
                .map(|f| f.dual_wield_view_model_offset)
        })
    } else {
        None
    };
    let weapon_id = session.weapon_id;
    let held = if session.parent_weapon != 0 {
        session.parent_weapon
    } else {
        weapon_id
    };
    product.camo = presented.viewweapon_player(local.0).map_or(0, |ps| {
        weapon_iw4::weapon_model_for_held(&ps.weapons, &ps.weapon_data, held)
    });
    let SessionFpvMeshesHandles {
        fpv: equipped,
        active_rig,
        view: equipped_view,
        ..
    } = session;
    let (mut kind, notetracks) = generate_fpv_pose(FpvGenerateArgs {
        dt,
        equipped,
        rigs: &equipped_view.rigs,
        active: active_rig,
        cursor: &mut cursor.0,
        rocket: rocket_visible,
        melee: presented.viewweapon_player(local.0).is_some_and(|ps| {
            matches!(
                weapon_iw4::WeaponState::from_i32(ps.weaponstate_primary),
                Ok(weapon_iw4::WeaponState::MeleeInit | weapon_iw4::WeaponState::MeleeFire)
            )
        }),
        ads: presented
            .viewweapon_player(local.0)
            .is_some_and(|ps| ps.f_weapon_pos_frac >= 1.0),
        jammed: presented
            .player(local.0)
            .is_some_and(|ps| ps.other_flags & playerstate_iw4::other_flags::EMP_JAMMED != 0),
        sample,
        predicted_fire,
        dual,
        dual_offset,
    });
    if weapon_id != 0
        && (!notetracks.records.is_empty() || notetracks.discarded != 0)
        && let Some(snapshot) = presented.snapshot()
        && let Some(meta) = snapshot.meta.for_client(local.0)
    {
        pending_notes.batch = Some(notetracks.into_audio_batch(
            *generation,
            events.timeline(),
            local.0,
            meta.life_sequence,
            weapon_id,
            snapshot.tick,
        ));
    }
    if let FpvPoseKind::Posed(frame) = &mut kind {
        // Nothing downstream of the bones waits for a vertex.
        if let Some(bolt) = frame.secondary_bolt.take() {
            bolts.set_pose(1, bolt);
        }
        for (hand, pose) in frame.poses.iter_mut().enumerate() {
            if let Some(pose) = pose.as_mut() {
                bolts.set_pose(hand, core::mem::take(&mut pose.bolt));
            }
        }
    }
    product.kind = kind;
}

/// Keep the hands draws already installed and add the parsed cast's surfaces
/// onto that plan. The gun shares the view matrix. No material is allocated:
/// the draws reuse a pass the hands plan already holds.
fn append_parsed_gun(
    plan: &mut crate::FpvDrawPlan,
    rig: &PreparedFpvRig,
    pose: &crate::anim::fpv_rig::FpvHandPose,
    skel: &asset_model::ModelSkel,
) -> Option<u32> {
    if skel.name != NX1_HELD_VIEW
        || skel.bones.len() != NX1_VIEW_BONES
        || skel.surface_index_ranges.len() != NX1_VIEW_SURFACES
        || skel.packed_vertices.is_empty()
        || skel.indices.is_empty()
        || plan.materials.is_empty()
        || rig.is_dual()
    {
        return None;
    }
    let hands_draws = plan.hands_plan_n.unwrap_or(0) as usize;
    if hands_draws == 0 {
        return None;
    }
    let (gun_indices, gun_ranges, gun_rows) = rig.parsed_gun_rows(pose, skel)?;
    if gun_ranges.len() != NX1_VIEW_SURFACES || gun_rows.is_empty() {
        return None;
    }
    let (hands_indices, hands_verts) = rig.hands_span()?;
    if hands_indices > plan.indices.len()
        || hands_draws > plan.draws.len()
        || hands_draws > plan.surface_ranges.len()
    {
        return None;
    }
    let hands_end = plan.surface_ranges[..hands_draws]
        .iter()
        .map(|(start, count)| start.saturating_add(*count))
        .max()
        .unwrap_or(0) as usize;
    if hands_end > hands_indices {
        return None;
    }
    let asset_world::PackedVertexPayload::Iw4(rows) = &plan.packed_vertices else {
        return None;
    };
    if hands_verts > rows.len() {
        return None;
    }
    let vert_base = u32::try_from(hands_verts).ok()?;
    let index_base = u32::try_from(hands_indices).ok()?;
    if gun_indices
        .iter()
        .any(|index| index.checked_add(vert_base).is_none())
    {
        return None;
    }
    for &(start, count) in &gun_ranges {
        let end = start.checked_add(count)?;
        if end as usize > gun_indices.len() || index_base.checked_add(start).is_none() {
            return None;
        }
    }

    plan.indices.truncate(hands_indices);
    plan.surface_ranges.truncate(hands_draws);
    plan.draws.truncate(hands_draws);
    let asset_world::PackedVertexPayload::Iw4(rows) = &mut plan.packed_vertices else {
        return None;
    };
    rows.truncate(hands_verts);
    rows.extend_from_slice(&gun_rows);
    plan.decoded_n = rows.len();
    plan.indices
        .extend(gun_indices.iter().map(|index| index + vert_base));
    for &(start, count) in &gun_ranges {
        let surface = plan.surface_ranges.len() as u32;
        plan.surface_ranges.push((index_base + start, count));
        plan.draws.push(crate::FpvSurfaceDraw {
            surface,
            material: 0,
            is_scope: false,
        });
    }
    let gun_n = plan.draws.len().saturating_sub(hands_draws) as u32;
    plan.gun_plan_n = Some(gun_n);
    plan.plan_draw_n = Some(plan.draws.len() as u32);
    let claimed = gun_n == NX1_VIEW_SURFACES as u32;
    plan.revisions.bump_surfaces();
    plan.revisions.bump_vertices();
    let topology =
        crate::topology_fingerprint(&plan.indices, &plan.surface_ranges, plan.decoded_n);
    plan.revision = crate::stamp_plan_geometry(&mut plan.revisions, plan.revision, topology);
    plan.geometry_ok = !plan.draws.is_empty();
    claimed.then_some(gun_n)
}

/// Write this frame's vertices into the buffer the rig published, and nothing
/// else: indices, surface ranges, materials and draws belong to the composition.
fn skin_fpv_geometry(
    product: Res<FpvPoseProduct>,
    session_vm: Option<Res<SessionViewmodel>>,
    tess: Option<Res<render_scene::TessMaterials>>,
    owners: FpvOwnerInputs,
    mut fpv_plan: ResMut<crate::FpvDrawPlan>,
    mut status: ResMut<FpvStatusGap>,
    gaps: Res<RenderPresentationGaps>,
    mut lenses: Query<
        &mut Transform,
        (
            With<FpvLens>,
            Without<RemotePlayer>,
            Without<WorldScriptModelInstance>,
        ),
    >,
) {
    let fpv_meshes = owners.meshes.as_ref();
    fpv_plan.drawgun = product.drawgun;
    let handle = fpv_plan.lighting_handle;
    match &product.kind {
        FpvPoseKind::Hide => crate::clear_fpv_draw_plan(&mut fpv_plan, handle),
        FpvPoseKind::Refuse(refuse) => {
            let cause = refuse_gap_cause(refuse.clone());
            crate::clear_fpv_draw_plan(&mut fpv_plan, handle);
            gaps.raise(cause.clone());
            status.0 = Some(FpvState::Blocked(cause));
        }
        FpvPoseKind::Posed(frame) => {
            let session = session_vm.as_ref().and_then(|session| session.0.as_ref());
            if !session.is_some_and(|session| {
                same_material_catalog(&session.material_catalog, tess.as_deref())
                    && owners.bind(&session.table).is_some()
            }) {
                crate::clear_fpv_draw_plan(&mut fpv_plan, handle);
                return;
            }
            let rig = session.and_then(|session| session.active_rig.as_ref());
            let (Some(rig), Some(catalog)) = (rig, fpv_meshes.as_ref()) else {
                crate::clear_fpv_draw_plan(&mut fpv_plan, handle);
                return;
            };
            if session.is_none_or(|session| session.catalog_id != catalog.0.identity()) {
                crate::clear_fpv_draw_plan(&mut fpv_plan, handle);
                return;
            }
            let scar2 = class_payload_base_is_scar2(
                owners
                    .classes
                    .as_ref()
                    .and_then(|c| c.equipped_primary.as_deref()),
            );
            let equipped = session.map(|session| session.weapon_id).unwrap_or(0);
            // Slot 0 was the sidearm, so that test submitted the cast on the
            // pistol and left the rifle on viewmodel_m4. The primary that sent
            // `m4` is the weapon named m4, not a weapons[] slot.
            let sent_m4 = owners.weapons.as_ref().is_some_and(|weapons| {
                let name = weapons.registry().name_of(equipped);
                name == "m4" || name == "m4_mp"
            });
            let submit_nx1 = scar2 && sent_m4;
            if equipped != 0 {
                static SEEN: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
                let mut seen = SEEN.lock().unwrap_or_else(|poison| poison.into_inner());
                if !seen.contains(&equipped) {
                    seen.push(equipped);
                    let name = owners
                        .weapons
                        .as_ref()
                        .map(|weapons| weapons.registry().name_of(equipped))
                        .unwrap_or("");
                    let scar2_bit = u8::from(scar2);
                    let sent_bit = u8::from(sent_m4);
                    let submit_bit = u8::from(submit_nx1);
                    diag::info!(
                        Fpv,
                        "fpv gate: weapon={equipped} name={name} scar2={scar2_bit} sent_m4={sent_bit} submit={submit_bit}"
                    );
                }
            }
            if submit_nx1
                || fpv_plan.rig_generation != rig.generation()
                || fpv_plan.camo != product.camo
            {
                let camo = session.and_then(|session| session.view.camo(product.camo));
                crate::install_prepared_fpv_plan(
                    &mut fpv_plan,
                    &rig.geometry,
                    camo.map(|swaps| &**swaps),
                    handle,
                );
                fpv_plan.rig_generation = rig.generation();
                fpv_plan.camo = product.camo;
            }
            if let Some(rows) = fpv_plan.packed_rows_mut() {
                if !rig.skin_into(&frame.poses, rows) {
                    crate::clear_fpv_draw_plan(&mut fpv_plan, handle);
                    return;
                }
            }
            // Hands are already in the plan. The parsed gun is added onto that
            // plan, parented to the same view. It is not swapped in for the hands.
            let written_gun = if submit_nx1 {
                fpv_meshes.as_ref().and_then(|meshes| {
                    let order = meshes.0.parsed_model_order(
                        NX1_HELD_VIEW,
                        NX1_VIEW_BONES,
                        NX1_VIEW_SURFACES,
                    )?;
                    let skel = &meshes.0.get_at(order)?.skel;
                    let pose = frame.poses[0].as_ref()?;
                    append_parsed_gun(&mut fpv_plan, rig, pose, skel)
                })
            } else {
                None
            };
            if scar2 && session.is_some_and(|session| session.weapon_id == 591) {
                static LOGGED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    let session = session.unwrap();
                    let meshes = fpv_meshes.as_ref().map(|meshes| &meshes.0);
                    let gun_name = session
                        .table
                        .gun_index(591)
                        .and_then(|index| meshes.and_then(|catalog| catalog.name_at(index.order())))
                        .unwrap_or("<unresolved>");
                    let named = owners
                        .weapons
                        .as_ref()
                        .and_then(|weapons| weapons.registry().gun_xmodel_of(591))
                        .unwrap_or("<none>");
                    let override_name = meshes
                        .and_then(|catalog| {
                            catalog.parsed_model_order(
                                NX1_HELD_VIEW,
                                NX1_VIEW_BONES,
                                NX1_VIEW_SURFACES,
                            )
                        })
                        .and_then(|order| meshes.and_then(|catalog| catalog.name_at(order)))
                        .unwrap_or("<missing>");
                    let surfaces = fpv_plan.gun_plan_n.unwrap_or(0) as usize;
                    let (skin, bones) = if written_gun == Some(NX1_VIEW_SURFACES as u32) {
                        (NX1_HELD_VIEW, NX1_VIEW_BONES)
                    } else {
                        (
                            rig.gun_mesh_name().unwrap_or("<unread>"),
                            rig.gun_mesh_shape().map(|(bones, _)| bones).unwrap_or(0),
                        )
                    };
                    diag::info!(
                        Fpv,
                        "fpv table: weapon=591 gun_xmodel={gun_name} row_gun_xmodel={named} override={override_name} skin={skin} bones={bones} surfaces={surfaces}"
                    );
                }
            }
            fpv_plan.revisions.bump_vertices();
            fpv_plan.geometry_ok = !fpv_plan.draws().is_empty();
            fpv_plan.settle_visible();
            gaps.clear(RenderGap::FpvViewmodel);
            status.0 = Some(FpvState::Drawn {
                idle_sampled: frame.idle_sampled,
            });
            for mut lens_tf in &mut lenses {
                *lens_tf = Transform::from_matrix(frame.lens);
            }
        }
    }
}

/// Where the viewmodel sits this frame. It reads the placement the camera and
/// the root carry and nothing the rig produced, so it does not wait behind the
/// geometry.
pub fn stamp_fpv_placement_matrix(
    mut fpv_plan: ResMut<crate::FpvDrawPlan>,
    cameras: Query<&Transform, (With<FlyCamera>, Without<FpvPlacementRoot>)>,
    roots: Query<&Transform, With<FpvPlacementRoot>>,
) {
    let (Ok(cam), Ok(local)) = (cameras.single(), roots.single()) else {
        fpv_plan.placement_ok = false;
        fpv_plan.settle_visible();
        return;
    };
    fpv_plan.world_from_local = cam.to_matrix() * local.to_matrix();
    fpv_plan.placement_ok = true;
    fpv_plan.settle_visible();
}

pub fn publish_fpv_dobj_pose(
    fpv_plan: Res<crate::FpvDrawPlan>,
    roots: Query<(), With<FpvPlacementRoot>>,
    mut bolts: ResMut<FpvBoltTargets>,
    mut dobj_poses: ResMut<crate::anim::dobj_pose::HostDObjPoseFrame>,
) {
    if roots.single().is_err() {
        bolts.clear();
        return;
    }
    bolts.tracker_screen = None;
    bolts.tracker_light = None;
    for hand in 0..2usize {
        let Some(frame) = bolts.pose[hand].take() else {
            continue;
        };
        let dobj = fx_iw4::FX_BOLT_VIEWMODEL_DOBJ_BASE + hand as u32;

        if dobj_poses
            .publish(dobj, true, 0, fpv_plan.world_from_local, &frame.bones)
            .is_err()
        {
            continue;
        }
        let target = |bone: Option<u16>| -> Option<fx::FxBoltTarget> {
            let bone = bone?;
            let orientation = dobj_poses.resolve(dobj, i32::from(bone)).ok()?;
            Some(fx::FxBoltTarget {
                dobj,
                bone,
                centity_teleport: false,
                orientation,
            })
        };

        if hand == 0 && fpv_plan.placement_ok {
            bolts.tracker_light = target(frame.tags.tracker_light);
            bolts.tracker_screen = (|| {
                let mut points = [Vec3::ZERO; 3];
                for (point, bone) in points.iter_mut().zip(frame.tags.tracker_screen) {
                    *point = fpv_plan
                        .world_from_local
                        .transform_point3(frame.bones.get(usize::from(bone?))?.w_axis.truncate());
                }
                Some(points)
            })();
        }
        bolts.flash[hand] = target(frame.tags.flash);
        bolts.brass[hand] = target(frame.tags.brass);
        bolts.knife[hand] = target(frame.tags.knife);
        bolts.laser[hand] = target(frame.tags.laser);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn apply_fpv_placement(
    clock: Res<FrameClock>,
    presented: Res<PresentedSnapshot>,
    local: Res<LocalPresentClient>,
    prepared: Res<PreparedFpv>,
    mut kick: ResMut<SessionViewKick>,
    cg_gun: Res<GunOffset>,
    mut aim: ResMut<ViewweaponAim>,
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    mut roots: Query<
        &mut Transform,
        (
            With<FpvPlacementRoot>,
            Without<RemotePlayer>,
            Without<WorldScriptModelInstance>,
        ),
    >,
    view_settings: (Res<ViewSubject>, Res<frame::GameSettings>),
    mut gfx_scene: ResMut<HostGfxScene>,
) {
    let (view, settings) = view_settings;
    *aim = ViewweaponAim::default();
    let Ok(mut transform) = roots.single_mut() else {
        return;
    };
    let Some(ps) = presented.player(local.0) else {
        return;
    };
    if presented_is_third_person(
        &presented,
        local.0,
        view.in_killcam(),
        settings.third_person,
    ) {
        return;
    }
    let Some(table) = prepared.table() else {
        return;
    };
    let viewmodel = fpv_viewmodel_weapon(ps, table);
    let Some(facts) = table.facts_of(viewmodel) else {
        return;
    };

    let mut state = WeaponPlacementState {
        sway_springs: kick.sway.springs(),
        gun_recoil: kick.state.gun,
        movement_origin: kick.placement_move_origin,
        movement_angles: kick.placement_move_angles,
        weap_idle_time: kick.weap_idle_time,
        last_idle_factor: kick.last_idle_factor,
        damage_kick_time: clock.time(),
        damage_time: kick.damage_time,
        v_dmg_pitch: kick.v_dmg_pitch,
        v_dmg_roll: kick.v_dmg_roll,
        ..Default::default()
    };

    let overlay_reticle = if facts.overlay_reticle != 0 {
        facts.overlay_reticle
    } else if table.overlay_is_hud_iris(viewmodel) {
        1
    } else {
        0
    };
    let ps_in = WeaponPlacementPsInputs {
        e_flags: ps.e_flags,
        weapon_pos_frac: ps.f_weapon_pos_frac,
        weapon_time: ps.weapon_time,
        aim_down_sight: facts.aim_down_sight,
        overlay_reticle,
        weapon_transition_active: false,

        lean_fraction: 0.0,
    };
    let stance = WeaponStanceStaticOfsInputs {
        ducked_ofs: facts.ducked_ofs,
        prone_ofs: facts.prone_ofs,
        ads_aim_pitch: facts.ads_aim_pitch,
        night_vision_wear_time: facts.night_vision_wear_time,
    };
    let bob_inputs = WeaponBobInputs {
        ads_bob_factor: facts.ads_bob_factor,
    };
    let xyspeed = {
        let vx = ps.velocity[0];
        let vy = ps.velocity[1];
        vec3_length([vx, vy, 0.0])
    };
    let kinematics = WeaponMovementKinematics {
        xyspeed,
        speed: ps.speed as f32,
        velocity: ps.velocity,
        viewangles: ps.viewangles,
        weaponstate: ps.weaponstate_primary,
        weaponstate_secondary: ps.weaponstate_secondary,
        pm_flags: ps.pm_flags,
        frametime: clock.frametime_secs(),
    };
    let waveform = calculate_weapon_movement_bob_waveform(WeaponBobWaveformInputs {
        bob_cycle: (ps.bob_cycle as u32 & 0xff) as u8,
        xyspeed,
        view_height_target: ps.view_height_target,
        pm_flags: ps.pm_flags,
        weapon_pos_frac: ps.f_weapon_pos_frac,
    });
    let hip = GunRecoilResponse::default();
    let ads = GunRecoilResponse::default();
    let mut steps = [WeaponPlacementAssembleStep::Sway; PLACEMENT_ASSEMBLE_STEP_COUNT];

    let mut idle = facts.idle;
    if facts.can_hold_breath {
        idle.ads_idle_amount *= ps.hold_breath_scale;
    }
    let contrib = weapon_placement_assemble(
        &mut state,
        ps_in,
        stance,
        StanceTransitionFadeGlobals::default(),
        facts.movement,
        kinematics,
        bob_inputs,
        idle,
        Some(waveform),
        hip,
        ads,
        facts.kick.gun_max_pitch,
        facts.kick.gun_max_yaw,
        0.0,
        &mut steps,
    );
    kick.placement_move_origin = state.movement_origin;
    kick.placement_move_angles = state.movement_angles;
    kick.weap_idle_time = state.weap_idle_time;
    kick.last_idle_factor = state.last_idle_factor;
    let mut origin = apply_viewweapon_land_view(
        apply_cg_gun_offset_view(contrib.origin, cg_gun.xyz()),
        kick.viewweapon_land_view,
    );
    if ps.last_weapon_hand == 1 {
        let add = dual_wield_view_model_origin_add(
            0,
            [0.0, 1.0, 0.0],
            facts.dual_wield_view_model_offset,
        );
        origin[0] += add[0];
        origin[1] += add[1];
        origin[2] += add[2];
    }
    let from_axis = viewweapon_iron_ads_saves_composed_axis(
        facts.aim_down_sight,
        ps.f_weapon_pos_frac,
        overlay_reticle,
    );
    let [gun_pitch, gun_yaw] = viewweapon_save_gun_pitch_yaw(
        contrib.angles,
        kick.refdef_view_angles,
        facts.aim_down_sight,
        ps.f_weapon_pos_frac,
        overlay_reticle,
    );
    let xhair = if kick.horiz_fov_deg > 0.0 {
        if let Ok(window) = windows.single() {
            let height = window.height().max(1.0);
            let aspect = window.width() / height;
            let (tan_x, tan_y) = tan_half_fov(kick.horiz_fov_deg, aspect);
            let (vf, vr, vu) = angle_vectors(kick.refdef_view_angles);
            calc_crosshair_position(
                gun_pitch,
                gun_yaw,
                kick.refdef_view_angles[2],
                vf,
                vr,
                vu,
                tan_x,
                tan_y,
            )
        } else {
            [0.0, 0.0]
        }
    } else {
        [0.0, 0.0]
    };
    *aim = ViewweaponAim {
        live: true,
        weapon: viewmodel,
        angle_offset: [
            math_iw4::angle_subtract(gun_pitch, ps.viewangles[0]),
            math_iw4::angle_subtract(gun_yaw, ps.viewangles[1]),
        ],
        gun_pitch,
        gun_yaw,
        xhair_x: xhair[0],
        xhair_y: xhair[1],
        from_composed_axis: from_axis,
    };
    let placed = iw_view_placement_to_bevy_camera_local(origin, contrib.angles);
    let world_delta = viewweapon_view_to_world_delta(origin, kick.refdef_view_angles);
    let pose_origin = [
        kick.refdef_vieworg[0] + world_delta[0],
        kick.refdef_vieworg[1] + world_delta[1],
        kick.refdef_vieworg[2] + world_delta[2],
    ];
    let pose_quat = scene_quat_from_viewmodel_axes(contrib.angles, kick.refdef_view_angles);
    gfx_scene
        .scene
        .store_pose_origin_quat(SCENE_VIEWMODEL_ENTNUM, pose_origin, Some(pose_quat));
    if ps.last_weapon_hand == 1 {
        gfx_scene.scene.store_pose_origin_quat(
            SCENE_VIEWMODEL_LEFT_ENTNUM,
            pose_origin,
            Some(pose_quat),
        );
    }
    *transform = placed;
}

fn fpv_spawn_queued(pending: Res<PendingFpvSpawn>) -> bool {
    pending.0.is_some()
}

fn flush_fpv_spawn(world: &mut World) {
    world.flush();
}

fn publish_fpv_notetracks(
    pending: Res<PendingFpvNotetracks>,
    mut notes: MessageWriter<audio::ViewmodelNotetracks>,
) {
    if let Some(batch) = &pending.batch {
        notes.write(batch.clone());
    }
}

pub fn register_fpv_present_systems(app: &mut App) {
    app.init_resource::<frame::ScreenEffectsView>()
        .init_resource::<frame::ScreenEffectsDvars>()
        .add_systems(
            Update,
            super::screen_effects::update
                .in_set(frame::ScreenEffectsPublished)
                .in_set(LifeFrontPublished)
                .after(reset_view_kick_on_life_started),
        )
        .init_resource::<SessionViewmodel>()
        .init_resource::<PreparedFpv>()
        .init_resource::<crate::anim::model_materials::PreparedModelMaterials>()
        .init_resource::<SessionViewKick>()
        .init_resource::<GunOffset>()
        .init_resource::<ViewweaponAim>()
        .init_resource::<PendingViewHurt>()
        .init_resource::<FpvStatusGap>()
        .init_resource::<RenderPresentationGaps>()
        .add_systems(
            Update,
            reset_view_kick_on_life_started.in_set(LifeFrontPublished),
        )
        .add_systems(
            Update,
            occupy_fpv_scene
                .after(PresentedPublished)
                .in_set(render_scene::GfxSceneAdd)
                .in_set(AnimSceneSubmit),
        )
        .add_systems(
            Update,
            (
                tick_session_view_kick.after(reset_view_kick_on_life_started),
                sync_camera_from_presented.after(tick_session_view_kick),
                spawn_pending_fpv
                    .run_if(fpv_spawn_queued)
                    .after(sync_camera_from_presented),
                flush_fpv_spawn.after(spawn_pending_fpv),
                tick_fpv_viewmodel.after(flush_fpv_spawn),
                skin_fpv_geometry
                    .after(tick_fpv_viewmodel)
                    .in_set(FpvGeometrySet),
                publish_fpv_notetracks.after(tick_fpv_viewmodel),
                apply_fpv_placement
                    .after(tick_fpv_viewmodel)
                    .before(WorkerCmdSet::CellSceneEnt),
                // Neither placement nor bone publication reads a vertex.
                stamp_fpv_placement_matrix
                    .after(apply_fpv_placement)
                    .in_set(FpvPlacementSet),
                publish_fpv_dobj_pose
                    .after(stamp_fpv_placement_matrix)
                    .after(crate::anim::dobj_pose::begin_dobj_pose_frame),
            )
                .in_set(ClientSet::Present),
        );
}

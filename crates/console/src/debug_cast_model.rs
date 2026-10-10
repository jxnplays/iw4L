//! `debug_cast_model` — place one converted NX1 `.cast` model at the player.
//!
//! A debug spawn, not a feature. The cast converter produces a `ModelSkel` that
//! nothing loads on its own, so this puts one in the scene catalog under a
//! private key and drops a model entity at the local player. Every draw
//! downstream is the map's existing path: `pose_dyn_ents` reads the catalog,
//! poses the bones, and hands the surfaces to the same plan the map's own dyn
//! ents use.
//!
//! The cast file's material names are DTZxPorter paths with no IW4 catalog
//! entry, so the surfaces cannot name a material of their own. A surface with no
//! material is skipped at draw time, which would make the model invisible, so
//! this binds all of them to one material the loaded map has already resolved.
//! No catalog entry is added: the binding reuses a `MaterialIndex` that came out
//! of the map's own `resolve_surface_materials`, so it resolves through the
//! map's material table exactly as a captured model's surface does. The texture
//! is whatever that material is, which is the wrong one for this gun.
//!
//! The `.cast` path comes from `IW4L_CAST_MODEL`, never from the command line,
//! so this cannot be pointed at an arbitrary file.

use std::sync::Arc;

use bevy::prelude::*;
use render_anim::PreparedModelMaterials;
use render_scene::{TessMaterials, WorldDynEntInstance};

use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};
use net::{LocalPresentClient, PresentedSnapshot};

/// Catalog key for the debug model. Namespaced so it cannot collide with a name
/// the map captured.
const CAST_MODEL_KEY: &str = "iw4l_debug_cast";

/// Distance in front of the eye, in world units. IW4 world units are inches, so
/// 1.2 m is 47.24 units and 0.3 m down is 11.81.
const CAST_VIEW_FORWARD_UNITS: f32 = 47.24;
const CAST_VIEW_DOWN_UNITS: f32 = 11.81;

/// Scale from cast units to IW4 world units.
///
/// The scale now lives on the skel, in `read_cast_xmodel`, because a cast read
/// as an `FpvMeshEntry` has no entity transform to carry it. Applying it here
/// as well would scale the debug model by 1/2.54 twice.
const CAST_TO_WORLD: f32 = 1.0;

/// Scale to apply to the cast skel's local geometry.
fn cast_scale() -> Vec3 {
    Vec3::splat(CAST_TO_WORLD)
}

/// Place the model in front of the eye along the view forward.
///
/// `xf` is the FPV camera's world transform. The forward axis is `-Z` in the
/// camera's local space, which is the convention `fpv_frustum_planes` uses for
/// the same camera.
fn cast_view_placement(xf: GlobalTransform) -> Vec3 {
    let forward = xf.rotation() * Vec3::NEG_Z;
    xf.translation() + forward * CAST_VIEW_FORWARD_UNITS - Vec3::Y * CAST_VIEW_DOWN_UNITS
}

/// The spawned debug model, kept so it can be held in front of the view.
#[derive(Resource, Default)]
pub(crate) struct CastModelHolder(pub Option<Entity>);

pub(crate) fn register_debug_cast_model_commands(registry: &mut ConsoleRegistry) {
    if registry.resolve("debug_cast_model").is_none() {
        registry.register(crate::CommandSpec::new("debug_cast_model").usage(
            "debug_cast_model — place the NX1 .cast model from IW4L_CAST_MODEL at the player",
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn route_debug_cast_model_commands(
    mut events: MessageReader<ConsoleCommand>,
    mut console: ResMut<ConsoleState>,
    settings: Res<ConsoleSettings>,
    mut line: ResMut<ConsoleLine>,
    mut commands: Commands,
    mut catalog: Option<ResMut<asset_world::MapXModelSceneCatalog>>,
    mut prepared: ResMut<PreparedModelMaterials>,
    tess: Option<Res<TessMaterials>>,
    trace: Option<Res<render_anim::DynEntDebugTrace>>,
    mut last_trace: Local<Option<String>>,
    mut hold: ResMut<CastModelHolder>,
    cameras: Query<&GlobalTransform, With<render_scene::FpvLens>>,
    mut placed: Query<&mut Transform>,
    _presented: Res<PresentedSnapshot>,
    _local: Res<LocalPresentClient>,
) {
    let capacity = settings.log_capacity;
    let echo = |msg: String, console: &mut ConsoleState, line: &mut ConsoleLine| {
        diag::info!(Console, "{msg}");
        line.0 = msg.clone();
        console.echo(msg, capacity);
    };

    // The draw path writes a reason here whenever it drops the debug model. Echo
    // it on the same console as the place line, once per distinct reason, so the
    // gate that stops it is visible without reading the log file.
    if let Some(trace) = trace.as_deref()
        && let Some(reason) = trace.0.clone()
        && last_trace.as_deref() != Some(reason.as_str())
    {
        *last_trace = Some(reason.clone());
        echo(format!("debug_cast_model: {reason}"), &mut console, &mut line);
    }

    // Hold the spawned model in front of the view every frame. Without this it
    // keeps the transform it was given once and stays where the player was when
    // the command ran.
    if let Some(entity) = hold.0
        && let Some(xf) = cameras.single().ok().copied()
        && let Ok(mut transform) = placed.get_mut(entity)
    {
        // Only the entity this command spawned. A query filtered on
        // `WorldDynEntInstance` would also match every map dyn-ent and drag the
        // map's clutter to the eye, which is what made the world look exploded.
        let at = cast_view_placement(xf);
        transform.translation = at;
        transform.scale = cast_scale();
    }

    for cmd in events.read() {
        if cmd.name != "debug_cast_model" {
            continue;
        }
        if std::env::var_os("IW4L_CAST_MODEL").is_none() {
            echo(
                "debug_cast_model: IW4L_CAST_MODEL is not set".into(),
                &mut console,
                &mut line,
            );
            continue;
        }
        let Some(path) = std::env::var_os("IW4L_CAST_MODEL") else {
            echo(
                "debug_cast_model: IW4L_CAST_MODEL is not set".into(),
                &mut console,
                &mut line,
            );
            continue;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) => {
                echo(
                    format!("debug_cast_model: {}: {error}", path.display()),
                    &mut console,
                    &mut line,
                );
                continue;
            }
        };
        // `pose_script_dobj_with_materials` looks a surface's material up by
        // `MapXModelAssetKey(skel.name)`, not by the key this model was inserted
        // under. Those must be the same string or every surface resolves to None
        // and the draw path drops the asset before it reaches a material. The file
        // name is only used for reporting, so keep it aside and hand the converter
        // the catalog key.
        let file_name = std::path::Path::new(&path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(CAST_MODEL_KEY)
            .to_owned();
        let name = CAST_MODEL_KEY;
        let skel = match asset_model::cast_xmodel::read_cast_xmodel(&bytes, name) {
            Ok(skel) => skel,
            Err(error) => {
                echo(
                    format!("debug_cast_model: {error}"),
                    &mut console,
                    &mut line,
                );
                continue;
            }
        };

        let Some(catalog) = catalog.as_deref_mut() else {
            echo(
                "debug_cast_model: no scene catalog (not in a map)".into(),
                &mut console,
                &mut line,
            );
            continue;
        };

        // One material the map already resolved, so no surface is skipped. If
        // the map resolved none, say so instead of placing a model that cannot
        // draw.
        let Some(authored) = catalog.first_resolved_material() else {
            echo(
                "debug_cast_model: the map resolved no material to bind".into(),
                &mut console,
                &mut line,
            );
            continue;
        };
        let surfaces = skel.surface_materials.len();
        let key = asset_world::MapXModelAssetKey(CAST_MODEL_KEY.to_owned());
        // Do NOT clear surface_materials here. The catalog is what resolves a
        // surface's material, but `meshes_from_blended` also requires the skel's
        // own row count to equal the surface count, and clearing drops it to 0
        // against 7 surfaces, which makes the whole pose return None.
        // Publish the DObj before the skel moves into the Arc below. The key is
        // the same one the catalog uses, so cull_dyn_ent_cell_models resolves
        // both halves. prepare_model_materials already ran and never saw this
        // model, so without this the lookup at dyn_ent.rs:540 misses and the
        // surfaces are dropped before any draw.
        let published = prepared.publish_scene_dobj(CAST_MODEL_KEY, &skel);
        catalog.insert(
            key.clone(),
            asset_world::MapXModelSceneAsset::Iw4(Arc::new(skel)),
        );
        catalog.bind_surfaces_to_material(&key, authored, surfaces);
        // Measured after the insert and the bind above. Reading these before them
        // reports a miss for a row that does not exist yet, which is not a
        // statement about the draw path.
        let authored_hits = (0..surfaces)
            .filter(|surface| catalog.surface_material(&key, *surface).is_some())
            .count();
        let authored_none = surfaces - authored_hits;
        // `authored` returns None for two unrelated reasons: the index may be
        // absent from `by_authored`, or the resource may not be settled for the
        // catalog in hand. Report them separately.
        let (by_authored_hit, settled) = match tess.as_deref() {
            Some(tess) => prepared.authored_state(tess.catalog(), authored),
            None => (false, false),
        };
        if !published {
            echo(
                format!(
                    "debug_cast_model: placed `{name}` but published no DObj; it will not draw this frame"
                ),
                &mut console,
                &mut line,
            );
        }

        let eye_pos = cameras.single().ok().copied();
        let Some(eye) = eye_pos else {
            echo(
                "debug_cast_model: no FPV camera to place it in front of".into(),
                &mut console,
                &mut line,
            );
            continue;
        };
        let at = cast_view_placement(eye);
        // Scale the local geometry from cast units to IW4 world units. Left at
        // 1.0 the 101-unit mesh is 2.85x too long and engulfs the view.
        let transform = Transform::from_translation(at).with_scale(cast_scale());
        // Hold it in front of the view every frame, not once at spawn. The
        // player origin is a moving, map-space point; the eye is what the viewer
        // is actually looking through, so re-derive the placement each frame or
        // the model drifts out of view as the player turns.
        let entity = commands.spawn((
            transform,
            Visibility::Inherited,
            WorldDynEntInstance {
                // The index and type are the map's own dyn-ent namespace. This
                // model is not a map placement, so it takes a value no placement
                // uses and carries nothing that would give it collision.
                index: u16::MAX,
                ty: asset_world::DynEntType::Clutter,
                current_model: key,
                transform,
                lighting_origin: cast_view_placement(eye).to_array(),
                phys_preset: None,
                health: 0,
                destroy_fx: None,
                dead: false,
            },
        ));
        hold.0 = Some(entity.id());
        // No `DynEntModelEntity` marker on purpose. `cull_dyn_ent_cell_models`
        // queries entities carrying it and hides any whose index is not a member
        // of an admitted cell; this model is not a map dyn-ent, so no cell lists
        // its index and it was hidden before it could be posed. `pose_dyn_ents`
        // queries `WorldDynEntInstance` without that filter, so dropping the
        // marker keeps it in the pose pass while leaving the cell cull alone.
        // The marker also gates the physics pass, which this model has no use for.
        echo(
            format!(
                "debug_cast_model: placed `{file_name}` as `{name}` in front of the view at {at:?} (MaterialIndex order {}; authored resolved {authored_hits}/{surfaces}, None {authored_none}; by_authored {}, settled {})",
                    authored.order(),
                    if by_authored_hit { "hit" } else { "miss" },
                    settled
            ),
            &mut console,
            &mut line,
        );
    }
}

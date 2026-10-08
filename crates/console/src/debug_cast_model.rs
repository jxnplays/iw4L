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
use render_scene::{DynEntModelEntity, WorldDynEntInstance};

use crate::{ConsoleCommand, ConsoleLine, ConsoleRegistry, ConsoleSettings, ConsoleState};
use net::{LocalPresentClient, PresentedSnapshot};

/// Catalog key for the debug model. Namespaced so it cannot collide with a name
/// the map captured.
const CAST_MODEL_KEY: &str = "iw4l_debug_cast";

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
    presented: Res<PresentedSnapshot>,
    local: Res<LocalPresentClient>,
) {
    let capacity = settings.log_capacity;
    let echo = |msg: String, console: &mut ConsoleState, line: &mut ConsoleLine| {
        diag::info!(Console, "{msg}");
        line.0 = msg.clone();
        console.echo(msg, capacity);
    };

    for cmd in events.read() {
        if cmd.name != "debug_cast_model" {
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
        let name = std::path::Path::new(&path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or(CAST_MODEL_KEY)
            .to_owned();
        let mut skel = match asset_model::cast_xmodel::read_cast_xmodel(&bytes, &name) {
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
        skel.surface_materials.clear();
        catalog.insert(
            key.clone(),
            asset_world::MapXModelSceneAsset::Iw4(Arc::new(skel)),
        );
        catalog.bind_surfaces_to_material(&key, authored, surfaces);

        let origin = presented
            .alive_player(local.0)
            .map_or([0.0, 0.0, 0.0], |player| player.origin);
        let transform = Transform::from_translation(Vec3::from_array(origin));
        commands.spawn((
            transform,
            Visibility::Inherited,
            DynEntModelEntity,
            WorldDynEntInstance {
                // The index and type are the map's own dyn-ent namespace. This
                // model is not a map placement, so it takes a value no placement
                // uses and carries nothing that would give it collision.
                index: u16::MAX,
                ty: asset_world::DynEntType::Clutter,
                current_model: key,
                transform,
                lighting_origin: origin,
                phys_preset: None,
                health: 0,
                destroy_fx: None,
                dead: false,
            },
        ));
        echo(
            format!(
                "debug_cast_model: placed `{name}` at {origin:?} ({surfaces} surfaces bound to the map's material)"
            ),
            &mut console,
            &mut line,
        );
    }
}

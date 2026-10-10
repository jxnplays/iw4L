scar2 color and specular slots are empty AuthoredImage placeholders; the real PNGs are on disk under the saluki models folder; no PNG-to-IWI path exists in the clone or the dump tools.
no MW2 weapon loads a bare IWI from disk; color and specular resolve by name into an .iwd or .ff.
an external PNG-to-IWI tool exists at github.com/bruhhwtf/iwi-converter; it is not present in the clone or the dump tools.
scar2 color and specular IWIs are packaged into nx1_scar2_images.iwd under images/; client can now resolve them by name once the .iwd is loaded.
nx1_scar2_images.iwd is now in the client's main\ directory at 1,049,150 bytes.
image_candidates lowercasing checked for scar2 specular name; result: survives.
image index cache check; result: next run sees it.
register_nx1_scar_body_color empty slots confirmed at crates/assets/src/session_load/match_walk.rs:58 and crates/assets/src/session_load/match_walk.rs:84.
normal decode path check; result: will locate.
scar2 material slot reachability; result: reached.
plan_material_color_maps invocation check; result: not invoked after registration.
NamespaceTrees indexing check; result: indexes main\.
game_main_for_zone derivation check; result: derives main\ correctly.
14: the map is told `m4` for the scar2 class because its GSC faults on unknown weapon names, so it reports weapon 591 back; clips, sounds, and the gun mesh were all being read off the m4 row.
15: WeaponRegistry::nx1_scar2_row_of redirects those reads onto the scar2 row when the equipped class names it; the m4 name test in the fpv submit gate is gone.
16: no public way to insert a synthetic weapon row, so the redirect itself is not unit tested; crates/asset_game/tests/nx1_scar2.rs covers the category mapping and the primary parse it depends on.
17: NX1 .cast anims are still unbound: read_cast_xmodel yields one rest pose, no animation tracks.
18: the 9 scar2 WAVs in nx1_scar2_sounds.iwd are still unbound: the .iwd reader indexes images/*.iwi only, and sounds resolve from zone soundbanks.
19: NX1 was first filed as a CacAuthoredCategory variant, which put it under PRIMARY beside Assault Rifles; CacAuthoredCategory is the within-game class axis, so that could never make NX1 top-level.
20: NX1 is now AssetNamespace::Nx1 (an alias target of FamilyId, also used as ZoneGame). The class picker builds folders as (namespace, category) and groups by namespace, so the namespace is what makes it top-level.
21: adding the variant forced 14 exhaustive match arms to be completed across asset_transport, asset_anim, asset_audio, asset_material, asset_model, asset_world, asset_game, assets, audio, and session; NX1 has no fastfile, so most of them decline explicitly rather than aliasing a retail game.
22: asset_transport::NamespaceTrees::slot_mut became fallible (Option) because an NX1 arm had nowhere to store; its two callers now guard.
23: NX1_SCAR2_BASE row keeps the donor's preparation recipe, which is what resolves its meshes in the Iw4 catalog where the cast insert placed them, while the row itself sits in the Nx1 namespace.
24: NX1's item group is weapon_assault, so within the NX1 folder the picker offers an Assault Rifles subcategory; it is not a second top-level axis.
25: not verified by launch: no client binary is built on this branch and the working tree has no display for a smoke run.
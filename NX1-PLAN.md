# NX1 gun viewmodel — analysis and remaining plan

Branch: `nx1-scar2-working`. Goal: one NX1 weapon (SCAR Mod 2) rendering as a
real first-person viewmodel under a top-level NX1 category, then generalise to
all NX1 weapons and maps.

## What is proven working (verified, not guessed)

- `.cast` mesh decode: `viewmodel_scar_h` — 35 bones, 34 surfaces, real geometry.
- `.cast` animation decode: 59 clips; 13 bind to the SCAR2 rig; IDLE =
  `viewmodel_scar_h_idle` on `szXAnims[IDLE]`.
- `.cast` material decode: each surface's `mesh.m` resolves to a real
  `matl` (`mc\mtl_*`); 34 surfaces → 24 distinct materials, all with real
  texture paths that exist in the dump.
- The gun binds NX1's own hands (`nx1_viewhands_us_army`) via a weapon-names-its-
  own-hands override gated on the `nx1_` prefix.
- Per-surface part bits computed from real bone usage (was an all-bones mask that
  hid the whole mesh whenever the donor hid any attachment).
- Placement measured: gun base bone lands in front of the camera (eye-space
  `z = -19.8`). `tag_view` is 60 in forward of the rig root in NX1; IW4 places the
  viewmodel relative to `tag_view`, which is accounted for.

## The roadblock: an ordering bug in the match walk

`match_walk.rs` (this file's sibling) runs in this order:

1. line 769 `global.publish()` — final material definitions.
2. line 961 `link_materials_after_absorb(...)` → `resolve_after_absorb` →
   `fpv_meshes.resolve_materials(...)` — stamps material edges (Absent/Unresolved
   → Bound) for every mesh **currently in the catalog**.
3. line 973 `insert_nx1_cast_meshes(...)` — inserts the NX1 meshes **after that
   pass**.

So the NX1 meshes are captured with `Unresolved(CatalogMiss)` material edges and
are **never stamped**, because the stamping pass already ran. This one ordering
bug explains every observed symptom:

- Donor-bound surfaces: edge is `Unresolved`, not `Absent`, so the pass-material
  path (`fpv_material.rs`) does not fire and `admit_surface` returns **Refused** →
  empty admitted list.
- Unbound surfaces: edge is `Absent`, so pass-material *does* fire and returns
  **Admitted**, but the draw loop (`fpv_rig.rs`) requires `edge.bound_index()`,
  which `Absent` lacks → surface skipped → `gun_plan_n = 0` → nothing drawn.

## The fix (single call, no new machinery)

After `insert_nx1_cast_meshes` and before `fpv_meshes.publish()`, re-stamp the
now-present NX1 meshes:

    fpv_meshes.resolve_materials(&global);

`FpvMeshBuild::resolve_materials` is `pub`. This is the identical pass the retail
meshes already get; the NX1 meshes simply need it run once more after they are
added.

### Why this makes it draw (traced through source)

1. `stamp_xmodel_material_edges` (`link.rs:80`) turns a captured edge into
   `Bound` when `material_index_by_key(key)` resolves to a real material. The NX1
   surfaces bind a real retail material (donor), so the key resolves → `Bound`.
2. `admit_surface` sees `Bound` → `admit_material` on a real material → it has a
   sorted ordinal and an uploaded colour map → **`Admitted`**.
3. `fpv_rig.rs:237-247`: `edge.bound_index()` is `Some` and `admitted()` is
   `Some` → the surface is pushed to `PreparedFpvSurface`.
4. `geometry.gun_plan_n` increments → `fpv_plan.draws()` non-empty →
   `geometry_ok = true` → the smodel submits geometry with a real texture.

Nothing online documents this specific bug — it is IW4L-internal, not a public
issue. The CryEngine docs confirm the viewmodel convention (arms+weapon as one
`_fp` model, weapon attached to a hand bone), consistent with the files here.

## ✅ BREAKTHROUGH — the gun is visible

Commit `b1f099e` (the ordering fix) is the one. At launch the draw plan finally
populated:

    fpv table: weapon=7613 gun_xmodel=nx1_viewmodel_scar_h
               skin=nx1_viewmodel_scar_h bones=35 surfaces=5

`bones` went 0 → 35 and `gun_plan_n` (surfaces) 0 → 5. Playtest confirmed a real
NX1 first-person weapon renders in IW4L — geometry, optic and launcher profile
visible, correctly positioned in the viewmodel spot, held by NX1's own hands.

Remaining problems are now cosmetic/asset-level rather than structural:
- Texture is a borrowed IW4 donor material (drapes brown), not NX1's own maps.
- Orientation: the muzzle points up-right instead of forward (a `tag_weapon`
  placement/rotation correction in the rig).

## Remaining work after a visible gun

- **Real NX1 textures.** Surfaces currently borrow a real IW4 material. Wire
  NX1's own PNG colour/normal/spec maps (`weapon_scar_h_col/nrml/spec`, already
  decoded and confirmed present) through the image pipeline so each surface keeps
  its own cast material. The blocker: colour maps are decoded out of the zone
  image trees, which hold none of the NX1 bytes, so NX1 maps must be injected.
- **Real NX1 sounds.** The 9 staged WAVs are not wired; the row still carries
  donor aliases.
- **The other 29 surfaces.** Currently a single donor material draws all of them;
  per-surface cast materials come back with real textures.
- **Generalise.** Replace the scar2-specific wiring with a gun-agnostic NX1 import
  so any NX1 weapon works, then maps. The format decode is the reusable core; the
  scar2 row wiring is scaffolding.

## Verification discipline (learned the hard way)

- Do not read global census lines (`inapplicable_surfaces`, `refused`) as
  NX1-specific — they are whole-catalog numbers. Use the per-mesh NX1
  diagnostics.
- Prove each layer offline (unit tests against the real dumped files) before
  spending a ~20-minute build. `nx1_surfaces_do_not_claim_every_bone` and the
  draw-contract tests are examples.
- When a fix is provable by reading source (like this ordering bug), do not spend
  a build cycle to confirm the reasoning first.
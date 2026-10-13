# NX1 (Call of Duty: Future Warfare) as an IW4L DLC

**Status:** working first-person weapon, textured asset pipeline incomplete.
**Branch:** `nx1-scar2-working` (29 commits ahead of `master`).
**Known-good build:** `38b75cb`.

Nobody has publicly done this: an NX1 weapon, parsed out of the leaked Xbox 360
`.cast` format, running as a first-person viewmodel inside IW4L.

---

## What this is, architecturally

NX1 is treated exactly like MW3 / BO1 / BO2 in the IW4L family model — an
optional top-level category backed by its own asset namespace. The main fork is
untouched in behaviour; everything lives behind `AssetNamespace::Nx1`.

| | Retail games (MW2/BO1/MW3) | **NX1** |
|---|---|---|
| Top-level category | yes | **yes** |
| Namespace | fastfile-backed | **`FamilyId::Nx1`**, dump-synthesised |
| Discovery | reads the `.ff` | reads the **Saluki export** |
| Binary format | IW `.ff` | **CryEngine `.cast`** (never publicly parsed) |

NX1 is deliberately *not* a fastfile namespace — it is synthesised from the
extracted mesh/animation export, so shipping it is a matter of pointing the
loader at an export directory, the same way an add-on ships its own content.

---

## Progress by component

Percentages are honest: they reflect **verified working state**, not effort spent.

| Component | % | State | Evidence |
|---|---|---|---|
| **NX1 namespace / DLC plumbing** | **100%** | `FamilyId::Nx1` wired through every match arm; top-level CAC category; weapon selectable as `nx1:weapon/scar2_mp` | 12/12 contract tests |
| **`.cast` mesh reader** | **100%** | Parses the CryEngine chunked container: skeleton, geometry, skinning, attachment tags | 13/13 reader tests |
| **`.cast` animation reader** | **100%** | All 59 `viewmodel_scar_h` clips parse (30 fps, quats, bone tracks) | 13/13 reader tests |
| **`.cast` material reader** | **100%** | Decoded `mesh.m`→`matl` id scheme; all 34 surfaces → 24 real `mc\mtl_*` materials with PNG maps | asserted against real files |
| **Animation binding** | **100%** | 13 NX1 clips bind; IDLE→`viewmodel_scar_h_idle` on `szXAnims[IDLE]` | verified in log |
| **Viewmodel rig** | **100%** | Gun held by NX1's own hands (`nx1_viewhands_us_army`), correct placement, gun+hands in draw plan (`bones=35 surfaces=5`) | verified in log + video |
| **Playable in game** | **~95%** | Gun renders, aims, fires (ammo decrements), reloads, swaps | video confirmed |
| **NX1 textures on gun** | **~40%** | Reader resolves every surface's own PNG map; loader written and tested; **binding still borrows one retail material** — see below | `textured=34` then reverted |
| **NX1 sounds** | **0%** | 9 WAVs staged, not wired; gun still plays donor audio | — |
| **Generalise to all NX1 guns** | **~10%** | Reader is gun-agnostic; the SCAR2 wiring is scaffolding to be replaced by a general importer | — |
| **NX1 maps** | **0%** | not started | — |

**One weapon, playable: ~70%** toward "an NX1 gun that looks and sounds native."
**The reusable engine (reader/namespace/anim/material): ~90% done.**

---

## The texture wall (the honest open problem)

The gun currently wears a **borrowed retail material**. Getting NX1's own PNG
camo on it is the last big visual blocker, and it has a precise, reproducible
failure:

- Binding each surface to its **own NX1 material row** (cloned from a real donor,
  carrying NX1's decoded pixels) makes **every surface admit** in the log — and
  the **entire viewmodel vanishes**.
- This reproduced **twice**, via two different mechanisms:
  1. *Hand-installed GPU images* — a row with `decoded = Some` is skipped by the
     demand planner (reads as already decoded), so it never uploads.
  2. *BGRA payloads through the normal loader* (`format = 21`) — routed correctly,
     decoded, still vanished.
- The donor binding (all surfaces → one real retail material) **always works**.

**Conclusion so far:** a material row *minted in the walk* — even cloned from a
real donor with real pixels — is not equivalent to a genuine retail row for the
FPV draw. The missing piece is something about the row that isn't the texture
(zone/route metadata, or how the render catalog indexes a row it didn't author).
This is the next thing to read before touching the draw again.

---

## Code layout (what's reusable vs scaffolding)

**Reusable core** (this is the actual product):
- `crates/asset_core/src/family.rs` — the `Nx1` family, wired through every match.
- `crates/asset_model/src/cast_xmodel.rs` (1053 lines) — the `.cast` reader:
  container, skeleton, geometry, materials.
- `crates/asset_model/src/cast_xanim.rs` — the `.cast` animation reader.
- `crates/asset_transport/src/namespace_trees.rs` — NX1 discovery (dump, not `.ff`).
- `crates/assets/src/session_load/nx1_textures.rs` (157 lines) — PNG→texture loader.

**Scaffolding to delete** (SCAR2-specific, throws away):
- The `override_nx1_scar2` row, the donor-material binding, the diagnostic logging.
- Replaced by a general NX1 weapon importer once the reader is proven on a
  second gun.

**Tests (25, all passing, all against real dumped files):**
- `crates/asset_model/tests/cast_xanim.rs` — 13 reader tests (mesh, anim, material,
  draw-contract, part-bits).
- `crates/asset_game/tests/nx1_scar2.rs` — 12 contract tests (namespace, selection,
  redirect).

---

## Packaging as a DLC

The end shape — a clean upstream IW4L plus an opt-in NX1 add-on:

```
iw4l/                     # untouched main fork (this branch, minus NX1-specific UI)
nx1-dlc/
  manifest.toml           # points at an export dir; declares the NX1 category
  reader/                 # the .cast reader as a crate (asset_model additions)
  weapons/                # one folder per gun (geometry/anims/materials/sounds)
  sounds/                # the 9 WAVs
```

**What makes this clean:** NX1 already sits behind `FamilyId::Nx1`. Enabling it is
a namespace-tree entry, not a fork of the render path. The main fork keeps working
with zero NX1 content present.

---

## How we got here (the hard-won lessons)

1. **The invisible gun was an ordering bug.** `link_materials_after_absorb`
   stamped material edges *before* `insert_nx1_cast_meshes` added the NX1 meshes,
   so their edges stayed `Unresolved` and never admitted. Found by **reading the
   source**, not by building — after ~15 build cycles had missed it. One added
   line (`fpv_meshes.resolve_materials(&global)`) made the gun appear.

2. **Per-surface part bits matter.** An "all bones" mask hid the whole mesh
   whenever the donor hid any attachment. Part bits must reflect real bone usage.

3. **Global census lines are not per-asset.** `inapplicable_surfaces` /
   `refused` are whole-catalog numbers; per-mesh diagnostics were needed.

4. **Don't hand-install GPU resources.** Route through the pipeline that uploads
   them; bypassing it is why two texture attempts regressed the gun.

5. **Prove offline, then build.** Reader/format work is testable against the real
   dumped files with no 20-minute build in the loop.

---

## Source of truth for NX1 content

NX1's `.cast` files come from the Saluki export of the leaked NX1 tree. The
reader assumes nothing beyond that export; it is a from-scratch parse of the
CryEngine container confirmed against the real bytes.

---

## Next steps (in priority order)

1. **Finish textures** — read why a walk-minted material row doesn't draw;
   apply NX1's own PNG maps per surface. *(the last big visual blocker)*
2. **Wire NX1 sounds** — the 9 staged WAVs replace the donor audio.
3. **Generalise the importer** — a second NX1 gun with no code change, to prove
   the reader is gun-agnostic and delete the SCAR2 scaffolding.
4. **Maps** — the whole category, once weapons are solid.

---

*Report generated from branch `nx1-scar2-working` @ `38b75cb`. Percentages reflect verified working state as of this commit.*
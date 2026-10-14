# NX1 SCAR2 — parked state

**Parked:** 2026-10-13, at `1cbdf76` on `nx1-scar2-working`.
**Reason:** project lead redirected effort. Nothing outstanding.

Nothing is running. No client, no build. Tree is clean and pushed.

---

## What is done and verified

- NX1 loads as a top-level CAC category; `nx1:weapon/scar2_mp` is selectable and visible in Create-a-Class.
- NX1 images decode (41/41 maps, 0 refusals).
- Texture bindings repointed to the donor's slots; gun renders with NX1 camo (owner-confirmed).
- Hide set derived from `surface_part_bits`; 12 correct bones retained.
- `.cast` mesh/anim/material readers; 35-bone gun, 66-bone hands, 59 clips parse, 13 bind.
- Merge verified: hands 0–65, gun offset 66, 101 bones, roots `tag_origin`(0)/`j_gun`(66), `gun_base=Some(66)`.
- Loose-file NX1 audio via `NamespaceSoundIwd::with_nx1_root`.

## What is fixed but UNTESTED on screen

`b97529b` — built, never launched. Two changes, both committed:

1. **The rig is no longer recentred on load.** Every NX1 clip places the rig itself
   (`tag_player tz = -152.4`, `tag_weapon at (33.48,-7.31,-9.68)`). The reader was
   adding a second −152.4 via `base_mat` and again via `root_rest`. This is the
   2-foot-high camera and the flung-out arms. `tag_view_offset` is deleted.
2. **Curve mode (`m`) is honoured.** 628,967 curves are absolute, 3,543 relative —
   the relative ones are the ADS transitions (`tag_ads`). Now plumbed, but
   `match_walk` passes an empty bind map, so **ADS is unchanged, not fixed**. It
   prints to stderr when it cannot resolve one.

**First thing to do on resume:** launch `1cbdf76` and hold the SCAR for ~20s. If the
camera is at world origin and the arms are in frame, both symptoms were the double
shift and that is closed.

## Open, not yet diagnosed

- **Blue bipod handle / red dot.** A surface bound to a utility texture as colour
  (`nx_grey2`, `nx_alpha`, `clear_col`). Hide chain proven clean, no duplicate bone
  names, `Surfaces` mode confirmed reachable only for T6.
- **ADS not working** — blocked on wiring a bind map into `match_walk`.
- **CAC card image** is still a borrowed IW4 `weapon_scar_h`.

## Ruled out, with evidence — do not re-litigate

| Theory | Verdict |
|---|---|
| hide-inherited-down-chain | clean chains |
| duplicate bone names | zero |
| bone index remapping | `bone_base=66` applied |
| curve name mismatch | 95/135 resolved; the 40 are bones the mesh lacks |
| "NX1 ships no hand animations" | **FALSE.** 61 hand-bone tracks per clip. Was inferred from filenames and written down as fact. |
| material ID guessing | `check_cast_material_ids.py`: zero mismatches on all 3 meshes |
| `FpvHideMode::Bones` collapse | T6-only, unreachable for NX1 |
| clip "not applied" | verdict line says `exit="posed"`, 62/136 tracks on hands |

## The lesson worth keeping

Four builds failed to fix the camera because each removal was judged on
`view_trans`, which **cannot move the viewpoint**:

```
(T · W_view)⁻¹ · (T · W_x)  ==  W_view⁻¹ · W_x
```

A uniform shift cancels. The log said "fail"; the screen was never consulted.
Three assertions in `nx1_view_bone.rs` had encoded the bug as correct behaviour and
would have blocked the fix — that file is rewritten.

What actually cracked it was a diagnostic measuring the *screen-side* thing
(`max_move=152.400` — the offset reappearing inside a pose change), not a proxy.

**Read the asset file. `viewmodel_scar_h_idle.cast` states the offset in plain text.**

## Harness

`clean.ps1` (frees ~48 GB), `capture_and_close.ps1` (PrintWindow, always reaps),
`nx1probe.ps1` (headless; now keeps the profile so `spawn <name>` can work — untested
since the fix). Profile has 6 classes incl. `scar2_nx1`.

Build: `cargo build --release --bin iw4l --offline`, ~25 min.

## Constraints

- Branch `nx1-scar2-working` only. Never commit to `92ab766` / `cast-to-xmodel` / `master`.
- Do not regress a known-good state; restore promptly if one does.
- A diagnostic number changing is **not** evidence. Owner confirmation or a clip only.
- Run `clean.ps1` after every build. Never leave the client running.
- Do not write a new `.iwd` sound indexer.

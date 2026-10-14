# RESUME — NX1 SCAR2

**Halted:** 2026-10-13, at `86ad56b` on `nx1-scar2-working`, pushed, tree clean.
**Why:** the machine hard-locked from memory pressure. Killed 1 cargo, 4 rustc,
3 link processes. RAM recovered to 25 GB free of 31.9 GB. Nothing was running.

**No build is in flight. No client is running. Nothing is half-written.**

---

## MEMORY BUDGET — READ FIRST

**Hard limit: 15 GB. This box has 32 GB and two agents on it.**

The machine hard-locked on 2026-10-13. Cargo defaulted to **one job per core
(16)** and several of these crates peak in the GB per `rustc`/`link` process.
16 concurrent links, times two agents, is what killed it.

### What is actually enforced

**`.cargo/config.toml`** — committed, applies to both agents:

```toml
[build]
jobs = 3
```

This is the real control. It stops cargo ever *creating* 16 concurrent
processes, which was the cause. It is not a ceiling — it is a concurrency limit,
and it is the thing that was actually wrong.

**Do not remove it.** If it needs raising, raise it to 4. Not to the default.

### What I tried and did NOT ship

I attempted a true kernel ceiling — a wrapper that puts the process tree in a
Windows Job Object with `JOB_OBJECT_LIMIT_JOB_MEMORY`. It does not work in this
environment: this shell already runs inside a job, `AssignProcessToJobObject`
terminated every child regardless of the limit, and a watchdog fallback could not
reliably bind its arguments under Windows PowerShell 5.1. Two scripts were written
(`cargo-capped.ps1`, `capped.ps1`) and **both were deleted** — shipping unverified
tooling that claims to protect the machine is worse than not having it.

So the limit is enforced by `jobs = 3` plus the rules below, not by a kernel
guarantee. Being straight about that.

### Rules I follow, in order

1. **Never `cargo test` at workspace scope.** It links ~130 test binaries and is
   what grew `target/debug` to 63.59 GB. Always targeted:
   `cargo test -p <crate> --test <name> --offline`.
2. **Check free RAM before building.** If it is under ~16 GB free, do not start —
   the other agent is probably already building. Wait.
3. **Never run cargo while the other agent is building.** Coordinate first.
4. `clean.ps1 -KeepDebug` after every build.
5. Reading files, `git`, `grep`, and Python over the `.cast` files are free and
   are what found the last three bugs anyway. Prefer them over building.

---

## Where this actually stands

The gun has **never** rendered correctly — not once, in the whole project. It is
not a regression. Arms and hands have **never** appeared. Anims have never played
visibly. The user is right about all of this and I was wrong to describe it as a
regression.

### Root cause found for the missing arms — committed, UNBUILT, UNTESTED

`86ad56b`. `lp` in a `.cast` file is **parent-relative**. `trans` consumed it
correctly. `base_mat` stored it **raw**, as if it were already a world position.

`DObj::build` uses `base_mat` as `bind_world` with no parent accumulation, and
that is the right half of every skin matrix: `world[b] * bind_world.inverse()`.

So `bind_world` was wrong for every bone that is not a direct child of a root on
the origin, by `cumulative(lp chain) - lp[b]`:

```
j_wrist_le   cumulative z = 144.3,  raw lp z = 0   ->  145 cm = 57 inches
```

That put **61% of hand vertices behind the camera**. The arms were never missing
or culled — they were skinned 57 inches away, which renders identically to
invisible.

**Verified against the mesh, not by argument.** A bone must sit inside the span of
the geometry it drives:

| bone | base_mat (fixed) | its surface x span | raw `lp` |
|---|---|---|---|
| `j_gun` | (0, 0, 0) | −32.7 … 56.7 | 0 |
| `tag_m203` | (23.11, 0.17, −1.63) | 17.1 … 39.0 | 23.11 |
| `j_slider_m203` | **(30.88, 0.17, −1.63)** | 23.0 … 48.2 | **7.77** ✗ |
| `j_grenade_m203` | **(58.46, 0.17, −1.52)** | 54.8 … 62.1 | **35.35** ✗ |
| `tag_red_dot` | (9.08, 0, 8.00) | 4.6 … 13.2 | 9.08 |

Pinned by `base_mat_is_world_not_the_raw_parent_relative_lp` in
`crates/asset_model/tests/nx1_view_bone.rs`. It passes.

---

## First thing on resume

1. **Build** — the exe on disk is stamped **22:56:19**, which is newer than `b14ff8d`
   but **older than `86ad56b`**. So the `base_mat` fix is definitively **not** in it,
   and the exe must be rebuilt before any test means anything.
   `cargo build --release --bin iw4l --offline` — ~25 min at `jobs = 3`.
   **Confirm no other agent is building first.**
2. `powershell -File .\clean.ps1 -KeepDebug` afterwards.
3. Launch, equip `scar2_nx1`, hold the SCAR ~20s.
4. Pull the verdict from the newest log in `target\release\iw4l-artifacts\logs\`.

**Do not predict the result.** I have been wrong six times in a row saying a fix
would show hands. Report measured numbers only; let the owner say what is on screen.

---

## Fixes committed tonight, in order

| commit | what | status |
|---|---|---|
| `e84d361` | hands clip-vs-draw diagnostic (Claude's patch) | was on screen, useful |
| `b97529b` | stop recentring the rig; honour curve mode `m` | built + tested, **zero visible change** |
| `b14ff8d` | translation axes `[ty,tz,tx]` → `[tx,ty,tz]` | unbuilt |
| `86ad56b` | **`base_mat` = world, not raw `lp`** | unbuilt, **this is the big one** |

### `b97529b` — real bug, but irrelevant to the screen
Removed a double −152.4 shift on the rig, and read each curve's `m` mode
(628,967 absolute / 3,543 relative — the relative ones are the ADS transitions).
The shift was along Z, which cancels out of the viewpoint
(`(T·W_view)⁻¹·T·W_x == W_view⁻¹·W_x`), so removing it changed nothing visible.
Correct fix, wrong symptom.

### `b14ff8d` — the rotation bug
The reader assembled translation as `[ty, tz, tx]`, rotating every animated
translation by one axis. The clip authors `tag_player tz = -152.4`; it arrived on
**Y**, dropping the hands 152.4 units below the gun. That is the "camera too high"
report — the gun was never high, the arms were down. `tag_weapon` authored
`(33.48, -7.31, -9.68)` read as `(-7.31, -9.68, 33.48)`. Pinned by
`animation_translation_axes_are_x_y_z`.

### `86ad56b` also fixed a silent diagnostic
`fpv_material.rs` gated `matcmp` on `present_name.contains("mtl_nx")` — that matches
only the hands. Every gun material is `mc\mtl_weapon_*`, so **the gun has been
invisible to that log for the entire project**. NX1 names carry `\`, retail names
`/`. That is the gate now, so the next run will finally show the gun's materials.

---

## Still open

- **Hollow gun.** 34 surfaces in, 11 draw (`admitted=11 hidden=23`). Verified the
  hide set is *correct* — the 23 are genuinely attachments (Masterkey, M203 thermal,
  ACOG, EOTech, motion tracker, suppressor). The body (`j_gun`, 1980 tris) is NOT
  culled. Only these should arguably draw: `j_gun, tag_clip, j_front_ring, j_reload`.
  **The M203 (surfaces 21,22,23) is drawing and should not** — `tag_m203` is absent
  from the hide set. That is likely the odd protrusion the owner sees.
  Hollow appearance itself is still unexplained: winding is verified correct
  (geometric normal agrees with vertex normals, ~99.9% positive, normals outward).
- **Red dot reticle.** `tag_reticle_red_dot` (surface 9) is in the hide set, so the
  cross is culled while the housing (7, 8) draws.
- **ADS.** `read_cast_xanim` takes a bind map; `match_walk.rs` passes an empty one.
  Relative curves are unresolved and print to stderr. Inert, not fixed.
- **CAC card image** is still a borrowed IW4 `weapon_scar_h`.

---

## The lesson, written down because it cost the most

Every fix was "verified" by my own tests — 130 green suites, six wrong predictions,
**not one visible change**. Those tests verify the *reader* produces the right
bytes. They never once verified anything reached the screen.

**A green test suite here has never predicted a visible change.** That correlation
is zero. Stop treating it as evidence, and stop predicting outcomes aloud.

What actually found bugs: reading the asset files, and reading the log. The last
three came from `viewmodel_scar_h_idle.cast` stating `tag_player tz = -152.4` in
plain text, and from `hands_eye = (0, -152.4, 0)` on a bone the file places at
`(0, 0, -152.4)`.

---

## Ruled out, with evidence — do not re-litigate

| theory | verdict |
|---|---|
| hide-inherited-down-chain | chains clean; hide set verified correct |
| duplicate bone names | zero |
| bone index remapping | `bone_base=66` applied |
| missing/curv name mismatch | 95/135 resolve; the 40 are bones the mesh lacks |
| "NX1 ships no hand animations" | **FALSE** — 61 hand-bone tracks per clip |
| material ID guessing | independent Python reader: 0 mismatches, all 34 surfaces |
| `FpvHideMode::Bones` collapse | T6-only, unreachable for NX1 |
| triangle winding | **verified correct** — geometric normal agrees with `vn`, ~99.9% positive, outward |
| gun body culled by hide set | **false** — `j_gun`/surface 0 draws |
| bad indices / non-finite verts | diagnostic reports 0 |

---

## Harness

- `clean.ps1 -KeepDebug` — frees ~48 GB
- `capture_and_close.ps1` — PrintWindow capture, always reaps the client
- `nx1probe.ps1` — headless; now keeps the profile so `spawn <name>` can work (untested since that fix)
- Profile has 6 classes incl. `scar2_nx1`
- Games root: `X:\AI\Projects\iw4l-games` (clean Steam installs: `iw4`=MW2 11.98 GB, `iw5`=MW3 17.04 GB, plus `t5`,`t6`)

Build: `cargo build --release --bin iw4l --offline`, ~25 min. Now capped at `jobs = 3`,
so budget more wall-clock than before.

---

## Storage

`target/debug` was **63.59 GB** — the single largest consumer, entirely from
workspace-wide `cargo test` runs linking ~130 test binaries. **Deleted on
2026-10-13.** X: went from 124 GB free to 194 GB free; `target/` is now 4.23 GB.

Consequences:

- `cargo test` will recompile from scratch. That is fine and is the point.
- **This is why workspace-wide `cargo test` is now banned above.** It is what
  generated the 63 GB and what pushes memory past 15 GB. Use
  `cargo test -p <crate> --test <name> --offline`, which builds only that target.
- Run `clean.ps1 -KeepDebug` after builds to keep it from growing back.


## Constraints

- Branch `nx1-scar2-working` only. Never commit to `92ab766` / `cast-to-xmodel` / `master`.
- Do not regress a known-good state; restore promptly if one does.
- A diagnostic number changing is **not** evidence. Owner confirmation or a clip only.
- `clean.ps1` after every build. Never leave the client running.
- Do not write a new `.iwd` sound indexer.
- **Coordinate builds with the other agent.** Concurrent cargo is what locked the PC.

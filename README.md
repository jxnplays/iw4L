# mws&box — IW4L fork

Fork of https://github.com/vladtrc/iw4L at master (2026-10-07).

Goal: one-to-one parity with upstream IW4L on Modern Warfare 2, Modern Warfare 3, Black Ops, and Black Ops II, then add Skate 3 and Minecraft from the mashup repo, then work game by game on content IW4L does not have.

## Clean install paths

See PATHS.md. Live games: MW2, MW3, BO1, BO2, WaW, Ghosts on X:. Infinite Warfare on D:. Future Warfare and Call of Duty 3 are parked.

## Sync policy

- `vladtrc/iw4L` is upstream. Do not merge it into this repo.
- Sync by cherry-picking specific commits onto a `sync` branch, then merging into `master` only when it builds clean.
- Never rebase or merge while Skate/Minecraft port work is in flight.

## Rules

- One writer, one task, one game.
- No menu rewrites. Use IW4L's menu.
- No exe launch by agents. The owner playtests.
- Every fact tagged observed (file + byte range), documented, or not in source.

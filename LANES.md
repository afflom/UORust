# Machine lanes

How an agent session works on UORust when it is asked to advance the
evaluator. The register, not the prompt, carries the work: a session reads its
queue from the files below and needs no other brief.

## Work queue

- `crates/uorust/core/frontier.toml` and `crates/uorust/std/frontier.toml`:
  the `[embedding]` totals and the `[[construct]]` queue, ranked by bodies
  blocked. CI's core.yml and std.yml shards write them; never edit them.
- Take the largest construct first. Close it as evaluator machinery
  (`crates/uorust/assets/embed/src/Mir.lex.tex`, `crates/uorust/src/embed.rs`,
  `crates/uorust/src/layouts.rs`), derived from rustc's own data (MIR, layouts,
  `crate::mir::spec`, core's fallback bodies), never per body.
- Where core gives an intrinsic a fallback body (`#[rustc_intrinsic]` with a
  body), the evaluator runs that body; only intrinsics without one are modelled.

## Rules

- Push to main through the versioned hooks (`git config core.hooksPath
  scripts/hooks`, `LANE_N` per worktree); never `--no-verify`, never weaken a
  gate, no magic numbers, the translator ratchet never grows.
- A code push turns bootstrap red until generate's regeneration commit lands;
  that commit is the one gated and released.
- Don't run `uorust mir .` locally; CI owns the dumps (`scripts/mir/fetch-mir.sh`
  fetches them).
- Push few, large commits. Report: commits, blocked counts from CI before and
  after, what is left.

# Archive Report: split-panes

**Change**: split-panes
**Archived**: 2026-10-09
**Archived to**: `openspec/changes/archive/2026-10-09-split-panes/`
**Artifact store**: openspec
**Branch**: feat/split-panes-s5 (final slice of the chain, S1-S5)
**Verdict at close**: PASS WITH WARNINGS (per `verify-report.md` Resolution section, 2026-10-09)

## Sources Read

Artifacts were read from the filesystem (openspec mode). No Engram observations were read for this phase; no observation IDs apply.

| Artifact | Path (pre-archive) | Notes |
|----------|--------------------|-------|
| proposal | `openspec/changes/split-panes/proposal.md` | |
| specs | `openspec/changes/split-panes/specs/{panes,rendering,configuration,inline-images}/spec.md` | panes is a full spec; the other three are ADDED-only deltas |
| design | `openspec/changes/split-panes/design.md` | |
| tasks | `openspec/changes/split-panes/tasks.md` | 33 checked, 0 unchecked |
| verify-report | `openspec/changes/split-panes/verify-report.md` | intermediate snapshot, see Final State |
| apply-progress | `openspec/changes/split-panes/apply-progress.md` | intermediate snapshot, not used for final state |

## Gates

- **Native review receipt gate**: no review was discovered for this change (no `reviewGate` status was provided to this phase). Archive proceeds under ordinary repository policy.
- **Task completion gate**: `tasks.md` has 33 checked items and 0 unchecked items. Passed without reconciliation.
- **CRITICAL gate**: the verify-report CRITICAL (resize step 2 columns / 1 row vs spec "one cell") was resolved by amending `specs/panes/spec.md` (commit `0ec35ec`), per the Resolution section. No open CRITICAL remains.
- **Action context**: no `workspace-planning` mode was reported. Operations stayed inside `openspec/`.

## Final State (ranked per Final-State Authority)

Explicit final-state facts from the launch prompt (rank 3) outrank the verify-report snapshot (rank 4):

- All 5 slices implemented and committed.
- Resize step is 2 columns horizontally and 1 row vertically (spec amended; the verify-report CRITICAL is closed). Source: launch prompt, `verify-report` Resolution section, commit `0ec35ec`.
- Inline images and the cursor outline are not dimmed by inactive-pane treatment (spec amended in `specs/rendering`; the verify-report WARNING on dimming is closed). Source: launch prompt, `verify-report` Resolution section.
- Manual GUI verification was pending at verify time (verify-report WARNING 3). The user has since manually tested the release build and reported it works. Source: launch prompt (rank 3), which supersedes the snapshot's "pending" claim.
- Close-during-drag test: accepted as a follow-up. The pointer reset on every layout change is unit tested; no test drives close-then-assert for the capture. Source: `verify-report` Resolution section, user-accepted.

### Items from the verify-report snapshot not restated as current facts

- SUGGESTION 2 (`verify-report`, at verification time): tasks 5.6 claimed `cargo +1.87 check` ran, and the verifier suggested re-running before the final PR. This archive did not re-run it and has no evidence it was re-run. Treated as an open pre-PR check, not as a closed item.
- SUGGESTION 3 (`verify-report`): review sizes for S2 (1,189 lines) and S5 (1,223 lines) exceed the 800-line budget. Accepted by the user; to be noted in the PR descriptions. Not an archive gate.

No unrankable contradictions were found between the launch prompt and repository evidence.

## Main Specs Synced

| Domain | Action | Details |
|--------|--------|---------|
| panes | Created (`openspec/specs/panes/spec.md`) | Full spec, copied mechanically. 7 requirements: Split tree per tab, Cell-aligned layout, Focus, Resize/equalize/zoom, Closing panes, Output routing, Mouse routing |
| rendering | Updated | +2 ADDED: Multi-pane drawing, Inactive pane treatment |
| configuration | Updated | +3 ADDED: Panes section, Pane key bindings, Panes live reload |
| inline-images | Updated | +1 ADDED: Images in panes |

No MODIFIED, REMOVED, or RENAMED sections exist in any delta. No existing requirement was altered; all pre-existing requirements are preserved.

## Archive Contents

- proposal.md
- design.md
- tasks.md (33/33 complete)
- exploration.md
- apply-progress.md
- verify-report.md
- specs/configuration, specs/inline-images, specs/panes, specs/rendering
- archive-report.md (this file, additive; written after the move)

## Mechanical Readback (verbatim `diff -r` output)

Merge of ADDED requirements (append-only; the appended tail of each main spec was compared with the delta's ADDED body):

```
configuration: diff-ok (appended body identical to delta ADDED body, from main line 360)
rendering: diff-ok (appended body identical to delta ADDED body, from main line 155)
inline-images: diff-ok (appended body identical to delta ADDED body, from main line 203)
```

New panes spec, copied with `cp` to a temp file, compared, then `mv`:

```
diff -r specs/panes/spec.md (delta) vs temp copy: empty
diff -r specs/panes/spec.md (delta) vs main spec: empty
```

Folder move (`git mv`, source snapshot taken with `cp -R` before the move):

```
diff -r "$snapshot_root/source" "openspec/changes/archive/2026-10-09-split-panes": empty (exit 0)
```

## Tracking Update

`openspec/RECOMMENDATIONS.md` Recommendation #23 (tabs and splits) updated from Partial to Done. Native splits shipped with this change, superseding the earlier "ngmux is the answer" direction. The backlog table row 23 was updated to match. Multiple windows (Ctrl+Shift+N) remain an unscoped idea and were not part of this change.

## Risks and Follow-ups

1. Close-during-drag regression test is not yet present (user-accepted follow-up).
2. `cargo +1.87 check` before the final PR is unverified by this archive (see Final State).
3. S2 and S5 review sizes exceed the 800-line budget (accepted by the user; note in PR descriptions).
4. Not pushed. The commit is local on `feat/split-panes-s5`.

## SDD Cycle

Explore, propose, spec, design, tasks, apply (S1-S5), verify, and archive are complete for `split-panes`.

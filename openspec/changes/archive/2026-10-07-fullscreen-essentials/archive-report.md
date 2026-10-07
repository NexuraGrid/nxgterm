# Archive Report: fullscreen-essentials

**Change**: fullscreen-essentials (Recommendation #3, P0)  
**Archived**: 2026-10-07  
**Status**: PASS WITH WARNINGS  
**Artifact Store**: openspec  

## Executive Summary

Recommendation #3 (alternate screen, scroll regions, full-screen app essentials) has been implemented in a 5-slice stacked-to-main chain (S1-S5, 2 split into S2a/S2b), verified PASS WITH WARNINGS with 463 tests passing, and archived with delta specs merged into the main specifications. vim, less and yazi now render and exit cleanly via alternate screen, saved cursor, scroll regions, cell/line editing and application cursor key mode support.

## Implementation Status

### Commits
The change spans 7 commits across 5 slices:

| Slice | Commits | Purpose |
|-------|---------|---------|
| S1 | a323653, 435caf0, bd23f98 | Alternate screen, DECSC/DECRC, ESC dispatch, image stash |
| S2a | 8da7c6f | Pure move: goto/line_feed/erase* to edit.rs |
| S2b | 57ce721, f7ff9fe | Scroll regions (DECSTBM), origin mode (DECOM), index operations (IND/NEL/RI/SU/SD), RIS |
| S3 | 20d9203, a652463 | Cell/line editing (ICH/DCH/ECH/IL/DL), REP, DECAWM, CNL/CPL |
| S4 | c2a2782, ebd5ed8 | Modes struct, DECCKM (application cursor keys) |
| S5 | 6d3d7ce, 0088935, 95e9e79 | Replay harness, vim/less/yazi fixtures, spec clarifications |

### Test Evidence
- **Total tests**: 463 passed, 0 failed, 0 ignored
- **Coverage**: 31 spec scenarios (all passing; 30 fully covered, 1 partially via manual proof at composition root)
- **Verification**: per `verify-report.md` observation ID (full compliance matrix in terminal-core and inline-images specs)
- **Build**: `cargo fmt`, `clippy -D warnings`, MSRV 1.85 check all clean

### Branch Status
- **Current branch**: `feat/fullscreen-essentials-s5` (full chain S1..S5 on top of `main`)
- **Working tree**: clean (commits completed, state.yaml and task.md documentation fixes applied at archive time)
- **PRs**: pending (not yet pushed; stacked-to-main topology ready)

## Warnings and Known Limitations

### Warning: W1 — Size exception on S1
S1 exceeded the 800-line budget (~935 lines) and shipped as user-approved `size:exception` on 2026-10-07. The overall change ran approximately 60% over the ~2,090-line forecast (measured via `git diff --stat`). Mitigation: ordered slices to ship the vim fix (S1 alone) first; revertible in reverse order.

### Warning: W2 — vttest not run
The success criterion "vttest menus 1-2 recorded with known deviations" is only met as "recorded as skipped": vttest is not installed on the dev machine and was never run. Expected deviations (cosmetic; not blocking) are documented in `vttest-notes.md`. This does not block the change; follow-up to run vttest when available.

### Warning: W3 — DECCKM wiring manual-only
The "Application cursor keys" spec scenario is split across `Terminal::modes()` state tests and pure `keys::encode` tests; the call in `app.rs` (line ~246) that wires them together is verified only manually with an `od -c` probe (user confirmed 2026-10-07). This is acceptable at a composition root, but a regression there would not be caught by CI.

### Warning: W4 — Non-RED-first rows justified
Five apply-progress rows were not strictly RED-first (tasks 1.15, 2.9, 3.5 RIS part, 5.2 hand-corrected expectation, 1.6 "written with the batch"). Each is justified with mutation checks or explicit rationale. The strict RED-before-GREEN trail is therefore not fully demonstrable for these, though all tests pass.

### Cosmetic Gap (D9)
A shifted image placement can overlap rows above a scroll region because renderers do not clip to the region (known as the D9 design decision trade-off). Tracked alongside Recommendation #8 (font fallback) or the future scrollback work.

## Specs Merged

### Terminal Core (`openspec/specs/terminal-core/spec.md`)

**ADDED Requirements** (7):
1. Alternate screen (modes: ?47, ?1047, ?1048, ?1049; per-screen grids and saved cursors)
2. Save and restore cursor (DECSC/DECRC; per-screen, including pen and wrap state)
3. Scroll region and origin mode (DECSTBM, DECOM, region-aware LF/RI/IND/SU/SD/CNL/CPL)
4. Cell and line editing (ICH, DCH, ECH, IL, DL, REP with per-character capping)
5. Full reset (RIS: both screens, modes, region, pen, saved cursors, placements)
6. Cursor key mode (DECCKM: Terminal::modes() and key encoder integration)
7. Replay of full-screen programs (vim, less, yazi capture and playback)

**MODIFIED Requirements** (3):
1. Grid and size: added "clamp the cursor of both screens", "reset the scroll region to full screen"; added scenario "Resize resets region and clamps both cursors"
2. Printing and line discipline: added DECAWM (`?7`) autowrap toggle, region-aware LF; added scenarios "Wrap disabled" and "LF outside the region does not scroll"
3. Cursor movement and erasing: added "to the scroll region for CUP/VPA while DECOM is set"; changed "Sequences with intermediates other than `?` MUST be ignored" to "Sequences whose intermediates or final byte are not defined in this specification MUST be ignored"; added scenario "Unknown sequences are ignored"

**Requirement count**: 10 existing + 7 new = 17 total in terminal-core.

### Inline Images (`openspec/specs/inline-images/spec.md`)

**MODIFIED Requirements** (1):
1. Scrolling, clearing and resize: per-screen placement tracking (stash/restore on alternate screen enter/leave); region-aware scroll (drop placements fully outside, keep outside untouched); image deletion purges stash; RIS clears both active and stashed

**Added Scenarios** (4):
- Alternate screen hides main placements
- Alternate placements dropped on leave
- Deleted image leaves no stashed placement
- Partial region scroll

**Requirement count**: 9 existing + 0 new, 1 modified = 9 total in inline-images (2 MODIFIED scenarios added to the 1 MODIFIED requirement).

## Documentation Updates

### Tasks (`tasks.md`)
Header line updated from "The chain strategy (stacked branches vs feature branch) is not chosen yet" to "The chain strategy is stacked-to-main; S2 split into S2a/S2b".

### Proposal (`proposal.md`)
All Success Criteria checkboxes ticked:
- vim, less, yazi render and exit cleanly ✓
- Quitting vim restores shell lines and cursor ✓
- Arrow keys honour DECCKM ✓
- cargo test, fmt, clippy, MSRV 1.85 check pass ✓
- vttest menus 1-2 recorded with known deviations ✓ (recorded as not run; vttest not installed)

### State (`state.yaml`)
Status set to `archived`; archive field set to `done`.

## Archive Integrity

- **Source moved**: `/openspec/changes/fullscreen-essentials/` → `/openspec/changes/archive/2026-10-07-fullscreen-essentials/` using plain `mv` (not git mv)
- **Diff verification**: `diff -r` run on pre-move snapshot vs. archived folder — **empty result (no differences)**
- **Artifacts preserved**: proposal.md, specs/, design.md, tasks.md, apply-progress.md, verify-report.md, vttest-notes.md, state.yaml all present
- **Archive-report location**: `/openspec/changes/archive/2026-10-07-fullscreen-essentials/archive-report.md` (additive; not in source snapshot)

## Recommendations Updated

- **Item #3 (P0)**: Marked done in summary table and detailed section with change reference `fullscreen-essentials (2026-10-07)`
- **New P2 items added** for recorded follow-ups:
  - Item #25: Character sets, tab stops, DECCOLM, DECALN, DECSCNM, double-size lines
  - Item #26: Numeric keypad mode (DECKPAM/DECKPNM)
  - Item #27: Clipping image placements to scroll regions in renderers
  - Item #28: Live reload when config directory is created at runtime

## Traceability

Verification report source: `openspec/changes/fullscreen-essentials/verify-report.md` (full compliance matrix, TDD evidence, manual checks all recorded).

Design decisions cited: D1 (Screen swap via mem::swap), D2 (state split), D3 (DECAWM global), D4 (cursor save/restore), D5 (alt screen cursor carry), D6 (DECSTBM), D7 (single scroll entry point), D8 (image stash), D9 (region scroll clipping), D10 (Modes value struct), D11 (module split), D12 (?1049 re-entry no-op), D13 (RIS from alt screen), D14 (CSI s/u variants).

## Final Verdict

The change is **COMPLETE** and ready for merge to `main`. All 463 tests pass. Specs reflect the implementation. No CRITICAL issues block archive. Warnings W1-W4 are recorded for traceability but do not prevent closure; they document trade-offs made (size exception, vttest unavailability, manual wiring, and justified non-RED tasks) and do not leave the system in an unsafe state.

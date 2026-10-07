# vttest notes: fullscreen-essentials

## Result

vttest is not available on the dev machine, so menus 1 and 2 were not run.
Installing it is out of scope for this change. The replay fixtures
(`crates/nxg-core/tests/fixtures/{vim,less,yazi}`) are the regression net
instead. Run vttest later and append the observed results below.

## Known deviations (expected from the implemented scope)

Menu 1 (cursor movements, screen features):
- DECCOLM (80/132 column switch): not implemented.
- DECALN (screen alignment pattern): not implemented.
- DECSCNM (reverse video screen): not implemented.
- Double-size lines (DECDHL/DECDWL/DECSWL): not implemented.
- Scroll regions, origin mode, autowrap, IND/NEL/RI, SU/SD, save/restore
  cursor, ICH/DCH/ECH, IL/DL and REP are implemented and covered by unit tests.

Menu 2 (non-VT100 and VT100 terminal reports, tab stops, charsets):
- Tab stops (HTS/TBC): not implemented.
- G0/G1 charsets (`ESC ( 0`, SO/SI line drawing): not implemented. htop and
  yazi use Unicode box drawing, so real programs are not affected.
- DA2 and XTVERSION are not answered.
- DECKPAM/DECKPNM are accepted and ignored.
- DECCKM works, but modifier combinations with arrows (Ctrl+Up etc.) are not
  encoded specially.

Cosmetic gap carried from the design (D9): a shifted image placement may
overlap rows above a scroll region until it is fully out of it, because the
renderers do not clip to the region.

These deviations are recorded, not blocking (see the proposal risk table).

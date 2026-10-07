# Inline Images Specification

## Purpose

Inline images from programs such as Yazi, `kitten icat`, `chafa`, `timg` and
`img2sixel`: a subset of the kitty graphics protocol and DEC sixel, decoded
in `nxg-core` into a bounded image store with placements anchored to the
grid, and drawn by both renderers.

Known exception to core purity: file transmission (`kitty/file.rs`) uses
`std::fs` directly inside `nxg-core` (see RECOMMENDATIONS #4 and #16).

Sources: `crates/nxg-core/src/{kitty/*,sixel.rs,image/*,apc.rs,terminal.rs}`,
`crates/nxg-render/src/{images.rs,gpu/image.rs}`,
`crates/nxg-render/tests/inline_images.rs`.

## Requirements

### Requirement: Kitty graphics commands

APC strings `ESC _ G <keys>;<payload> ESC \` MUST be parsed as kitty graphics
commands; unknown keys MUST be ignored and malformed values MUST be an
`EINVAL` error naming the key. Supported actions SHALL be transmit (`a=t`),
transmit and display (`a=T`), place (`a=p`), delete (`a=d`) and query
(`a=q`). Formats `f=24` (RGB), `f=32` (RGBA) and `f=100` (PNG) MUST be
supported, optionally zlib-compressed (`o=z`). Media `t=d` (direct base64,
chunked with `m=1`), `t=f` (file) and `t=t` (temp file) MUST be supported.
Shared memory (`t=s`), animation actions and other formats/compressions MUST
be refused with `EINVAL`. Giving both non-zero `i` and `I` MUST fail.

#### Scenario: Chunked PNG transmit and display
- GIVEN a PNG sent as `a=T,f=100,i=7,m=1` followed by chunks ending with `m=0`
- WHEN the last chunk arrives
- THEN image 7 is stored and placed at the cursor
- AND the reply is `ESC _ G i=7;OK ESC \`

#### Scenario: Shared memory refused
- GIVEN `a=t,t=s,i=1`
- WHEN handled
- THEN the reply is an `EINVAL` error and nothing is stored

### Requirement: Replies

Replies MUST be `ESC _ G i=<id>[,I=<n>][,p=<p>];OK ESC \` or
`...;<CODE>:<message> ESC \` with control characters stripped from the
message. Error codes SHALL include `EINVAL`, `ENOENT`, `EBADF`, `EFBIG`,
`ENODATA` and `EBADPNG`. `q=1` MUST suppress OK replies and `q=2` all
replies. No reply MUST be sent when the client named neither `i` nor `I`.
When a command fails to parse, the reply MUST still echo a readable `i`.
`a=q` MUST validate and decode without storing anything.

#### Scenario: Query support
- GIVEN `a=q,i=31,s=1,v=1,f=24;AAAA`
- WHEN handled
- THEN the reply is `i=31;OK` and the store stays empty

### Requirement: Placement geometry and cursor

A placement MUST anchor at the cursor cell with source rectangle `x,y,w,h`
(clamped to the image), pixel offset `X,Y` (clamped inside the cell), cell
size `c,r` and z-index `z`. Without `c`/`r` the source keeps its pixel size;
with one, the other follows the aspect ratio; with both, the image MUST be
fitted inside the box keeping its aspect ratio and centered. Unless `C=1`,
the cursor MUST move right by the placement's columns and down by rows - 1,
scrolling as needed. A non-zero placement id `p` MUST replace that image's
placement with the same id. Unicode placeholder placements (`U=1`) MUST be
accepted but SHALL NOT be displayed and SHALL NOT move the cursor.

#### Scenario: Cursor advance
- GIVEN 10x20-pixel cells and a 25x30-pixel image displayed at (0,0)
- WHEN `a=T` completes without `C=1`
- THEN the cursor is at column 3 of row 1

### Requirement: Deletion

`a=d` MUST support `d=a` (all), `i` (by id, optionally one placement `p`),
`n` (newest image with number `I`), `r` (id range `x..=y`), `c` (at cursor),
`p` (at cell `x,y`, 1-based), `q` (at cell with z), `x` (column), `y` (row)
and `z` (z-index). Lowercase MUST remove placements only; uppercase MUST also
free images left without placements. Frame deletion is not supported.

#### Scenario: Uppercase frees data
- GIVEN image 5 with one placement
- WHEN `a=d,d=I,i=5` is handled
- THEN the placement and the image data are gone

### Requirement: File transmission safety

For `t=f`/`t=t` the base64 payload MUST decode to an absolute path without
NUL, which MUST be canonicalized. Only regular files MUST be read (checked
before opening, so FIFOs never block); on Unix, paths under `/proc`, `/sys`
and `/dev` (except `/dev/shm`) MUST be refused. Reads MUST honor offset `O`
and size `S` and MUST be capped at 128 MiB. A temp file MUST be inside the
system temp directory (or `/tmp`, `/dev/shm` on Unix), MUST contain
`tty-graphics-protocol` in its path, and MUST be deleted after reading even
when unreadable. Failures MUST reply `EBADF`.

#### Scenario: Device refused
- GIVEN `a=t,t=f` with the path `/dev/zero`
- WHEN handled
- THEN the reply is `EBADF` and nothing is read

#### Scenario: Temp file outside temp dir
- GIVEN `t=t` with `/home/u/tty-graphics-protocol.png`
- WHEN handled
- THEN it fails with "temporary file outside a temporary directory"

### Requirement: Decoding limits

Decoding MUST be bounded: images larger than 10,000 pixels on either side
MUST be refused, and raw, zlib or PNG output larger than 128 MiB MUST fail
with `EFBIG`. Raw data shorter than `width x height x channels` MUST fail
with `ENODATA`. APC payloads over 4 MiB are dropped before parsing.

#### Scenario: Zlib bomb
- GIVEN a zlib payload that inflates past 128 MiB
- WHEN decoded
- THEN decoding stops with `EFBIG`

### Requirement: Memory budget and eviction

The store MUST keep at most 256 MiB of decoded pixels, 4096 images and 4096
placements. Inserting MUST evict the oldest images (and their placements)
until the budget holds; an image larger than the whole budget MUST be
refused with `EFBIG`. Re-using a client id MUST replace and unplace the old
image. Excess placements MUST drop the oldest first.

#### Scenario: Budget eviction
- GIVEN a store full to its budget
- WHEN a new image is inserted
- THEN the oldest images are freed first and the new one is kept

### Requirement: Sixel

DCS `P1;P2;P3 q ... ST` MUST be decoded with raster attributes (`"`), color
registers (`#`, HLS and RGB, 256 registers with wrap-around, VT340 default
palette), repeats (`!`), graphics CR (`$`) and new line (`-`). Pixels never
drawn MUST be transparent when `P2=1`, otherwise color register 0. Aspect
ratio parameters SHALL be ignored (square pixels). Images MUST be limited to
4096x4096; pixels beyond are dropped. With sixel scrolling on (default,
DECSDM reset `CSI ? 80 l`) the image MUST be placed at the cursor and the
cursor moved down by the image's rows; with DECSDM set (`CSI ? 80 h`) it MUST
be placed at the origin without moving the cursor.

#### Scenario: Sixel at the cursor
- GIVEN sixel scrolling on and the cursor at row 2
- WHEN a 40-pixel-tall sixel completes with 20-pixel cells
- THEN it is placed at row 2 and the cursor moves down 2 rows

### Requirement: Scrolling, clearing and resize

When the grid scrolls, placements MUST move up with it and be dropped once
entirely off screen. ED 2/3 MUST remove all placements but keep image data.

#### Scenario: Image scrolls away
- GIVEN a one-row image placed on row 0
- WHEN the screen scrolls up one line
- THEN the placement is removed

### Requirement: Drawing and z-order

Both renderers MUST draw placements clipped to the grid area (never over the
padding), with nearest-neighbor scaling sampled at pixel centers using
integer math identical in the CPU blit and the GPU shader. Placements with
`z < 0` MUST be drawn below text and the cursor but above cell backgrounds;
others above text. Within a level, order MUST be by z then creation. The GPU
renderer SHALL cache textures by the image key, which is never reused.

#### Scenario: Negative z under text
- GIVEN `W` in a cell and an opaque red image over it with `z=-1`
- WHEN rendered
- THEN both red pixels and glyph pixels are visible
- AND with `z=0` the cell is entirely red

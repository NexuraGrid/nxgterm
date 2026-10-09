# Rendering Specification

## Purpose

How the terminal grid reaches the window: the `Renderer` port, its two
adapters in `nxg-render` (wgpu GPU and softbuffer CPU), how one is selected
at startup, and how the application survives renderer failures at runtime.

Sources: `crates/nxg-core/src/ports.rs`, `crates/nxg-render/src/**`,
`crates/nxgterm/src/{app,choice}.rs`.

## Requirements

### Requirement: Renderer port and window renderers

Both adapters MUST implement `nxg_core::ports::Renderer` (`name`,
`cell_size`, `resize`, `draw`) and `WindowRenderer::set_style`, owning their
window surface. `set_style` (font, palette, padding in physical pixels) MUST
take effect on the next draw without recreating the surface. A zero width or
height MUST make `draw` a no-op (minimized window).

#### Scenario: Style change without a new surface
- GIVEN a running renderer
- WHEN `set_style` is called with a new background and padding 2
- THEN the next frame uses them and the surface is not recreated

### Requirement: Renderer selection at startup

The renderer order MUST come from `NXGTERM_RENDERER` (`auto`, `gpu`, `cpu`;
case-insensitive, spaces ignored) when it holds a known value, otherwise
from `[renderer] backend`. `auto` SHALL try `gpu` then `cpu`; `gpu` or `cpu`
SHALL try only that renderer. Selection MUST use `fallback::first_available`;
skipped renderers MUST be logged as `nxgterm: skipped <name>: <reason>` and
the choice as `nxgterm: renderer <name>`. When none starts, startup MUST
fail with `no renderer available (<name>: <reason>; ...)`.

#### Scenario: Env overrides config
- GIVEN `[renderer] backend = "cpu"` and `NXGTERM_RENDERER=auto`
- WHEN the order is computed
- THEN it is `[gpu, cpu]`

#### Scenario: Unknown env value
- GIVEN `NXGTERM_RENDERER=vulkan` and backend `cpu`
- WHEN the order is computed
- THEN it is `[cpu]`

### Requirement: GPU adapter choice

The GPU renderer MUST enumerate all wgpu backends (Vulkan, Metal, DX12, GL),
keep adapters that can present to the window surface, and rank them
integrated, discrete, virtual, other. Software adapters (`DeviceType::Cpu`:
WARP, llvmpipe, SwiftShader) MUST be rejected for the window renderer, so
`auto` falls back to the lighter CPU renderer. The device MUST request
WebGL2/GLES 3.0-level downlevel limits. A panic during GPU initialization
MUST be caught and reported as a failed candidate. The chosen adapter SHALL
be logged as `nxgterm: gpu adapter <name> (<backend>, <type>)`.

#### Scenario: Only a software adapter
- GIVEN a machine whose only adapter is WARP
- WHEN nxgterm starts with `auto`
- THEN `gpu` is skipped with `no usable hardware adapter (found: ...)` and `cpu` is used

### Requirement: GPU drawing

The GPU renderer MUST draw cell backgrounds, the cursor and glyphs as
instanced quads, with glyph coverage from a 1024x1024 R8 atlas filled on
demand per `(char, bold)`. When the atlas is full it MUST be cleared and the
frame rebuilt once; glyphs that still do not fit are skipped. It SHOULD use a
non-sRGB 8-bit surface format so blending matches the CPU renderer; with only
sRGB formats the shader MUST linearize colors. Surface size MUST be clamped
to the device's maximum texture dimension.

#### Scenario: GPU matches CPU output
- GIVEN the same terminal content
- WHEN rendered offscreen by the GPU painter and by the CPU renderer
- THEN every channel differs by at most 2

### Requirement: CPU drawing

The CPU renderer MUST draw into a 0RGB framebuffer presented with
softbuffer, in this order: clear to background, cell backgrounds, images with
`z < 0`, block cursor, glyphs, then images with `z >= 0`. Inverse video MUST
swap foreground and background; bold MUST select the bright variant for ANSI
colors 0-7 and the bold face when the family has one. The glyph under a
visible cursor MUST be drawn in the cell background color. Selected cells
MUST use `selection_foreground`/`selection_background` when the palette has
them, each unset one taking the cell's colors swapped (inverse video); the
GPU renderer MUST match. Italic and
underline flags are parsed but SHALL NOT be rendered yet.

#### Scenario: Glyph stays inside its cell
- GIVEN a 2x1 terminal showing `W` with the cursor hidden
- WHEN rendered
- THEN only the first cell has non-background pixels

### Requirement: Padding and grid fit

The grid MUST be inset by `padding` physical pixels on every side and the
padding MUST always be painted with the background. The grid size MUST be
the number of whole cells that fit the window minus padding (at least 1x1).
On every resize, scale change, font zoom or restyle the terminal and the PTY
MUST be resized to that grid and told the cell pixel size.

#### Scenario: Padding stays blank
- GIVEN padding 5 and a single red cell
- WHEN rendered
- THEN pixel (4,4) is background, (5,5) is red, and the last 5 pixels are background

### Requirement: Fonts

Fonts MUST be discovered with fontdb: the first installed configured
family (in list order), then the generic `monospace` alias, then DejaVu Sans Mono, Cascadia Mono, Consolas,
Menlo, SF Mono, Liberation Mono, Noto Sans Mono, Courier New. Requested
families skipped because they are not installed MUST be reported on stderr. Glyphs
MUST be rasterized with fontdue and cached. There is no per-glyph fallback
to other fonts for missing characters.

#### Scenario: Missing family
- GIVEN `family = "Nope Mono"`
- WHEN nxgterm starts
- THEN stderr shows `font family `Nope Mono` not found; using `<found>``

#### Scenario: First installed family of a list
- GIVEN `family = ["Nope Mono", "Fira Code"]` and Fira Code is installed
- WHEN nxgterm starts
- THEN Fira Code is used
- AND stderr shows `font family `Nope Mono` not found; using `Fira Code``

### Requirement: Transient frame errors

`Outdated` and `Lost` surfaces MUST reconfigure the surface and return
`Transient`; `Timeout` MUST return `Transient`. The application MUST retry a
transient frame at most 3 consecutive times, then wait for the next event
before drawing again; a successful frame MUST reset the counter.

#### Scenario: Occluded window
- GIVEN a surface that keeps timing out
- WHEN redraws fail 3 times in a row
- THEN no further redraw is requested until another event arrives

### Requirement: Runtime fallback to CPU

Device loss and uncaptured wgpu errors MUST be recorded and turned into
`RenderError::Fatal` on the next draw (or at construction). On a fatal error
from a non-CPU renderer the application MUST log
`<name> renderer failed: <error>; falling back to cpu`, drop the failed
renderer before attaching a new surface, create the CPU renderer with the
current style, resize the grid and redraw. A fatal error from the CPU
renderer MUST stop the application with that error.

#### Scenario: Device lost mid-session
- GIVEN the GPU renderer is active
- WHEN the device is lost
- THEN the next redraw switches to the CPU renderer and logs `nxgterm: renderer cpu`

### Requirement: Multi-pane drawing

`WindowRenderer` MUST draw an ordered list of panes (terminal, cell
position, focused flag) plus the header, overlay and shapes. Each pane MUST
be drawn at its cell offset, keeping window-level padding, in the same order
(backgrounds, images below, cursor/text, images above) on the CPU and GPU
renderers. Dividers MUST be 1 cell wide with a line drawn in its color and
width. A single pane at the origin MUST render exactly as before.

#### Scenario: Two panes on CPU
- GIVEN panes A | B with different content
- WHEN rendered
- THEN each pane's cells appear only inside its rect and the divider cell shows the divider line

#### Scenario: GPU matches CPU with panes
- GIVEN the same two-pane content
- WHEN rendered offscreen by both renderers
- THEN every channel differs by at most 2

#### Scenario: Single pane unchanged
- GIVEN one pane at the origin
- WHEN rendered
- THEN the output equals the pre-change single-terminal frame

### Requirement: Inactive pane treatment

An unfocused pane MUST show a hollow cursor outline instead of a solid block
(none when the cursor is hidden), and its text and backgrounds MUST be
dimmed by `panes.inactive_dim` (inline images and the cursor outline are not
dimmed).
The focused pane MUST NOT be dimmed. A dim of 0 MUST disable dimming.

#### Scenario: Hollow cursor
- GIVEN panes A | B, B focused, A's cursor visible
- WHEN rendered
- THEN A's cursor cell is an outline with its center unfilled

#### Scenario: Dimming off
- GIVEN `inactive_dim = 0`
- WHEN rendered
- THEN inactive pane colors equal the focused rendering

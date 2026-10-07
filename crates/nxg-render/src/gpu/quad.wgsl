// Instanced quads: solid cell backgrounds/cursor and atlas-masked glyphs.
// Pixel coordinates have their origin at the top-left of the target.

struct Globals {
    viewport: vec2<f32>,
    // 1 when the target is an sRGB format and colors must be linearized.
    srgb: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var atlas: texture_2d<f32>;

struct Instance {
    @location(0) pos: vec2<i32>,
    @location(1) size: vec2<u32>,
    @location(2) uv: vec2<u32>,
    @location(3) color: u32,
    @location(4) kind: u32,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) origin: vec2<i32>,
    @location(1) @interpolate(flat) uv: vec2<u32>,
    @location(2) @interpolate(flat) color: vec4<f32>,
    @location(3) @interpolate(flat) kind: u32,
};

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32, instance: Instance) -> VertexOut {
    // Triangle strip corners: (0,0) (1,0) (0,1) (1,1).
    let corner = vec2<f32>(f32(index & 1u), f32(index >> 1u));
    let pixel = vec2<f32>(instance.pos) + corner * vec2<f32>(instance.size);
    let ndc = vec2<f32>(
        pixel.x / globals.viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / globals.viewport.y * 2.0,
    );

    var rgb = vec3<f32>(
        f32((instance.color >> 16u) & 0xffu),
        f32((instance.color >> 8u) & 0xffu),
        f32(instance.color & 0xffu),
    ) / 255.0;
    if globals.srgb == 1u {
        rgb = to_linear(rgb);
    }

    var out: VertexOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.origin = instance.pos;
    out.uv = instance.uv;
    out.color = vec4<f32>(rgb, 1.0);
    out.kind = instance.kind;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    if in.kind == 0u {
        return in.color;
    }
    // Exact texel fetch: one atlas texel per target pixel, no filtering.
    let local = vec2<i32>(floor(in.position.xy)) - in.origin;
    let coverage = textureLoad(atlas, in.uv + vec2<u32>(local), 0).r;
    return vec4<f32>(in.color.rgb, coverage);
}

// Image quads: one placement per instance, sampled nearest-neighbor with
// the same integer math as the CPU renderer (`images::sample`).

struct Globals {
    viewport: vec2<f32>,
    // 1 when the target is an sRGB format and colors must be linearized.
    srgb: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var image: texture_2d<f32>;

struct Instance {
    @location(0) pos: vec2<i32>,
    @location(1) size: vec2<u32>,
    @location(2) src_pos: vec2<u32>,
    @location(3) src_size: vec2<u32>,
};

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) origin: vec2<i32>,
    @location(1) @interpolate(flat) size: vec2<u32>,
    @location(2) @interpolate(flat) src_pos: vec2<u32>,
    @location(3) @interpolate(flat) src_size: vec2<u32>,
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
    var out: VertexOut;
    out.position = vec4<f32>(ndc, 0.0, 1.0);
    out.origin = instance.pos;
    out.size = instance.size;
    out.src_pos = instance.src_pos;
    out.src_size = instance.src_size;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    let local = vec2<u32>(max(vec2<i32>(floor(in.position.xy)) - in.origin, vec2<i32>(0)));
    // Source pixel under the destination pixel's center.
    let scaled = (local * 2u + 1u) * in.src_size / (in.size * 2u);
    let texel = in.src_pos + min(scaled, in.src_size - 1u);
    let color = textureLoad(image, texel, 0);
    var rgb = color.rgb;
    if globals.srgb == 1u {
        rgb = to_linear(rgb);
    }
    return vec4<f32>(rgb, color.a);
}

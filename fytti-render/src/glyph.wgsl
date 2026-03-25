// Instanced glyph shader.
// Each instance is a positioned quad sampling from the glyph atlas.
// Atlas texel is white+alpha; vertex color tints it.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: vec2<f32>;

@group(1) @binding(0)
var atlas_tex: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    // Per-instance: screen position + size
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    // Per-instance: UV rect in atlas (xy = top-left, zw = size in texels)
    @location(2) uv_rect: vec4<f32>,
    // Per-instance: tint color
    @location(3) color: vec4<f32>,
) -> VertexOutput {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);
    let pixel = pos + vec2<f32>(x, y) * size;
    let clip = vec2<f32>(
        pixel.x / viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.y * 2.0,
    );

    let atlas_size = vec2<f32>(textureDimensions(atlas_tex));
    let uv = (uv_rect.xy + vec2<f32>(x, y) * uv_rect.zw) / atlas_size;

    var out: VertexOutput;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.uv = uv;
    out.color = color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex = textureSample(atlas_tex, atlas_sampler, in.uv);
    // Atlas stores white glyphs with alpha — tint by vertex color
    return vec4<f32>(in.color.rgb, in.color.a * tex.a);
}

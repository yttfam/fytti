// Unified 2D shape shader.
// Each instance is a quad with mode-dependent rendering:
//   mode 0: solid color rect
//   mode 1: linear gradient (color → color2, vertical flag in extra.x)
//   mode 2: filled ellipse (SDF)
//   mode 3: stroked ellipse (SDF, stroke width in extra.x)

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) color2: vec4<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) @interpolate(flat) mode: u32,
    @location(4) extra: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> viewport: vec2<f32>;

@vertex
fn vs_main(
    @builtin(vertex_index) vi: u32,
    @location(0) pos: vec2<f32>,
    @location(1) size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) color2: vec4<f32>,
    @location(4) mode_and_extra: vec4<f32>,
) -> VertexOutput {
    let x = f32(vi & 1u);
    let y = f32((vi >> 1u) & 1u);
    let pixel = pos + vec2<f32>(x, y) * size;
    let clip = vec2<f32>(
        pixel.x / viewport.x * 2.0 - 1.0,
        1.0 - pixel.y / viewport.y * 2.0,
    );

    var out: VertexOutput;
    out.position = vec4<f32>(clip, 0.0, 1.0);
    out.color = color;
    out.color2 = color2;
    out.uv = vec2<f32>(x, y);
    out.mode = u32(mode_and_extra.x);
    out.extra = mode_and_extra.yz;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    switch in.mode {
        // Solid rect
        case 0u: {
            return in.color;
        }
        // Linear gradient
        case 1u: {
            let vertical = in.extra.x > 0.5;
            var t: f32;
            if vertical {
                t = in.uv.y;
            } else {
                t = in.uv.x;
            }
            return mix(in.color, in.color2, t);
        }
        // Filled ellipse (SDF)
        case 2u: {
            let p = in.uv * 2.0 - 1.0; // -1..1
            let d = dot(p, p); // circle SDF (works for ellipse because quad is already stretched)
            if d > 1.0 {
                discard;
            }
            // Soft edge for anti-aliasing
            let aa = fwidth(d);
            let alpha = 1.0 - smoothstep(1.0 - aa, 1.0, d);
            return vec4<f32>(in.color.rgb, in.color.a * alpha);
        }
        // Stroked ellipse (SDF)
        case 3u: {
            let p = in.uv * 2.0 - 1.0;
            let d = length(p);
            let stroke_w = in.extra.x; // normalized stroke width (0..1)
            let inner = 1.0 - stroke_w;
            let aa = fwidth(d);
            let outer_alpha = 1.0 - smoothstep(1.0 - aa, 1.0, d);
            let inner_alpha = smoothstep(inner - aa, inner, d);
            let alpha = outer_alpha * inner_alpha;
            if alpha < 0.01 {
                discard;
            }
            return vec4<f32>(in.color.rgb, in.color.a * alpha);
        }
        default: {
            return in.color;
        }
    }
}

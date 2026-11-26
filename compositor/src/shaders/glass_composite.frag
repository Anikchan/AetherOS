// P3-T05 glass composite pass (GLES 2.0 / GLSL ES 1.00).
//
// One fused pass for the material layers that used to be separate entries in
// the `glass.rs` pass list: saturation, tint, edge refraction, edge highlight,
// the rounded-corner mask and the premultiplied-alpha output. Fusing them saves
// a full damage-region round trip per surface; `glass.rs` keeps the individual
// pass kinds for the cost model, so the modelled budget stays conservative.
//
// See glass_downsample.frag for the renderer contract (`#version 100` is added
// by smithay, `//_DEFINES_` is replaced with its variant defines).
//
// Uniforms:
//   tint        straight-alpha tint colour (18% white/black at Full/Reduced,
//               92% panel colour for the Minimal fill)
//   saturation  backdrop saturation multiplier (1.2 / 1.1 / 1.0)
//   edge        x = highlight width px, y = alpha, z = extra top edge alpha,
//               w = corner radius px
//   surface     surface size in px, for the corner mask and the edge distance
//   region      damaged region inside the surface (x, y, w, h) in px; the
//               source texture covers exactly this region, so `v_coords` maps
//               onto it
//   refraction  refraction band px (0 disables the displacement)
//   fill        1.0 = Minimal tier: no backdrop, a solid tinted panel
//
// Output is premultiplied (smithay blends with ONE / ONE_MINUS_SRC_ALPHA).
//_DEFINES_

precision mediump float;

uniform sampler2D tex;
uniform float alpha;
uniform float saturation;
uniform vec4 tint;
uniform vec4 edge;
uniform vec2 surface;
uniform vec4 region;
uniform float refraction;
uniform float fill;

varying vec2 v_coords;

void main() {
    // Position inside the surface, in px, y down.
    vec2 pos = region.xy + v_coords * region.zw;

    vec4 color;
    if (fill > 0.5) {
        // Minimal tier: nothing is sampled, the panel is a solid tint.
        color = tint;
    } else {
        vec2 uv = v_coords;
        if (refraction > 0.0) {
            // Displace the backdrop towards the nearest edge, fading out over
            // the refraction band: a subtle lens at the border of the glass.
            vec2 d = min(pos, surface - pos);
            vec2 dir;
            if (d.x < d.y) {
                dir = vec2(pos.x < surface.x * 0.5 ? -1.0 : 1.0, 0.0);
            } else {
                dir = vec2(0.0, pos.y < surface.y * 0.5 ? -1.0 : 1.0);
            }
            float near = 1.0 - smoothstep(0.0, refraction, min(d.x, d.y));
            uv += dir * near * (refraction / max(region.zw, vec2(1.0))) * 0.5;
        }
        color = texture2D(tex, uv);

        // Saturation boost on the (opaque) blurred backdrop.
        float luma = dot(color.rgb, vec3(0.2126, 0.7152, 0.0722));
        color.rgb = mix(vec3(luma), color.rgb, saturation);
        color.rgb = mix(color.rgb, tint.rgb, tint.a);
        color.a = 1.0;
    }

    // Rounded-rectangle mask (signed distance, 1 px of anti-aliasing).
    vec2 half_size = surface * 0.5;
    float radius = max(edge.w, 0.0);
    vec2 q = abs(pos - half_size) - (half_size - vec2(radius));
    float dist = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    float mask = 1.0 - smoothstep(-1.0, 0.0, dist);

    // 1 px rim light, brighter along the top edge ("the glass catches light").
    float edge_dist = min(min(pos.x, pos.y), min(surface.x - pos.x, surface.y - pos.y));
    float rim = 1.0 - smoothstep(0.0, max(edge.x, 0.001), edge_dist);
    float rim_alpha = edge.y + (pos.y < surface.y * 0.5 ? edge.z : 0.0);
    color.rgb = mix(color.rgb, vec3(1.0), rim * clamp(rim_alpha, 0.0, 1.0));
    color.a = max(color.a, rim * clamp(rim_alpha, 0.0, 1.0));

    float out_alpha = color.a * mask * alpha;
    gl_FragColor = vec4(color.rgb * out_alpha, out_alpha);
}

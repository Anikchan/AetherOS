// P3-T05 dual-Kawase downsample pass (GLES 2.0 / GLSL ES 1.00).
//
// The smithay `GlesRenderer` wraps this source in a `#version 100` program and
// replaces the `//_DEFINES_` token below with its own `#define`s, so this file
// must never carry a `#version` directive of its own.
//
// Inputs supplied by the renderer: `tex` (bound to unit 0), `alpha` (the draw
// alpha), `matrix`/`tex_matrix` (vertex stage) and the `v_coords` varying.
// `sample_step` is the per-tap offset in normalised texture coordinates.
//
// Kernel: the 5-tap dual-Kawase "down" filter (Kawase 2015, used by KWin and
// the AetherOS design doc D1 section 4). Each level halves the resolution, so
// the union of the whole chain blurs by roughly `radius_px` from the material.
//_DEFINES_

precision mediump float;

uniform sampler2D tex;
uniform vec2 sample_step;
uniform float alpha;

varying vec2 v_coords;

void main() {
    vec2 h = sample_step;
    float hh = 0.5;

    vec4 sum = texture2D(tex, v_coords) * 4.0;
    sum += texture2D(tex, v_coords - vec2(h.x, 0.0));
    sum += texture2D(tex, v_coords + vec2(h.x, 0.0));
    sum += texture2D(tex, v_coords + vec2(h.x * hh, h.y * hh));
    sum += texture2D(tex, v_coords - vec2(h.x * hh, h.y * hh));

    gl_FragColor = (sum / 8.0) * alpha;
}

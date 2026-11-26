// P3-T05 dual-Kawase upsample pass (GLES 2.0 / GLSL ES 1.00).
//
// See glass_downsample.frag for the renderer contract (`#version 100` is added
// by smithay, `//_DEFINES_` is replaced with its variant defines).
//
// Kernel: the 8-tap tent filter of the dual-Kawase "up" phase, sampling the
// previous (smaller) level and drawing into the next larger one. `sample_step`
// is expressed against the *source* level, so the 2x taps below span two source
// texels and the reconstruction stays stable while the chain grows back up to
// the damage region size.
//_DEFINES_

precision mediump float;

uniform sampler2D tex;
uniform vec2 sample_step;
uniform float alpha;

varying vec2 v_coords;

void main() {
    vec2 h = sample_step;

    vec4 sum = vec4(0.0);
    sum += texture2D(tex, v_coords + vec2(-2.0 * h.x, 0.0));
    sum += texture2D(tex, v_coords + vec2(-h.x, h.y));
    sum += texture2D(tex, v_coords + vec2(0.0, 2.0 * h.y));
    sum += texture2D(tex, v_coords + vec2(h.x, h.y));
    sum += texture2D(tex, v_coords + vec2(2.0 * h.x, 0.0));
    sum += texture2D(tex, v_coords + vec2(h.x, -h.y));
    sum += texture2D(tex, v_coords + vec2(0.0, -2.0 * h.y));
    sum += texture2D(tex, v_coords + vec2(-h.x, -h.y));

    gl_FragColor = (sum / 8.0) * alpha;
}

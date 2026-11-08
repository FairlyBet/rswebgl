// The display pass: the HDR target (premultiplied linear radiance and
// coverage) → the canvas: exposure, tone mapping, sRGB encoding, and a
// little dither against banding in dark gradients.

precision highp float;

#include "color.glsl"
#include "tonemap.glsl"

in vec2 v_ndc;
out vec4 frag_color;

uniform sampler2D u_hdr;
uniform float u_exposure;
uniform int u_tone_mapping;
uniform float u_dither; // 0 off, 1 on

// Interleaved gradient noise (Jimenez 2014): a per-pixel value in [0, 1).
float ign(vec2 p) {
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}

void main() {
    vec4 hdr = texelFetch(u_hdr, ivec2(gl_FragCoord.xy), 0);
    float a = hdr.a;
    // Shade the unpremultiplied color, then premultiply again for the
    // canvas (WebGL's default premultipliedAlpha: true).
    vec3 c = a > 0.0 ? hdr.rgb / a : vec3(0.0);
    c = linear_to_srgb(tone_map(c * u_exposure, u_tone_mapping));
    c += (ign(gl_FragCoord.xy) - 0.5) / 255.0 * u_dither;
    frag_color = vec4(c * a, a);
}

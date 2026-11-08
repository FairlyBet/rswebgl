// Temporal antialiasing resolve — ported from Bevy's TAA
// (crates/bevy_anti_alias/src/taa/taa.wesl, MIT / Apache-2.0), itself after
// Karis 2014 ("High Quality Temporal Supersampling"), Salvi 2016 (variance
// clipping), Playdead's YCoCg helpers (MIT), Jimenez's 5-tap Catmull-Rom and
// Hoppe's confidence. Differences:
// - motion comes from reprojecting the depth through the camera's motion
//   (u_reproject), not from a motion-vector target: exact for a moving
//   camera; moving objects are held back by the variance clip;
// - the history holds linear premultiplied radiance (tone-mapped on read for
//   the filter and the clip, Karis' reversible map) and the confidence in
//   its alpha; coverage alpha is read from the current frame by the display.
// Needs nothing from the scene prelude.

precision highp float;
precision highp sampler2D;

in vec2 v_ndc;
out vec4 frag_color; // rgb linear premultiplied, a confidence

uniform sampler2D u_current; // this frame (jittered), linear HDR
uniform sampler2D u_history; // the last resolve
uniform sampler2D u_depth;   // this frame's depth
uniform mat4 u_reproject;    // clip now (unjittered) → clip last frame
uniform vec4 u_taa;          // x 1: reset, y 1: reverse-Z

// Lower = more history: chosen empirically by Bevy.
const float DEFAULT_HISTORY_BLEND_RATE = 0.1;
const float MIN_HISTORY_BLEND_RATE = 0.015;

float max3(vec3 c) { return max(c.r, max(c.g, c.b)); }
vec3 tonemap(vec3 c) { return c / (max3(c) + 1.0); }
vec3 reverse_tonemap(vec3 c) { return c / max(1.0 - max3(c), 1e-4); }

vec3 rgb_to_ycocg(vec3 c) {
    return vec3(c.r / 4.0 + c.g / 2.0 + c.b / 4.0,
                c.r / 2.0 - c.b / 2.0,
                -c.r / 4.0 + c.g / 2.0 - c.b / 4.0);
}

vec3 ycocg_to_rgb(vec3 c) {
    return clamp(vec3(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z), 0.0, 1.0);
}

vec3 clip_towards_aabb_center(vec3 history, vec3 lo, vec3 hi) {
    vec3 p = 0.5 * (hi + lo);
    vec3 e = 0.5 * (hi - lo) + 1e-8;
    vec3 v = history - p;
    float m = max3(abs(v / e));
    return m > 1.0 ? p + v / m : history;
}

vec3 sample_history(vec2 uv) {
    return tonemap(texture(u_history, uv).rgb);
}

vec3 sample_current(ivec2 p) {
    ivec2 size = textureSize(u_current, 0);
    return rgb_to_ycocg(tonemap(texelFetch(u_current, clamp(p, ivec2(0), size - 1), 0).rgb));
}

float depth_at(vec2 uv) {
    return texture(u_depth, uv).r;
}

void main() {
    vec2 size = vec2(textureSize(u_current, 0));
    vec2 texel = 1.0 / size;
    vec2 uv = gl_FragCoord.xy * texel;
    ivec2 px = ivec2(gl_FragCoord.xy);
    vec4 original = texelFetch(u_current, px, 0);
    if (u_taa.x > 0.5) {
        frag_color = vec4(original.rgb, 1.0 / MIN_HISTORY_BLEND_RATE);
        return;
    }
    vec3 current = tonemap(original.rgb);
    bool reverse = u_taa.y > 0.5;

    // The nearest depth of five taps (Karis 2014, slide 27): moving edges
    // take the foreground's motion.
    vec2 offset = texel * 2.0;
    vec2 closest_uv = uv;
    float closest = depth_at(uv);
    vec2 taps[4] = vec2[4](uv + vec2(-offset.x, offset.y), uv + vec2(offset.x, offset.y),
                           uv + vec2(-offset.x, -offset.y), uv + vec2(offset.x, -offset.y));
    for (int i = 0; i < 4; i++) {
        float d = depth_at(taps[i]);
        if (reverse ? d > closest : d < closest) {
            closest = d;
            closest_uv = taps[i];
        }
    }

    // Where that point was last frame: through the camera's motion.
    vec3 ndc = vec3(closest_uv * 2.0 - 1.0, reverse ? closest : closest * 2.0 - 1.0);
    vec4 prev = u_reproject * vec4(ndc, 1.0);
    vec2 prev_uv = prev.xy / prev.w * 0.5 + 0.5;
    vec2 motion = closest_uv - prev_uv;

    // The history, 5-tap Catmull-Rom (corners left out).
    vec2 history_uv = uv - motion;
    vec2 sample_position = history_uv * size;
    vec2 texel_center = floor(sample_position - 0.5) + 0.5;
    vec2 f = sample_position - texel_center;
    vec2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    vec2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    vec2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    vec2 w3 = f * f * (-0.5 + 0.5 * f);
    vec2 w12 = w1 + w2;
    vec2 tp0 = (texel_center - 1.0) * texel;
    vec2 tp3 = (texel_center + 2.0) * texel;
    vec2 tp12 = (texel_center + w2 / w12) * texel;
    vec3 history = sample_history(vec2(tp12.x, tp0.y)) * w12.x * w0.y;
    history += sample_history(vec2(tp0.x, tp12.y)) * w0.x * w12.y;
    history += sample_history(vec2(tp12.x, tp12.y)) * w12.x * w12.y;
    history += sample_history(vec2(tp3.x, tp12.y)) * w3.x * w12.y;
    history += sample_history(vec2(tp12.x, tp3.y)) * w12.x * w3.y;

    // Clipped to the 3×3 neighborhood's spread in YCoCg (Salvi 2016).
    vec3 m1 = vec3(0.0);
    vec3 m2 = vec3(0.0);
    for (int y = -1; y <= 1; y++) {
        for (int x = -1; x <= 1; x++) {
            vec3 s = (x == 0 && y == 0) ? rgb_to_ycocg(current) : sample_current(px + ivec2(x, y));
            m1 += s;
            m2 += s * s;
        }
    }
    vec3 mean = m1 / 9.0;
    vec3 sd = sqrt(max(m2 / 9.0 - mean * mean, vec3(0.0)));
    history = ycocg_to_rgb(clip_towards_aabb_center(rgb_to_ycocg(history), mean - sd, mean + sd));

    // Confidence grows while the pixel holds still, resets when it moves.
    float confidence = texelFetch(u_history, px, 0).a;
    vec2 pixel_motion = abs(motion) * size;
    confidence = (pixel_motion.x < 0.01 && pixel_motion.y < 0.01) ? min(confidence + 10.0, 1000.0) : 1.0;
    float factor = clamp(1.0 / confidence, MIN_HISTORY_BLEND_RATE, DEFAULT_HISTORY_BLEND_RATE);
    // History from off screen: none to use.
    if (any(notEqual(clamp(history_uv, 0.0, 1.0), history_uv))) {
        factor = 1.0;
        confidence = 1.0;
    }
    current = mix(history, current, factor);
    frag_color = vec4(reverse_tonemap(current), confidence);
}

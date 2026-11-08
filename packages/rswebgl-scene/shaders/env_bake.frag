// Environment baking, one cube face (u_face) per draw. Modes (`#define`):
//   DECODE      the RGBE image as uploaded (u_rgbe, RGBA8) → linear radiance,
//               one texel per pixel, into the equirect texture
//   EQUIRECT    an equirectangular image (u_equirect) → the source cube
//   SPECULAR    the source cube (u_source) prefiltered for u_roughness by GGX
//               importance sampling (filtered: each sample reads the mip that
//               matches its solid angle)
//   IRRADIANCE  the source cube convolved with a cosine lobe, stored as E/π
//               (so diffuse = albedo * texel)

precision highp float;
// Samplers default to lowp in fragment shaders, which can clip HDR values on
// mobile GPUs.
precision highp sampler2D;
precision highp samplerCube;

in vec2 v_ndc;
out vec4 frag_color;

uniform int u_face;
#ifdef DECODE
uniform sampler2D u_rgbe;
#elif defined(EQUIRECT)
uniform sampler2D u_equirect;
#else
uniform samplerCube u_source;
uniform float u_source_size; // texels per face side at mip 0
#endif
#ifdef SPECULAR
uniform float u_roughness;
#endif

const float PI = 3.14159265359;

// Direction through this texel of face u_face (GL cube-map conventions; the
// framebuffer's rows are the face's t axis).
vec3 face_direction(vec2 st) {
    float s = st.x;
    float t = st.y;
    if (u_face == 0) return vec3(1.0, -t, -s);
    if (u_face == 1) return vec3(-1.0, -t, s);
    if (u_face == 2) return vec3(s, 1.0, t);
    if (u_face == 3) return vec3(s, -1.0, -t);
    if (u_face == 4) return vec3(s, -t, 1.0);
    return vec3(-s, -t, -1.0);
}

#if !defined(EQUIRECT) && !defined(DECODE)
vec2 hammersley(uint i, uint n) {
    uint b = i;
    b = (b << 16u) | (b >> 16u);
    b = ((b & 0x55555555u) << 1u) | ((b & 0xAAAAAAAAu) >> 1u);
    b = ((b & 0x33333333u) << 2u) | ((b & 0xCCCCCCCCu) >> 2u);
    b = ((b & 0x0F0F0F0Fu) << 4u) | ((b & 0xF0F0F0F0u) >> 4u);
    b = ((b & 0x00FF00FFu) << 8u) | ((b & 0xFF00FF00u) >> 8u);
    return vec2(float(i) / float(n), float(b) * 2.3283064365386963e-10);
}

mat3 basis(vec3 n) {
    vec3 up = abs(n.z) < 0.999 ? vec3(0.0, 0.0, 1.0) : vec3(1.0, 0.0, 0.0);
    vec3 t = normalize(cross(up, n));
    return mat3(t, cross(n, t), n);
}

// The mip whose texel covers the solid angle of one of `count` samples drawn
// with density `pdf`.
float sample_lod(float pdf, uint count) {
    float texel = 4.0 * PI / (6.0 * u_source_size * u_source_size);
    float sample_angle = 1.0 / (float(count) * pdf + 1e-6);
    return max(0.5 * log2(sample_angle / texel) + 1.0, 0.0);
}
#endif

void main() {
#ifdef DECODE
    // value = byte × 2^(e − 136); bytes come back as /255.
    vec4 p = floor(texelFetch(u_rgbe, ivec2(gl_FragCoord.xy), 0) * 255.0 + 0.5);
    vec3 c = p.a > 0.0 ? p.rgb * exp2(p.a - 136.0) : vec3(0.0);
    // Half float tops out at 65504; brighter texels (a sun in a real HDRI
    // can reach 2e5) would become infinities, which filtering turns into NaN
    // (∞·0) that the prefilter spreads over whole faces. Scale such a texel
    // down to the limit, keeping its hue.
    c *= min(1.0, 65504.0 / max(max(c.r, max(c.g, c.b)), 1e-30));
    frag_color = vec4(c, 1.0);
#else
    vec3 n = normalize(face_direction(v_ndc));
#endif

#ifdef EQUIRECT
    vec2 uv = vec2(atan(n.z, n.x) / (2.0 * PI) + 0.5, acos(clamp(n.y, -1.0, 1.0)) / PI);
    frag_color = vec4(texture(u_equirect, uv).rgb, 1.0);
#endif

#ifdef SPECULAR
    if (u_roughness <= 0.0) {
        frag_color = vec4(textureLod(u_source, n, 0.0).rgb, 1.0);
        return;
    }
    const uint COUNT = 128u;
    float a = u_roughness * u_roughness;
    float a2 = a * a;
    mat3 tbn = basis(n);
    vec3 sum = vec3(0.0);
    float weight = 0.0;
    for (uint i = 0u; i < COUNT; i++) {
        vec2 xi = hammersley(i, COUNT);
        float phi = 2.0 * PI * xi.x;
        float cos_t = sqrt((1.0 - xi.y) / (1.0 + (a2 - 1.0) * xi.y));
        float sin_t = sqrt(1.0 - cos_t * cos_t);
        vec3 h = tbn * vec3(sin_t * cos(phi), sin_t * sin(phi), cos_t);
        // n = v = r (the split-sum assumption).
        vec3 l = 2.0 * dot(n, h) * h - n;
        float n_l = dot(n, l);
        if (n_l > 0.0) {
            float d = (cos_t * cos_t * (a2 - 1.0) + 1.0);
            float pdf = a2 / (PI * d * d) * 0.25; // D·(n·h) / (4 v·h), n = v
            sum += textureLod(u_source, l, sample_lod(pdf, COUNT)).rgb * n_l;
            weight += n_l;
        }
    }
    frag_color = vec4(sum / max(weight, 1e-6), 1.0);
#endif

#ifdef IRRADIANCE
    const uint COUNT = 128u;
    mat3 tbn = basis(n);
    vec3 sum = vec3(0.0);
    for (uint i = 0u; i < COUNT; i++) {
        vec2 xi = hammersley(i, COUNT);
        // Cosine-weighted: pdf = cos / π, and the estimator of E/π is the
        // plain mean of the samples.
        float phi = 2.0 * PI * xi.x;
        float cos_t = sqrt(1.0 - xi.y);
        float sin_t = sqrt(xi.y);
        vec3 l = tbn * vec3(sin_t * cos(phi), sin_t * sin(phi), cos_t);
        sum += textureLod(u_source, l, sample_lod(cos_t / PI, COUNT)).rgb;
    }
    frag_color = vec4(sum / float(COUNT), 1.0);
#endif
}

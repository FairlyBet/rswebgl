// The first mip of the scene copy transmission sees through, from level 0,
// with Filament's firefly suppression: each tap weighted by
// 1 / (1 + max component) — ported from Filament
// (filament/src/materials/separableGaussianBlur.fs, tapReinhard; Apache-2.0),
// which does it for the first level of its screen-space refraction mips
// (PostProcessManager::generateMipmapSSR → generateGaussianMipmap with
// reinhard). Without it a subpixel HDR highlight behind glass (a point
// light's specular on metal) blooms into the rough mips and flashes as it
// moves between pixels.
//
// Changes from the original: a 2×2 box (the rest of the chain is
// generateMipmap's box) in place of the separable Gaussian.

precision highp float;
precision highp int;
precision highp sampler2D;

out vec4 frag_color;

uniform sampler2D u_source; // sampled at level 0 only (its range set so)

float vmax(vec4 v) {
    return max(max(v.x, v.y), max(v.z, v.w));
}

void tapReinhard(inout vec4 sum, inout float totalWeight, ivec2 p, ivec2 size) {
    vec4 s = texelFetch(u_source, min(p, size - 1), 0);
    float w = 1.0 / (1.0 + vmax(s));
    totalWeight += w;
    sum += s * w;
}

void main() {
    ivec2 size = textureSize(u_source, 0);
    ivec2 p = ivec2(gl_FragCoord.xy) * 2;
    vec4 sum = vec4(0.0);
    float totalWeight = 0.0;
    tapReinhard(sum, totalWeight, p, size);
    tapReinhard(sum, totalWeight, p + ivec2(1, 0), size);
    tapReinhard(sum, totalWeight, p + ivec2(0, 1), size);
    tapReinhard(sum, totalWeight, p + ivec2(1, 1), size);
    frag_color = sum * (1.0 / totalWeight);
}

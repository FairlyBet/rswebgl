// FXAA — ported from three.js r180 (examples/jsm/shaders/FXAAShader.js;
// MIT): NVIDIA's FXAA as Jasper Flick's C# implementation (Catlike Coding,
// "FXAA"), GLSL by Dave Hoskins. Runs after the display pass, on its
// display-encoded (sRGB, premultiplied) colors: the luma FXAA compares is
// meant to be perceptual.
//
// Changes from the original: GLSL ES 3.00 (texture / out); the edge step
// table filled element by element (Mali rejects array constructors without
// a default precision: S0032); uv from gl_FragCoord; the tunables as
// constants; the corner taps read only past the skip test (as FXAA 3.11
// does; the same result, fewer reads where there is no edge).

precision highp float;
precision highp int;
precision highp sampler2D;

in vec2 v_ndc;
out vec4 frag_color;

uniform sampler2D u_ldr; // the display pass's output, bilinear, clamped
uniform vec2 u_texel;    // 1 / its size

#define EDGE_STEP_COUNT 6
#define EDGE_GUESS 8.0

const float _ContrastThreshold = 0.0312;
const float _RelativeThreshold = 0.063;
const float _SubpixelBlending = 1.0;

float edgeSteps[EDGE_STEP_COUNT];

vec4 Sample(sampler2D tex2D, vec2 uv) {
    return texture(tex2D, uv);
}

float SampleLuminance(sampler2D tex2D, vec2 uv) {
    return dot(Sample(tex2D, uv).rgb, vec3(0.3, 0.59, 0.11));
}

float SampleLuminance(sampler2D tex2D, vec2 texSize, vec2 uv, float uOffset, float vOffset) {
    uv += texSize * vec2(uOffset, vOffset);
    return SampleLuminance(tex2D, uv);
}

struct LuminanceData {
    float m, n, e, s, w;
    float ne, nw, se, sw;
    float highest, lowest, contrast;
};

LuminanceData SampleLuminanceNeighborhood(sampler2D tex2D, vec2 texSize, vec2 uv) {
    LuminanceData l;
    l.m = SampleLuminance(tex2D, uv);
    l.n = SampleLuminance(tex2D, texSize, uv, 0.0, 1.0);
    l.e = SampleLuminance(tex2D, texSize, uv, 1.0, 0.0);
    l.s = SampleLuminance(tex2D, texSize, uv, 0.0, -1.0);
    l.w = SampleLuminance(tex2D, texSize, uv, -1.0, 0.0);

    // The corners are sampled once past ShouldSkipPixel (SampleCorners),
    // which reads only the cross: most pixels skip after 5 taps, not 9.

    l.highest = max(max(max(max(l.n, l.e), l.s), l.w), l.m);
    l.lowest = min(min(min(min(l.n, l.e), l.s), l.w), l.m);
    l.contrast = l.highest - l.lowest;
    return l;
}

// Moved out of SampleLuminanceNeighborhood (see there); same taps.
void SampleCorners(sampler2D tex2D, vec2 texSize, vec2 uv, inout LuminanceData l) {
    l.ne = SampleLuminance(tex2D, texSize, uv, 1.0, 1.0);
    l.nw = SampleLuminance(tex2D, texSize, uv, -1.0, 1.0);
    l.se = SampleLuminance(tex2D, texSize, uv, 1.0, -1.0);
    l.sw = SampleLuminance(tex2D, texSize, uv, -1.0, -1.0);
}

bool ShouldSkipPixel(LuminanceData l) {
    float threshold = max(_ContrastThreshold, _RelativeThreshold * l.highest);
    return l.contrast < threshold;
}

float DeterminePixelBlendFactor(LuminanceData l) {
    float f = 2.0 * (l.n + l.e + l.s + l.w);
    f += l.ne + l.nw + l.se + l.sw;
    f *= 1.0 / 12.0;
    f = abs(f - l.m);
    f = clamp(f / l.contrast, 0.0, 1.0);

    float blendFactor = smoothstep(0.0, 1.0, f);
    return blendFactor * blendFactor * _SubpixelBlending;
}

struct EdgeData {
    bool isHorizontal;
    float pixelStep;
    float oppositeLuminance, gradient;
};

EdgeData DetermineEdge(vec2 texSize, LuminanceData l) {
    EdgeData e;
    float horizontal =
        abs(l.n + l.s - 2.0 * l.m) * 2.0 +
        abs(l.ne + l.se - 2.0 * l.e) +
        abs(l.nw + l.sw - 2.0 * l.w);
    float vertical =
        abs(l.e + l.w - 2.0 * l.m) * 2.0 +
        abs(l.ne + l.nw - 2.0 * l.n) +
        abs(l.se + l.sw - 2.0 * l.s);
    e.isHorizontal = horizontal >= vertical;

    float pLuminance = e.isHorizontal ? l.n : l.e;
    float nLuminance = e.isHorizontal ? l.s : l.w;
    float pGradient = abs(pLuminance - l.m);
    float nGradient = abs(nLuminance - l.m);

    e.pixelStep = e.isHorizontal ? texSize.y : texSize.x;

    if (pGradient < nGradient) {
        e.pixelStep = -e.pixelStep;
        e.oppositeLuminance = nLuminance;
        e.gradient = nGradient;
    } else {
        e.oppositeLuminance = pLuminance;
        e.gradient = pGradient;
    }

    return e;
}

float DetermineEdgeBlendFactor(sampler2D tex2D, vec2 texSize, LuminanceData l, EdgeData e, vec2 uv) {
    vec2 uvEdge = uv;
    vec2 edgeStep;
    if (e.isHorizontal) {
        uvEdge.y += e.pixelStep * 0.5;
        edgeStep = vec2(texSize.x, 0.0);
    } else {
        uvEdge.x += e.pixelStep * 0.5;
        edgeStep = vec2(0.0, texSize.y);
    }

    float edgeLuminance = (l.m + e.oppositeLuminance) * 0.5;
    float gradientThreshold = e.gradient * 0.25;

    vec2 puv = uvEdge + edgeStep * edgeSteps[0];
    float pLuminanceDelta = SampleLuminance(tex2D, puv) - edgeLuminance;
    bool pAtEnd = abs(pLuminanceDelta) >= gradientThreshold;

    for (int i = 1; i < EDGE_STEP_COUNT && !pAtEnd; i++) {
        puv += edgeStep * edgeSteps[i];
        pLuminanceDelta = SampleLuminance(tex2D, puv) - edgeLuminance;
        pAtEnd = abs(pLuminanceDelta) >= gradientThreshold;
    }

    if (!pAtEnd) {
        puv += edgeStep * EDGE_GUESS;
    }

    vec2 nuv = uvEdge - edgeStep * edgeSteps[0];
    float nLuminanceDelta = SampleLuminance(tex2D, nuv) - edgeLuminance;
    bool nAtEnd = abs(nLuminanceDelta) >= gradientThreshold;

    for (int i = 1; i < EDGE_STEP_COUNT && !nAtEnd; i++) {
        nuv -= edgeStep * edgeSteps[i];
        nLuminanceDelta = SampleLuminance(tex2D, nuv) - edgeLuminance;
        nAtEnd = abs(nLuminanceDelta) >= gradientThreshold;
    }

    if (!nAtEnd) {
        nuv -= edgeStep * EDGE_GUESS;
    }

    float pDistance, nDistance;
    if (e.isHorizontal) {
        pDistance = puv.x - uv.x;
        nDistance = uv.x - nuv.x;
    } else {
        pDistance = puv.y - uv.y;
        nDistance = uv.y - nuv.y;
    }

    float shortestDistance;
    bool deltaSign;
    if (pDistance <= nDistance) {
        shortestDistance = pDistance;
        deltaSign = pLuminanceDelta >= 0.0;
    } else {
        shortestDistance = nDistance;
        deltaSign = nLuminanceDelta >= 0.0;
    }

    if (deltaSign == (l.m - edgeLuminance >= 0.0)) {
        return 0.0;
    }

    return 0.5 - shortestDistance / (pDistance + nDistance);
}

vec4 ApplyFXAA(sampler2D tex2D, vec2 texSize, vec2 uv) {
    LuminanceData luminance = SampleLuminanceNeighborhood(tex2D, texSize, uv);
    if (ShouldSkipPixel(luminance)) {
        return Sample(tex2D, uv);
    }
    SampleCorners(tex2D, texSize, uv, luminance);

    float pixelBlend = DeterminePixelBlendFactor(luminance);
    EdgeData edge = DetermineEdge(texSize, luminance);
    float edgeBlend = DetermineEdgeBlendFactor(tex2D, texSize, luminance, edge, uv);
    float finalBlend = max(pixelBlend, edgeBlend);

    if (edge.isHorizontal) {
        uv.y += edge.pixelStep * finalBlend;
    } else {
        uv.x += edge.pixelStep * finalBlend;
    }

    return Sample(tex2D, uv);
}

void main() {
    // EDGE_STEPS 1.0, 1.5, 2.0, 2.0, 2.0, 4.0
    edgeSteps[0] = 1.0;
    edgeSteps[1] = 1.5;
    edgeSteps[2] = 2.0;
    edgeSteps[3] = 2.0;
    edgeSteps[4] = 2.0;
    edgeSteps[5] = 4.0;
    frag_color = ApplyFXAA(u_ldr, u_texel, gl_FragCoord.xy * u_texel);
}

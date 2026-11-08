// Punctual lights and the environment's strength (the lighting block, slot
// 0). Light type: 0 directional, 1 point, 2 spot (KHR_lights_punctual).

#define MAX_LIGHTS 8
struct Light {
    vec4 position;  // xyz world position (point, spot)
    vec4 direction; // xyz world direction the light points (directional, spot)
    vec4 color;     // rgb color, w intensity (lux for directional, candela else)
    vec4 params;    // x range (0 = infinite), y spot scale, z spot offset, w type
};
layout(std140) uniform Lighting {
    Light u_lights[MAX_LIGHTS];
    vec4 u_environment; // rgb intensity of the environment light
    vec4 u_lighting;    // x light count, y exposure, z environment rotation,
                        // w last mip of the specular cube
    vec4 u_output;      // x 1: scene_output shows (exposure, tone map, sRGB),
                        //   0: it leaves linear radiance (HDR target);
                        // y tone mapper (tone_map modes)
};

// Direction from a surface point toward light `light` (unnormalized for
// point and spot lights: the distance is in it).
vec3 light_vector(Light light, vec3 position) {
    return light.params.w < 0.5 ? -light.direction.xyz : light.position.xyz - position;
}

// Distance and cone falloff (KHR_lights_punctual): the range window
// clamp(1 - (d / range)^4, 0, 1) / d², the spot's smooth cone.
float light_attenuation(Light light, vec3 to_light) {
    float type = light.params.w;
    if (type < 0.5) return 1.0; // directional
    float d2 = max(dot(to_light, to_light), 1e-8);
    float att = 1.0 / d2;
    float range = light.params.x;
    if (range > 0.0) {
        float r = sqrt(d2) / range;
        att *= clamp(1.0 - r * r * r * r, 0.0, 1.0);
    }
    if (type > 1.5) { // spot
        float cd = dot(normalize(light.direction.xyz), -normalize(to_light));
        float s = clamp(cd * light.params.y + light.params.z, 0.0, 1.0);
        att *= s * s;
    }
    return att;
}

// Radiance arriving from `light` at `position`, along `l` (normalized).
vec3 light_radiance(Light light, vec3 position, out vec3 l) {
    vec3 to_light = light_vector(light, position);
    l = normalize(to_light);
    return light.color.rgb * light.color.w * light_attenuation(light, to_light);
}

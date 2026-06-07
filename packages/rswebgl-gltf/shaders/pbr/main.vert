#version 300 es

// ============================================================================
// glTF 2.0 metallic-roughness — VERTEX stage (reference shader)
//
// This declares the full data interface a glTF primitive can feed a shader, per
// the glTF 2.0 spec (§3.7 meshes, §3.9 materials, §5 skins). Optional inputs are
// gated behind feature `#define`s that the host injects right after `#version`
// (one per attribute/feature the primitive+material actually provide). The PBR
// lighting math lives in main.frag and is still a placeholder — here we only
// transform geometry and forward everything the fragment stage may need.
//
// Attribute locations are fixed by the translator (see src/geometry.rs):
//   0 POSITION  1 NORMAL  2 TEXCOORD_0  3 TANGENT  4 COLOR_0
//   5 TEXCOORD_1  6 JOINTS_0  7 WEIGHTS_0   (reserved; emitted once supported)
//
// Recognized defines (host enables the ones that apply):
//   HAS_NORMAL_VEC3            primitive has NORMAL
//   HAS_TANGENT_VEC4           primitive has TANGENT (w = handedness)
//   HAS_TEXCOORD_0_VEC2        primitive has TEXCOORD_0
//   HAS_TEXCOORD_1_VEC2        primitive has TEXCOORD_1
//   HAS_COLOR_0_VEC3 / _VEC4   primitive has COLOR_0 (3- or 4-component)
//   USE_SKINNING               primitive has JOINTS_0 + WEIGHTS_0 (needs Skin UBO)
//   MAX_JOINTS <n>             joint-matrix array size (default 64)
// ============================================================================

precision highp float;
precision highp int;

// --- Vertex attributes ------------------------------------------------------
layout(location = 0) in vec3 a_position;
#ifdef HAS_NORMAL_VEC3
layout(location = 1) in vec3 a_normal;
#endif
#ifdef HAS_TEXCOORD_0_VEC2
layout(location = 2) in vec2 a_texcoord_0;
#endif
#ifdef HAS_TANGENT_VEC4
layout(location = 3) in vec4 a_tangent;
#endif
#ifdef HAS_COLOR_0_VEC4
layout(location = 4) in vec4 a_color_0;
#endif
#ifdef HAS_COLOR_0_VEC3
layout(location = 4) in vec3 a_color_0;
#endif
#ifdef HAS_TEXCOORD_1_VEC2
layout(location = 5) in vec2 a_texcoord_1;
#endif
#ifdef USE_SKINNING
layout(location = 6) in uvec4 a_joints_0;
layout(location = 7) in vec4  a_weights_0;
#endif

// --- Transforms (std140 uniform blocks) -------------------------------------
// Per-frame camera data, shared by every draw.
layout(std140) uniform Frame {
    mat4 u_view_proj;      // world -> clip
    vec4 u_camera_position; // world-space eye (.xyz; .w unused)
};
// Per-object transform. u_normal_matrix is the inverse-transpose of u_model,
// stored as a mat4 to avoid std140's mat3 column padding pitfalls.
layout(std140) uniform Object {
    mat4 u_model;
    mat4 u_normal_matrix;
};
#ifdef USE_SKINNING
#ifndef MAX_JOINTS
#define MAX_JOINTS 64
#endif
layout(std140) uniform Skin {
    mat4 u_joint_matrix[MAX_JOINTS];
};
#endif

// --- Varyings forwarded to the fragment stage -------------------------------
out vec3 v_world_position;
#ifdef HAS_NORMAL_VEC3
out vec3 v_normal;
#ifdef HAS_TANGENT_VEC4
out vec3 v_tangent;
out vec3 v_bitangent;
#endif
#endif
out vec2 v_uv0;
out vec2 v_uv1;
#if defined(HAS_COLOR_0_VEC3) || defined(HAS_COLOR_0_VEC4)
out vec4 v_color0;
#endif

// Skinning matrix for this vertex (identity when not skinned), per glTF §5.
mat4 skin_matrix() {
#ifdef USE_SKINNING
    return a_weights_0.x * u_joint_matrix[int(a_joints_0.x)]
         + a_weights_0.y * u_joint_matrix[int(a_joints_0.y)]
         + a_weights_0.z * u_joint_matrix[int(a_joints_0.z)]
         + a_weights_0.w * u_joint_matrix[int(a_joints_0.w)];
#else
    return mat4(1.0);
#endif
}

void main() {
    mat4 model = u_model * skin_matrix();
    vec4 world = model * vec4(a_position, 1.0);
    v_world_position = world.xyz;

    // Normal matrix folds in the skin transform (mat3 of the combined model).
    mat3 normal_mtx = mat3(u_normal_matrix) * mat3(skin_matrix());

#ifdef HAS_NORMAL_VEC3
    v_normal = normalize(normal_mtx * a_normal);
#ifdef HAS_TANGENT_VEC4
    vec3 t = normalize(mat3(model) * a_tangent.xyz);
    // Bitangent handedness comes from TANGENT.w (±1), per glTF §3.7.2.1.
    v_tangent = t;
    v_bitangent = cross(v_normal, t) * a_tangent.w;
#endif
#endif

#ifdef HAS_TEXCOORD_0_VEC2
    v_uv0 = a_texcoord_0;
#else
    v_uv0 = vec2(0.0);
#endif
#ifdef HAS_TEXCOORD_1_VEC2
    v_uv1 = a_texcoord_1;
#else
    v_uv1 = vec2(0.0);
#endif

#ifdef HAS_COLOR_0_VEC4
    v_color0 = a_color_0;
#elif defined(HAS_COLOR_0_VEC3)
    v_color0 = vec4(a_color_0, 1.0);
#endif

    gl_Position = u_view_proj * world;
}

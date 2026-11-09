// glTF 2.0 metallic-roughness — vertex stage, after the scene prelude
// (`scene.vert.glsl`: camera, object). `PbrShaders` adds one `#define` per
// feature the primitive has:
//   HAS_NORMAL HAS_UV0 HAS_TANGENT HAS_COLOR HAS_UV1
// Attribute locations are the loader's (rswebgl-gltf `geometry.rs`).

layout(location = 0) in vec3 a_position;
#ifdef HAS_NORMAL
layout(location = 1) in vec3 a_normal;
#endif
#ifdef HAS_UV0
layout(location = 2) in vec2 a_uv0;
#endif
#ifdef HAS_TANGENT
layout(location = 3) in vec4 a_tangent;
#endif
#ifdef HAS_COLOR
layout(location = 4) in vec4 a_color;
#endif
#ifdef HAS_UV1
layout(location = 5) in vec2 a_uv1;
#endif

out vec3 v_world;
out vec2 v_uv0;
out vec2 v_uv1;
#ifdef HAS_NORMAL
out vec3 v_normal;
#ifdef HAS_TANGENT
out vec4 v_tangent; // world xyz, w = handedness
#endif
#endif
#ifdef HAS_COLOR
out vec4 v_color;
#endif
// The model's scale per axis (the volume's thickness is in mesh units).
flat out vec3 v_model_scale;

#ifdef HAS_SKIN
// Skinning (after three.js skinning_* chunks): four joints and their weights
// per vertex; joint j's matrix world(joint) × inverse_bind(joint) is in the
// instance texture's slot u_joint_base + j. It replaces the model matrix
// (glTF 2.0 §3.7.3: a skinned mesh's node transform doesn't apply).
layout(location = 6) in vec4 a_joints;
layout(location = 7) in vec4 a_weights;
uniform int u_joint_base;
#endif
#ifdef HAS_SKIN8
// Four more (JOINTS_1 / WEIGHTS_1).
layout(location = 8) in vec4 a_joints1;
layout(location = 9) in vec4 a_weights1;
#endif

#ifdef HAS_MORPH
// Morph targets (after three.js morphtarget_* chunks): deltas in
// u_morph_targets, texel (vertex * u_morph_count + target) * MORPH_KINDS +
// kind (position, normal, tangent — those present), MORPH_TEXTURE_WIDTH a
// row; the weights in the instance texture from slot u_morph_base, four a
// texel. Added before skinning.
#define MORPH_TEXTURE_WIDTH 2048
uniform highp sampler2D u_morph_targets;
uniform int u_morph_base;
uniform int u_morph_count;

vec3 morph_delta(int target, int kind) {
    int t = (gl_VertexID * u_morph_count + target) * MORPH_KINDS + kind;
    return texelFetch(u_morph_targets, ivec2(t % MORPH_TEXTURE_WIDTH, t / MORPH_TEXTURE_WIDTH), 0).xyz;
}

float morph_weight(int target) {
    return instance_texel(u_morph_base * 3 + target / 4)[target % 4];
}
#endif

void main() {
    vec3 position = a_position;
#ifdef HAS_NORMAL
    vec3 normal = a_normal;
#ifdef HAS_TANGENT
    vec3 tangent = a_tangent.xyz;
#endif
#endif
#ifdef HAS_MORPH
    for (int i = 0; i < u_morph_count; i++) {
        float w = morph_weight(i);
        if (w == 0.0) continue;
        int kind = 0;
#ifdef HAS_MORPH_POSITION
        position += w * morph_delta(i, kind++);
#endif
        // A kind's slot counts whether or not this variant uses it.
#ifdef HAS_MORPH_NORMAL
#ifdef HAS_NORMAL
        normal += w * morph_delta(i, kind);
#endif
        kind++;
#endif
#if defined(HAS_MORPH_TANGENT) && defined(HAS_NORMAL) && defined(HAS_TANGENT)
        tangent += w * morph_delta(i, kind);
#endif
    }
#endif

#ifdef HAS_SKIN
    mat4 model = a_weights.x * instance_matrix(u_joint_base + int(a_joints.x))
               + a_weights.y * instance_matrix(u_joint_base + int(a_joints.y))
               + a_weights.z * instance_matrix(u_joint_base + int(a_joints.z))
               + a_weights.w * instance_matrix(u_joint_base + int(a_joints.w));
#ifdef HAS_SKIN8
    model += a_weights1.x * instance_matrix(u_joint_base + int(a_joints1.x))
           + a_weights1.y * instance_matrix(u_joint_base + int(a_joints1.y))
           + a_weights1.z * instance_matrix(u_joint_base + int(a_joints1.z))
           + a_weights1.w * instance_matrix(u_joint_base + int(a_joints1.w));
#endif
    // As three.js: normals by the skin matrix itself (shading normalizes).
    mat3 normal_matrix = mat3(model);
#else
    mat4 model = u_model; // read once
    mat3 normal_matrix = u_normal_matrix;
#endif
    vec4 world = model * vec4(position, 1.0);
    v_world = world.xyz;
#ifdef HAS_NORMAL
    v_normal = normal_matrix * normal;
#ifdef HAS_TANGENT
    v_tangent = vec4(mat3(model) * tangent, a_tangent.w);
#endif
#endif
#ifdef HAS_UV0
    v_uv0 = a_uv0;
#else
    v_uv0 = vec2(0.0);
#endif
#ifdef HAS_UV1
    v_uv1 = a_uv1;
#else
    v_uv1 = vec2(0.0);
#endif
#ifdef HAS_COLOR
    v_color = a_color;
#endif
    v_model_scale = vec3(length(model[0].xyz), length(model[1].xyz), length(model[2].xyz));
    gl_Position = u_view_proj * world;
}

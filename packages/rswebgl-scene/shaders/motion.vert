// Motion vectors of a moving mesh, vertex stage, after the scene prelude
// (`scene.vert.glsl`: camera, instance texture). The vertex now (as the
// scene drew it: jittered, for the raster to match) and where it was last
// frame: last frame's model matrix, joints, morph weights and camera. The
// deformation is pbr.vert's (positions only). Defines: HAS_SKIN, and
// MORPH_KINDS when position morph targets move the mesh.

layout(location = 0) in vec3 a_position;

uniform int u_prev_base;        // last frame's model matrix (slot)
uniform mat4 u_prev_view_proj;  // last frame's camera, unjittered
uniform vec4 u_motion;          // xy this frame's jitter (NDC), z 1: reverse-Z

out vec4 v_now;  // clip now, unjittered
out vec4 v_prev; // clip last frame

#ifdef HAS_SKIN
layout(location = 6) in vec4 a_joints;
layout(location = 7) in vec4 a_weights;
#ifdef HAS_SKIN8
layout(location = 8) in vec4 a_joints1;
layout(location = 9) in vec4 a_weights1;
#endif
uniform int u_joint_base;
uniform int u_prev_joint_base;

mat4 skin_matrix(int base) {
    mat4 m = a_weights.x * instance_matrix(base + int(a_joints.x))
         + a_weights.y * instance_matrix(base + int(a_joints.y))
         + a_weights.z * instance_matrix(base + int(a_joints.z))
         + a_weights.w * instance_matrix(base + int(a_joints.w));
#ifdef HAS_SKIN8
    m += a_weights1.x * instance_matrix(base + int(a_joints1.x))
       + a_weights1.y * instance_matrix(base + int(a_joints1.y))
       + a_weights1.z * instance_matrix(base + int(a_joints1.z))
       + a_weights1.w * instance_matrix(base + int(a_joints1.w));
#endif
    return m;
}
#endif

#ifdef MORPH_KINDS
#define MORPH_TEXTURE_WIDTH 2048
uniform highp sampler2D u_morph_targets;
uniform int u_morph_base;
uniform int u_prev_morph_base;
uniform int u_morph_count;

float morph_weight(int base, int target) {
    return instance_texel(base * 3 + target / 4)[target % 4];
}
#endif

void main() {
    vec3 now = a_position;
    vec3 prev = a_position;
#ifdef MORPH_KINDS
    for (int i = 0; i < u_morph_count; i++) {
        // The position delta: kind 0.
        int t = (gl_VertexID * u_morph_count + i) * MORPH_KINDS;
        vec3 d = texelFetch(u_morph_targets, ivec2(t % MORPH_TEXTURE_WIDTH, t / MORPH_TEXTURE_WIDTH), 0).xyz;
        now += morph_weight(u_morph_base, i) * d;
        prev += morph_weight(u_prev_morph_base, i) * d;
    }
#endif
#ifdef HAS_SKIN
    mat4 model = skin_matrix(u_joint_base);
    mat4 prev_model = skin_matrix(u_prev_joint_base);
#else
    mat4 model = instance_matrix(u_instance_base);
    mat4 prev_model = instance_matrix(u_prev_base);
#endif
    gl_Position = u_view_proj * (model * vec4(now, 1.0));
    v_now = gl_Position;
    v_now.xy -= u_motion.xy * gl_Position.w;
    v_prev = u_prev_view_proj * (prev_model * vec4(prev, 1.0));
}

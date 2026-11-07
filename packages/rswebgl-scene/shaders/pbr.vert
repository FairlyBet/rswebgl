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

void main() {
    vec4 world = u_model * vec4(a_position, 1.0);
    v_world = world.xyz;
#ifdef HAS_NORMAL
    v_normal = u_normal_matrix * a_normal;
#ifdef HAS_TANGENT
    v_tangent = vec4(mat3(u_model) * a_tangent.xyz, a_tangent.w);
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
    gl_Position = u_view_proj * world;
}

// The object, vertex stage: local → world and the inverse-transpose for
// normals, read from the frame's instance texture rather than uniforms, so
// identical meshes draw as one instanced draw. u_instances holds three
// RGBA32F texels per instance — the rows of its affine model matrix —
// INSTANCE_TEXTURE_WIDTH texels a row; this draw's instances start at
// u_instance_base (gl_InstanceID counts from there).
//
// u_model and u_normal_matrix read as before (they are macros here). The
// fragment stage keeps them as uniforms (object.glsl): a shader using them
// there draws each object on its own.

uniform highp sampler2D u_instances;
uniform int u_instance_base;
#define INSTANCE_TEXTURE_WIDTH 1024

// Texel t of the instance texture (row-major, INSTANCE_TEXTURE_WIDTH a row).
vec4 instance_texel(int t) {
    return texelFetch(u_instances, ivec2(t % INSTANCE_TEXTURE_WIDTH, t / INSTANCE_TEXTURE_WIDTH), 0);
}

// The affine matrix in slot i (three texels: its rows). Instances, skin
// joints and morph weights all live in these slots.
mat4 instance_matrix(int i) {
    vec4 r0 = instance_texel(i * 3);
    vec4 r1 = instance_texel(i * 3 + 1);
    vec4 r2 = instance_texel(i * 3 + 2);
    return mat4(r0.x, r1.x, r2.x, 0.0,
                r0.y, r1.y, r2.y, 0.0,
                r0.z, r1.z, r2.z, 0.0,
                r0.w, r1.w, r2.w, 1.0);
}

mat4 object_model() {
    return instance_matrix(u_instance_base + gl_InstanceID);
}

// Inverse-transpose of the upper 3×3: normals stay perpendicular under
// non-uniform scale.
mat3 object_normal_matrix() {
    return transpose(inverse(mat3(object_model())));
}

#define u_model object_model()
#define u_normal_matrix object_normal_matrix()

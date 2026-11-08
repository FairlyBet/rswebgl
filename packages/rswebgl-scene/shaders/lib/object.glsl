// The object, fragment stage (group slot 2): local → world, and the
// inverse-transpose for normals, as uniforms. A shader that uses them here
// draws each object on its own (the vertex stage reads its instance instead:
// object_instanced.glsl).
uniform mat4 u_model;
uniform mat3 u_normal_matrix;

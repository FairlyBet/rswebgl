// One triangle covering the target, from gl_VertexID (draw 3 vertices, no
// vertex array). v_ndc is the clip-space xy.

out vec2 v_ndc;

void main() {
    vec2 p = vec2(float((gl_VertexID << 1) & 2), float(gl_VertexID & 2)) * 2.0 - 1.0;
    v_ndc = p;
    gl_Position = vec4(p, 1.0, 1.0);
}

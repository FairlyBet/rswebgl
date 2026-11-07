// The environment as the background: the specular cube looked up along each
// pixel's view ray, at a mip for blur, through the same output transform as
// the lit scene. Drawn first, without depth, by SceneRenderer. Comes after
// the scene prelude (`scene.frag.glsl`).

in vec2 v_ndc;
out vec4 frag_color;

uniform float u_background_blur; // 0 sharp … 1 the roughest mip

void main() {
    // View-space ray through this pixel, then to world space (the view's
    // rotation is orthonormal: its inverse is its transpose).
    vec3 view_ray = vec3(v_ndc.x / u_proj[0][0], v_ndc.y / u_proj[1][1], -1.0);
    vec3 dir = normalize(transpose(mat3(u_view)) * view_ray);
    frag_color = vec4(scene_output(scene_reflection(dir, u_background_blur)), 1.0);
}

// Motion vectors of a moving mesh, fragment stage: its screen motion since
// last frame (uv now − uv then, unjittered) where it is what the scene
// shows. The depth test is by hand against the scene's depth (the motion
// target has none), with a tolerance: this pass's vertices are computed by
// another program than the scene's. Equal, not only nearer: where the mesh
// lies in front of what the scene shows there, its material discarded the
// fragment (alpha-masked cutouts): what shows there moves on its own.

in vec4 v_now;
in vec4 v_prev;
out vec4 frag_color;

uniform vec4 u_motion; // xy jitter (NDC), z 1: reverse-Z
uniform highp sampler2D u_scene_depth;

void main() {
    float scene = texelFetch(u_scene_depth, ivec2(gl_FragCoord.xy), 0).r;
    // Both ~ near / distance (reverse-Z depth, or 1 − depth for a standard
    // one), so the tolerance is relative to the distance.
    bool reverse = u_motion.z > 0.5;
    float q = reverse ? gl_FragCoord.z : 1.0 - gl_FragCoord.z;
    float q_scene = reverse ? scene : 1.0 - scene;
    if (abs(q - q_scene) > q_scene * 2e-3 || v_prev.w <= 0.0) discard;
    vec2 motion = (v_now.xy / v_now.w - v_prev.xy / v_prev.w) * 0.5;
    frag_color = vec4(motion, 0.0, 1.0);
}

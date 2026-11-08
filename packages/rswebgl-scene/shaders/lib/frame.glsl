// The camera: the frame block (group slot 0).
layout(std140) uniform Frame {
    mat4 u_view;
    mat4 u_proj;
    mat4 u_view_proj;
    vec4 u_camera_pos; // xyz world position of the eye
};

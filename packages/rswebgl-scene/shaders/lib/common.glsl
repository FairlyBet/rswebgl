// Constants and small helpers shared by every shader.

const float PI = 3.14159265359;
const float INV_PI = 0.31830988618;

float saturate(float x) { return clamp(x, 0.0, 1.0); }
vec3 saturate(vec3 x) { return clamp(x, 0.0, 1.0); }
float sq(float x) { return x * x; }
vec3 sq(vec3 x) { return x * x; }
float pow5(float x) { float x2 = x * x; return x2 * x2 * x; }
float max3(vec3 v) { return max(v.x, max(v.y, v.z)); }

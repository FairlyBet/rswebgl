# Third-party code

Parts of the shaders in `packages/rswebgl-scene/shaders/` are ported from
the projects below. Each ported file says in its header what it took and
from where. The originals' licenses apply to those parts.

| Project | License | Used in |
|---|---|---|
| [Khronos glTF-Sample-Renderer](https://github.com/KhronosGroup/glTF-Sample-Renderer) | Apache-2.0 | glTF reference model (`lib/surface_gltf.glsl`), GGX/Smith/Schlick (`lib/brdf.glsl`), transmission (`lib/surface.glsl`), sheen (`lib/sheen.glsl`), iridescence (`lib/iridescence.glsl`), the BRDF table's integration (`src/brdf_lut.rs`) |
| [MaterialX](https://github.com/AcademySoftwareFoundation/MaterialX) | Apache-2.0 | exact dielectric and F82 Fresnel, GGX directional-albedo fit, energy compensation (`lib/brdf.glsl`), EON diffuse (`lib/eon.glsl`), Zeltner fuzz (`lib/sheen.glsl`), Airy thin film (`lib/iridescence.glsl`), Burley-profile subsurface approximation (`lib/subsurface.glsl`), the OpenPBR layering it follows (`lib/surface_openpbr.glsl`) |
| [OpenPBR](https://github.com/AcademySoftwareFoundation/OpenPBR) | Apache-2.0 | the model `lib/surface_openpbr.glsl` implements |
| [Filament](https://github.com/google/filament) | Apache-2.0 | specular antialiasing and specular occlusion (`lib/brdf.glsl`) |
| [Bevy](https://github.com/bevyengine/bevy) | MIT / Apache-2.0 | the TAA resolve (`taa.frag`: Catmull-Rom history, YCoCg variance clip, confidence, reversible tone map; Halton jitter sequence), with Playdead's YCoCg helpers (MIT) |
| [three.js](https://github.com/mrdoob/three.js) | MIT | ACES and AgX tone mapping (`lib/tonemap.glsl`); skinning and morph target vertex code (`pbr.vert`, after its `skinning_*` / `morphtarget_*` chunks) |

Apache License 2.0: https://www.apache.org/licenses/LICENSE-2.0.
Changes from the originals: renamed and restructured into this project's
chunks; per-file notes describe behavioural differences.

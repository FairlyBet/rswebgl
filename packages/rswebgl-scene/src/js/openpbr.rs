//! OpenPBR Surface parameter names (`base_color`, `coat_weight`, …) for the
//! material API, next to glTF's (`baseColorFactor`, `clearcoatFactor`, …).
//! Materials store glTF's parameters plus OpenPBR's own; each OpenPBR name
//! reads and writes the ones that carry it:
//!
//! | OpenPBR | stored as |
//! |---|---|
//! | base_weight, base_diffuse_roughness | baseWeight, diffuseRoughness |
//! | base_color, geometry_opacity | baseColorFactor rgb, a |
//! | base_metalness | metallicFactor |
//! | specular_weight, specular_color, specular_roughness, specular_ior | specularFactor, specularColorFactor, roughnessFactor, ior |
//! | specular_roughness_anisotropy | anisotropyStrength |
//! | transmission_weight, transmission_color, transmission_depth | transmissionFactor, attenuationColor, attenuationDistance |
//! | transmission_dispersion_abbe_number | dispersion = 20 / Abbe |
//! | coat_weight, coat_roughness, coat_ior, coat_color, coat_darkening | clearcoatFactor, clearcoatRoughnessFactor, coatIor, coatColor, coatDarkening |
//! | fuzz_weight, fuzz_color, fuzz_roughness | sheenColorFactor = weight × color, sheenRoughnessFactor |
//! | thin_film_weight, thin_film_thickness (µm), thin_film_ior | iridescenceFactor, iridescenceThickness (nm), iridescenceIor |
//! | emission_luminance, emission_color | emissiveStrength, emissiveFactor |
//!
//! Emission is radiance in the scene's units, as glTF's (not photometric
//! nits). A volume's depth needs `thicknessFactor` > 0 to absorb (glTF's
//! convention); subsurface parameters aren't supported.

use rswebgl_gltf::Material;

/// Stored properties to write: (glTF-side name, values).
pub type Writes = Vec<(&'static str, Vec<f32>)>;

/// Every OpenPBR name the API takes.
pub const NAMES: [&str; 27] = [
    "base_weight",
    "base_color",
    "base_metalness",
    "base_diffuse_roughness",
    "specular_weight",
    "specular_color",
    "specular_roughness",
    "specular_ior",
    "specular_roughness_anisotropy",
    "transmission_weight",
    "transmission_color",
    "transmission_depth",
    "transmission_dispersion_abbe_number",
    "coat_weight",
    "coat_color",
    "coat_roughness",
    "coat_ior",
    "coat_darkening",
    "fuzz_weight",
    "fuzz_color",
    "fuzz_roughness",
    "thin_film_weight",
    "thin_film_thickness",
    "thin_film_ior",
    "emission_luminance",
    "emission_color",
    "geometry_opacity",
];

// Names that are one stored property as is.
const DIRECT: [(&str, &str); 18] = [
    ("base_weight", "baseWeight"),
    ("base_metalness", "metallicFactor"),
    ("base_diffuse_roughness", "diffuseRoughness"),
    ("specular_weight", "specularFactor"),
    ("specular_color", "specularColorFactor"),
    ("specular_roughness", "roughnessFactor"),
    ("specular_ior", "ior"),
    ("specular_roughness_anisotropy", "anisotropyStrength"),
    ("transmission_weight", "transmissionFactor"),
    ("transmission_color", "attenuationColor"),
    ("transmission_depth", "attenuationDistance"),
    ("coat_weight", "clearcoatFactor"),
    ("coat_color", "coatColor"),
    ("coat_roughness", "clearcoatRoughnessFactor"),
    ("coat_ior", "coatIor"),
    ("coat_darkening", "coatDarkening"),
    ("fuzz_roughness", "sheenRoughnessFactor"),
    ("thin_film_weight", "iridescenceFactor"),
];

fn prop(m: &Material, name: &str) -> Vec<f32> {
    m.property(name).unwrap_or_default()
}

// The fuzz as (weight, color) from the sheen color; without fuzz the color
// can't be recovered from it, so `remembered` (the last one set) stands in.
fn fuzz(m: &Material, remembered: [f32; 3]) -> (f32, Vec<f32>) {
    let sheen = prop(m, "sheenColorFactor");
    let w = sheen.iter().copied().fold(0.0, f32::max);
    let color = if w > 0.0 {
        sheen.iter().map(|c| c / w).collect()
    } else {
        remembered.to_vec()
    };
    (w, color)
}

/// The value of OpenPBR parameter `name`; None if it isn't one.
pub fn get(m: &Material, name: &str, fuzz_color: [f32; 3]) -> Option<Vec<f32>> {
    if let Some((_, p)) = DIRECT.iter().find(|(n, _)| *n == name) {
        return Some(prop(m, p));
    }
    let base = prop(m, "baseColorFactor");
    Some(match name {
        "base_color" => base[..3].to_vec(),
        "geometry_opacity" => vec![base[3]],
        "transmission_dispersion_abbe_number" => {
            let d = prop(m, "dispersion")[0];
            vec![if d > 0.0 { 20.0 / d } else { 0.0 }]
        }
        "fuzz_weight" => vec![fuzz(m, fuzz_color).0],
        "fuzz_color" => fuzz(m, fuzz_color).1,
        "thin_film_thickness" => vec![prop(m, "iridescenceThickness")[0] / 1000.0],
        "thin_film_ior" => prop(m, "iridescenceIor"),
        "emission_luminance" => prop(m, "emissiveStrength"),
        "emission_color" => prop(m, "emissiveFactor"),
        _ => return None,
    })
}

/// The stored properties to write for OpenPBR parameter `name` = `values`;
/// None if it isn't one, Err for the wrong number of values.
pub fn set(
    m: &Material,
    name: &str,
    values: &[f32],
    fuzz_color: [f32; 3],
) -> Option<Result<Writes, String>> {
    let want = |n: usize| -> Result<(), String> {
        if values.len() == n {
            Ok(())
        } else {
            Err(format!(
                "\"{name}\" takes {n} value(s), got {}",
                values.len()
            ))
        }
    };
    if let Some((_, p)) = DIRECT.iter().find(|(n, _)| *n == name) {
        let len = prop(m, p).len();
        return Some(want(len).map(|()| vec![(*p, values.to_vec())]));
    }
    let base = prop(m, "baseColorFactor");
    let result = match name {
        "base_color" => want(3).map(|()| {
            vec![(
                "baseColorFactor",
                vec![values[0], values[1], values[2], base[3]],
            )]
        }),
        "geometry_opacity" => want(1).map(|()| {
            vec![(
                "baseColorFactor",
                vec![base[0], base[1], base[2], values[0]],
            )]
        }),
        "transmission_dispersion_abbe_number" => want(1).map(|()| {
            let d = if values[0] > 0.0 {
                20.0 / values[0]
            } else {
                0.0
            };
            vec![("dispersion", vec![d])]
        }),
        "fuzz_weight" => want(1).map(|()| {
            let (_, color) = fuzz(m, fuzz_color);
            vec![(
                "sheenColorFactor",
                color.iter().map(|c| c * values[0]).collect(),
            )]
        }),
        "fuzz_color" => want(3).map(|()| {
            let (w, _) = fuzz(m, fuzz_color);
            // Without fuzz the color is only remembered (the caller keeps it).
            vec![("sheenColorFactor", values.iter().map(|c| c * w).collect())]
        }),
        "thin_film_thickness" => {
            want(1).map(|()| vec![("iridescenceThickness", vec![values[0] * 1000.0])])
        }
        "thin_film_ior" => want(1).map(|()| vec![("iridescenceIor", values.to_vec())]),
        "emission_luminance" => want(1).map(|()| vec![("emissiveStrength", values.to_vec())]),
        "emission_color" => want(3).map(|()| vec![("emissiveFactor", values.to_vec())]),
        _ => return None,
    };
    Some(result)
}

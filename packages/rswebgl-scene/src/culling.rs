use glam::{Mat4, Vec3, Vec4, Vec4Swizzles};

/// The entity's local axis-aligned bounding box, in the space of its
/// `Transform` (before the world matrix). An entity without one is never
/// culled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    /// The smallest box around `points` (x, y, z triples), or None for none.
    pub fn from_points(points: &[f32]) -> Option<Self> {
        let mut it = points.as_chunks::<3>().0.iter().map(|&p| Vec3::from(p));
        let first = it.next()?;
        Some(it.fold(Self::new(first, first), |b, p| {
            Self::new(b.min.min(p), b.max.max(p))
        }))
    }

    /// Center and half-size in world space after `m`: the box around the
    /// transformed box (conservative under rotation).
    pub fn transformed(&self, m: &Mat4) -> (Vec3, Vec3) {
        let center = (self.min + self.max) * 0.5;
        let half = (self.max - self.min) * 0.5;
        let c = m.transform_point3(center);
        let h = m.x_axis.xyz().abs() * half.x
            + m.y_axis.xyz().abs() * half.y
            + m.z_axis.xyz().abs() * half.z;
        (c, h)
    }
}

/// The six planes of a view-projection's clip volume, normals pointing in.
#[derive(Debug, Clone, Copy)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    /// From a GL-style view-projection (clip z in `[-w, w]`).
    pub fn from_view_proj(m: &Mat4) -> Self {
        let r = [m.row(0), m.row(1), m.row(2), m.row(3)];
        let planes = [
            r[3] + r[0],
            r[3] - r[0],
            r[3] + r[1],
            r[3] - r[1],
            r[3] + r[2],
            r[3] - r[2],
        ]
        .map(|p| {
            let len = p.xyz().length();
            if len > 0.0 { p / len } else { p }
        });
        Self { planes }
    }

    /// Whether a box (world center and half-size) is at least partly inside.
    /// Conservative: a box near a corner can pass while outside.
    pub fn intersects(&self, center: Vec3, half: Vec3) -> bool {
        self.planes.iter().all(|p| {
            let n = p.xyz();
            n.dot(center) + p.w >= -n.abs().dot(half)
        })
    }
}

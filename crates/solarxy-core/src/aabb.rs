use cgmath::{InnerSpace, Point3, Vector3};

#[derive(Clone, Copy)]
pub struct AABB {
    pub min: Point3<f32>,
    pub max: Point3<f32>,
}

impl AABB {
    pub fn center(&self) -> Point3<f32> {
        Point3::new(
            f32::midpoint(self.min.x, self.max.x),
            f32::midpoint(self.min.y, self.max.y),
            f32::midpoint(self.min.z, self.max.z),
        )
    }

    pub fn diagonal(&self) -> f32 {
        (self.max - self.min).magnitude()
    }

    pub fn half_extents(&self) -> Vector3<f32> {
        Vector3::new(
            (self.max.x - self.min.x) * 0.5,
            (self.max.y - self.min.y) * 0.5,
            (self.max.z - self.min.z) * 0.5,
        )
    }

    pub fn size(&self) -> Vector3<f32> {
        Vector3::new(
            self.max.x - self.min.x,
            self.max.y - self.min.y,
            self.max.z - self.min.z,
        )
    }

    pub fn corners(&self) -> [Point3<f32>; 8] {
        let (mn, mx) = (self.min, self.max);
        [
            Point3::new(mn.x, mn.y, mn.z),
            Point3::new(mx.x, mn.y, mn.z),
            Point3::new(mn.x, mx.y, mn.z),
            Point3::new(mx.x, mx.y, mn.z),
            Point3::new(mn.x, mn.y, mx.z),
            Point3::new(mx.x, mn.y, mx.z),
            Point3::new(mn.x, mx.y, mx.z),
            Point3::new(mx.x, mx.y, mx.z),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_cube() -> AABB {
        AABB {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(1.0, 1.0, 1.0),
        }
    }

    #[test]
    fn size_and_derived_metrics() {
        let aabb = AABB {
            min: Point3::new(-2.0, 0.0, 3.0),
            max: Point3::new(4.0, 6.0, 9.0),
        };
        let s = aabb.size();
        assert!((s.x - 6.0).abs() < f32::EPSILON);
        assert!((s.y - 6.0).abs() < f32::EPSILON);
        assert!((s.z - 6.0).abs() < f32::EPSILON);

        let he = aabb.half_extents();
        assert!((he.x - 3.0).abs() < f32::EPSILON);
        assert!((he.y - 3.0).abs() < f32::EPSILON);

        let c = aabb.center();
        assert!((c.x - 1.0).abs() < f32::EPSILON);
        assert!((c.y - 3.0).abs() < f32::EPSILON);
        assert!((c.z - 6.0).abs() < f32::EPSILON);

        let d = aabb.diagonal();
        let expected = (6.0_f32 * 6.0 + 6.0 * 6.0 + 6.0 * 6.0).sqrt();
        assert!((d - expected).abs() < 1e-6);
    }

    #[test]
    fn zero_volume_aabb() {
        let aabb = AABB {
            min: Point3::new(3.0, 4.0, 5.0),
            max: Point3::new(3.0, 4.0, 5.0),
        };
        assert!((aabb.diagonal()).abs() < f32::EPSILON);
        assert!((aabb.size().x).abs() < f32::EPSILON);
        assert_eq!(aabb.center(), Point3::new(3.0, 4.0, 5.0));
    }

    #[test]
    fn corners_ordering() {
        let c = unit_cube().corners();
        assert_eq!(c[0], Point3::new(0.0, 0.0, 0.0));
        assert_eq!(c[1], Point3::new(1.0, 0.0, 0.0));
        assert_eq!(c[2], Point3::new(0.0, 1.0, 0.0));
        assert_eq!(c[3], Point3::new(1.0, 1.0, 0.0));
        assert_eq!(c[4], Point3::new(0.0, 0.0, 1.0));
        assert_eq!(c[5], Point3::new(1.0, 0.0, 1.0));
        assert_eq!(c[6], Point3::new(0.0, 1.0, 1.0));
        assert_eq!(c[7], Point3::new(1.0, 1.0, 1.0));

        let aabb = AABB {
            min: Point3::new(-3.0, -2.0, -1.0),
            max: Point3::new(4.0, 5.0, 6.0),
        };
        let c2 = aabb.corners();
        assert_eq!(c2[0], aabb.min);
        assert_eq!(c2[7], aabb.max);
    }
}

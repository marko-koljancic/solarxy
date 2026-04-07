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
}

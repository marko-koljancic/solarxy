//! Direct lights ([`LightEntry`]) plus the consolidated [`LightsUniform`]
//! pushed to the GPU. The CPU-side L0 SH ambient comes from `IblState` and is
//! merged here before upload.

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightEntry {
    pub position: [f32; 3],
    pub _pad0: f32,
    pub color: [f32; 3],
    pub intensity: f32,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightsUniform {
    pub lights: [LightEntry; 3],
    pub sphere_scale: f32,
    pub ibl_avg_r: f32,
    pub ibl_avg_g: f32,
    pub ibl_avg_b: f32,
}

const _: () = assert!(std::mem::size_of::<LightEntry>() == 32);
const _: () = assert!(std::mem::size_of::<LightsUniform>() == 112);

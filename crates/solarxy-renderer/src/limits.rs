//! The device limits both GPU shells request, and the ceilings a buffer
//! allocation is judged against.
//!
//! Both shells used to request [`wgpu::Limits::default()`] and never read
//! what their adapter offered, so a model whose vertex data cleared the
//! core WebGPU floor of 256 MiB failed to allocate on hardware offering
//! gigabytes. [`required_limits`] raises the size limits off the adapter's
//! report; [`buffer_ceiling`] is the other half, so a mesh that still will
//! not fit is refused with a message rather than a dead viewport.
//!
//! # Why this is not `Limits::or_better_values_from`
//!
//! wgpu offers exactly that one-liner, and it is wrong here. It walks every
//! field of its internal table and takes the better of the two, which
//! raises the **count** limits along with the sizes. The path tracer's
//! scene bind group binds six compute-stage storage buffers against core
//! WebGPU's budget of eight, and `bind_groups.rs` records that the design
//! spends that budget deliberately. Raising a count limit would let a later
//! change quietly exceed what the target platform guarantees and fail only
//! on the machines that guarantee least. So the merge below names the
//! fields it touches, and adding one is a decision rather than a default.

/// The limits to request at device creation: the core WebGPU defaults with
/// the buffer size ceilings raised to whatever `adapter` reports.
///
/// [`wgpu::Limits::default()`] stays the base and **no field is ever
/// lowered from it**, which is what keeps a device request valid on any
/// conformant adapter: each field takes the larger of the two values, so an
/// adapter reporting below the baseline still yields the baseline.
///
/// Two fields are raised, and they are the two that bind real geometry:
///
/// - `max_buffer_size` (256 MiB by default) caps every allocation, so it is
///   what a large mesh's vertex or index buffer hits first.
/// - `max_storage_buffer_binding_size` (128 MiB by default, half the above)
///   caps a storage binding. The edge position and edge index buffers are
///   `STORAGE`, so on the raster path this is the *tighter* wall, and it is
///   also the binding limit for the traced arena's storage buffers. Raising
///   only the first would move the failure rather than fix it.
///
/// Everything else is left at the default on purpose. The count limits are
/// argued above. `max_uniform_buffer_binding_size` is untouched because no
/// uniform in this renderer approaches 64 KiB, and the texture dimension
/// limits because the capture path clamps against `device.limits()` and its
/// budget is a deliberate ceiling rather than a hardware one. The two
/// `min_*_offset_alignment` fields are minima, where a raise would mean a
/// worse value, and are the reason this merge is written field by field
/// rather than as a loop over "take the better one".
#[must_use]
pub fn required_limits(adapter: &wgpu::Limits) -> wgpu::Limits {
    let base = wgpu::Limits::default();
    wgpu::Limits {
        max_buffer_size: base.max_buffer_size.max(adapter.max_buffer_size),
        max_storage_buffer_binding_size: base
            .max_storage_buffer_binding_size
            .max(adapter.max_storage_buffer_binding_size),
        ..base
    }
}

/// The largest allocation `usage` permits on a device with these limits.
///
/// `max_buffer_size` bounds every buffer; a binding limit bounds it further
/// when the buffer is bound as one. A buffer carrying several such usages
/// takes the smallest ceiling that applies to it.
#[must_use]
pub fn buffer_ceiling(limits: &wgpu::Limits, usage: wgpu::BufferUsages) -> u64 {
    let mut ceiling = limits.max_buffer_size;
    if usage.contains(wgpu::BufferUsages::STORAGE) {
        ceiling = ceiling.min(u64::from(limits.max_storage_buffer_binding_size));
    }
    if usage.contains(wgpu::BufferUsages::UNIFORM) {
        ceiling = ceiling.min(u64::from(limits.max_uniform_buffer_binding_size));
    }
    ceiling
}

/// A byte count as a person reads it.
///
/// Binary divisors with binary labels. The desktop shell carries its own
/// formatter for the Properties panel which divides by 1024 and labels the
/// result "MB"; this one is not shared with it, because that one is private
/// to an egui module the renderer cannot reach and correcting its labels
/// would change text the panel has always shown, which is not this fix.
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = 1024 * KIB;
    const GIB: u64 = 1024 * MIB;
    match bytes {
        b if b >= GIB => format!("{:.1} GiB", b as f64 / GIB as f64),
        b if b >= MIB => format!("{:.1} MiB", b as f64 / MIB as f64),
        b if b >= KIB => format!("{:.1} KiB", b as f64 / KIB as f64),
        b => format!("{b} B"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An adapter offering more than the baseline raises both size fields
    /// and nothing else. The count fields are listed by name rather than
    /// compared as a whole struct, so a wgpu upgrade that adds a field
    /// cannot quietly start raising it.
    #[test]
    fn an_adapter_raises_the_size_fields_and_no_count_field() {
        let base = wgpu::Limits::default();
        let adapter = wgpu::Limits {
            max_buffer_size: 4 << 30,
            max_storage_buffer_binding_size: 2 << 30,
            max_storage_buffers_per_shader_stage: 64,
            max_bind_groups: 8,
            max_bindings_per_bind_group: 4096,
            max_sampled_textures_per_shader_stage: 1024,
            max_uniform_buffer_binding_size: 1 << 30,
            max_texture_dimension_2d: 32768,
            ..base
        };
        let got = required_limits(&adapter);

        assert_eq!(got.max_buffer_size, 4 << 30);
        assert_eq!(got.max_storage_buffer_binding_size, 2 << 30);

        assert_eq!(
            got.max_storage_buffers_per_shader_stage, base.max_storage_buffers_per_shader_stage,
            "the tracer's scene group is designed against the core budget"
        );
        assert_eq!(got.max_bind_groups, base.max_bind_groups);
        assert_eq!(
            got.max_bindings_per_bind_group,
            base.max_bindings_per_bind_group
        );
        assert_eq!(
            got.max_sampled_textures_per_shader_stage,
            base.max_sampled_textures_per_shader_stage
        );
        assert_eq!(
            got.max_uniform_buffer_binding_size,
            base.max_uniform_buffer_binding_size
        );
        assert_eq!(got.max_texture_dimension_2d, base.max_texture_dimension_2d);
    }

    /// An adapter reporting below the WebGPU baseline yields the baseline,
    /// which is what makes the request valid rather than impossible.
    #[test]
    fn a_poorer_adapter_lowers_nothing() {
        let base = wgpu::Limits::default();
        let adapter = wgpu::Limits {
            max_buffer_size: 16 << 20,
            max_storage_buffer_binding_size: 8 << 20,
            ..base
        };
        let got = required_limits(&adapter);
        assert_eq!(got.max_buffer_size, base.max_buffer_size);
        assert_eq!(
            got.max_storage_buffer_binding_size,
            base.max_storage_buffer_binding_size
        );
    }

    /// The default request is unchanged by an adapter that reports it.
    #[test]
    fn an_adapter_at_the_baseline_changes_nothing() {
        let base = wgpu::Limits::default();
        assert_eq!(required_limits(&base), base);
    }

    /// A storage buffer is bounded by the binding limit, which is half the
    /// buffer ceiling at the defaults, and a vertex buffer is not.
    #[test]
    fn a_storage_binding_has_the_tighter_ceiling() {
        let limits = wgpu::Limits::default();
        let vertex = buffer_ceiling(
            &limits,
            wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        );
        let storage = buffer_ceiling(
            &limits,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        );
        assert_eq!(vertex, 256 << 20);
        assert_eq!(storage, 128 << 20);
        assert!(storage < vertex);
    }

    /// Raising the limits raises both ceilings, which is the point.
    #[test]
    fn raising_the_limits_raises_both_ceilings() {
        let limits = required_limits(&wgpu::Limits {
            max_buffer_size: 4 << 30,
            max_storage_buffer_binding_size: 2 << 30,
            ..wgpu::Limits::default()
        });
        assert_eq!(buffer_ceiling(&limits, wgpu::BufferUsages::VERTEX), 4 << 30);
        assert_eq!(
            buffer_ceiling(&limits, wgpu::BufferUsages::STORAGE),
            2 << 30
        );
    }

    #[test]
    fn byte_counts_read_as_a_person_reads_them() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KiB");
        assert_eq!(format_bytes(256 << 20), "256.0 MiB");
        assert_eq!(format_bytes(294_290_388), "280.7 MiB");
        assert_eq!(format_bytes(4 << 30), "4.0 GiB");
    }
}

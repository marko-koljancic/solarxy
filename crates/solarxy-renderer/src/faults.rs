//! The uncaptured-error hook both shells install on their device.
//!
//! wgpu delivers an error to the innermost matching error scope, then to
//! the uncaptured handler, and only with neither installed to its default
//! handler, which panics. The desktop therefore died on any validation
//! error with nothing a user could read, while the browser wrote to its
//! own console and carried on; that asymmetry is what let a buffer's
//! missing usage flag ship unnoticed. One shared hook makes the shells
//! agree: the full message is logged on the `solarxy::gpu` target, a
//! compact record lands in a bounded queue, and each shell drains the
//! queue once per frame into whatever it shows people.
//!
//! Deliberately NOT here: device-loss handling. A lost device is not a bad
//! frame; every resource is gone and the answer is rebuilding them, not
//! logging. Conflating the two paths in one handler would hide that
//! difference.

use std::sync::{Arc, Mutex};

/// Which class of GPU fault a report carries. The three deserve different
/// wording: a validation error is a Solarxy bug and should be loud, out of
/// memory is a capacity problem the user can act on, and an internal error
/// is the driver's or the backend's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuFaultKind {
    Validation,
    OutOfMemory,
    Internal,
}

/// One drained report: the newest full message of a run of identical
/// errors, and how many errors that record stands for.
#[derive(Debug, Clone)]
pub struct GpuFault {
    pub kind: GpuFaultKind,
    pub message: String,
    pub count: u32,
}

/// Distinct records kept between drains. Identical consecutive errors
/// collapse into one record, so the cap only matters when DIFFERENT errors
/// alternate faster than the shell drains; then the newest folds into the
/// last slot rather than growing the queue.
const PENDING_CAP: usize = 8;

/// The handler's shared queue. [`install`] clones one end into the wgpu
/// callback; the shell keeps the other and calls [`GpuFaults::drain`] once
/// per frame.
#[derive(Clone, Default)]
pub struct GpuFaults {
    inner: Arc<Mutex<Vec<GpuFault>>>,
}

impl GpuFaults {
    /// Records one uncaptured error. Runs inside the wgpu callback: any
    /// thread, after wgpu has released its own locks, and it must not
    /// re-enter wgpu (it does not).
    fn record(&self, error: &wgpu::Error) {
        let kind = match error {
            wgpu::Error::OutOfMemory { .. } => GpuFaultKind::OutOfMemory,
            wgpu::Error::Validation { .. } => GpuFaultKind::Validation,
            wgpu::Error::Internal { .. } => GpuFaultKind::Internal,
        };
        let message = error.to_string();
        // The full message, unabridged: the only thing a bug report can be
        // written from. The toast a shell raises from the drained record
        // is deliberately shorter and points at the log.
        tracing::error!(target: "solarxy::gpu", "{message}");
        let Ok(mut pending) = self.inner.lock() else {
            return;
        };
        let full = pending.len() >= PENDING_CAP;
        if let Some(last) = pending.last_mut() {
            if last.kind == kind && last.message == message {
                last.count = last.count.saturating_add(1);
                return;
            }
            if full {
                let folded = last.count.saturating_add(1);
                *last = GpuFault {
                    kind,
                    message,
                    count: folded,
                };
                return;
            }
        }
        pending.push(GpuFault {
            kind,
            message,
            count: 1,
        });
    }

    /// Takes everything recorded since the last drain.
    #[must_use]
    pub fn drain(&self) -> Vec<GpuFault> {
        self.inner
            .lock()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default()
    }
}

/// Installs the shared uncaptured-error handler on a device. Call
/// immediately after the device is created, before anything uses it.
///
/// Error scopes are unaffected: wgpu consults the innermost matching scope
/// first, so a deliberately scoped error (the headless smoke suite, the
/// path-tracing probes) is still caught by its scope and never reaches
/// this hook.
#[must_use]
pub fn install(device: &wgpu::Device) -> GpuFaults {
    let sink = GpuFaults::default();
    let handler = sink.clone();
    device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| handler.record(&error)));
    sink
}

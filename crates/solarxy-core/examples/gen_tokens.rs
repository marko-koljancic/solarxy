//! Emits the web frontend's color tokens from `solarxy_core::theme`.
//!
//! The palette is the single source of truth for every shell: the egui GUI
//! and the analyze TUI read `theme::Palette` directly, and this renders the
//! same data as CSS custom properties for `web/`. A color authored once
//! therefore lands on all three surfaces, which is what stops the drift
//! that produced a green "change" pin on desktop and a red one on web.
//!
//! ```text
//! cargo run -p solarxy-core --example gen_tokens > web/src/styles/tokens.generated.css
//! ```
//!
//! `tests/tokens_drift.rs` asserts the checked-in file matches this output,
//! so a palette edit without a regenerate fails CI. The rendering itself
//! lives in `theme::generate_css` rather than here, because cargo does not
//! run an example's unit tests.

fn main() {
    print!("{}", solarxy_core::theme::generate_css());
}

//! The attribute channel on this shell: the strip's state, the rebuild of
//! the label set and the arrow lines when it or the scene changes, and the
//! lane inventory the strip's picker offers.
//!
//! The assembly is `solarxy_host::attr_channel`, shared with the browser
//! host; what stays here is what the channel reads from and writes to:
//! the engine's displayed geometries on one side, the renderer's label
//! and line buffers on the other, and the theme colours the labels wear.

use solarxy_host::attr_viz::AttrVizState;

use super::State;

/// One point lane the strip's picker offers: its name and its type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SceneLane {
    pub name: String,
    pub ty: &'static str,
}

impl State {
    /// What the playbar shows, read from the engine's clock each frame.
    pub(super) fn transport_readout(&self) -> crate::gui::TransportReadout {
        self.engine
            .as_deref()
            .map_or_else(crate::gui::TransportReadout::default, |engine| {
                let clock = engine.clock();
                let (start, end) = clock.effective_range();
                crate::gui::TransportReadout {
                    open: true,
                    playing: clock.playing,
                    frame: clock.frame,
                    start,
                    end,
                    fps: clock.effective_fps(),
                    loop_mode: clock.loop_mode,
                }
            })
    }

    /// The displayed geometries as the shared channel takes them: each
    /// cooked set with the world matrix its object is placed by.
    fn displayed_geometries(&self) -> Vec<solarxy_host::attr_channel::DisplayedGeometry> {
        self.engine.as_deref().map_or_else(Vec::new, |engine| {
            engine
                .display_geometries()
                .into_iter()
                .map(|(_node, set, m)| (set, m))
                .collect()
        })
    }

    /// Replace the strip's state, from any of its controls. The whole state
    /// travels, as the browser's does, so a control cannot leave a sibling
    /// field behind.
    pub(super) fn set_attr_viz(&mut self, viz: AttrVizState) {
        if viz != self.attr_viz {
            self.attr_viz = viz;
            self.attr_dirty = true;
        }
    }

    /// Rebuild, or clear, both attribute channels when they are stale, and
    /// record the sampling facts the strip's notice reports.
    ///
    /// One consumer of the flag by construction: splitting the channels
    /// over two consumers would starve whichever ran second. Independent of
    /// the overlays rebuild: the channels draw whenever the strip enables
    /// them, with or without the normals and bounds overlays.
    pub(super) fn sync_attr_channels(&mut self) {
        if !self.attr_dirty {
            return;
        }
        self.attr_dirty = false;
        self.push_label_style();

        let geos = self.displayed_geometries();
        if self.attr_viz.vectors && self.attr_viz.name.is_some() {
            let lines = solarxy_host::attr_channel::build_vector_lines(&geos, &self.attr_viz);
            self.env.vis.set_attr_lines(&self.device, &lines);
        } else if self.env.vis.attr_lines_count > 0 {
            self.env.vis.set_attr_lines(&self.device, &[]);
        }

        let (instances, words, capacity, total) =
            solarxy_host::attr_channel::build_label_set(&geos, &self.attr_viz);
        self.renderer
            .set_attr_labels(&self.device, &self.queue, &instances, &words);
        self.attr_pin_stats = (capacity, total);
    }

    /// The label style from its two halves: the theme's three colours, the
    /// same roles the browser reads (text, elevated background, accent),
    /// and the size, background and opacity the strip's settings pick.
    fn push_label_style(&mut self) {
        let [text, chip, dot] = self.gui.label_colors();
        let base = solarxy_renderer::labels::LabelStyle {
            text,
            chip,
            dot,
            dpr: self.window.scale_factor() as f32,
            ..solarxy_renderer::labels::LabelStyle::new_default()
        };
        let style = self.attr_viz.apply_to_style(base);
        self.renderer.write_label_style(&self.queue, &style);
    }

    /// The union of point lanes across every displayed geometry, by name,
    /// for the strip's picker. Sorted, as the browser sorts its list.
    pub(super) fn scene_lanes(&self) -> Vec<SceneLane> {
        let Some(engine) = self.engine.as_deref() else {
            return Vec::new();
        };
        let mut lanes: Vec<SceneLane> = Vec::new();
        for (node, _, _) in engine.display_geometries() {
            let Some(summary) = engine.attribute_summary(node) else {
                continue;
            };
            for lane in summary.point {
                if !lanes.iter().any(|l| l.name == lane.name) {
                    lanes.push(SceneLane {
                        name: lane.name,
                        ty: lane.ty,
                    });
                }
            }
        }
        lanes.sort_by(|a, b| a.name.cmp(&b.name));
        lanes
    }
}

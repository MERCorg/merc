use std::sync::Arc;

use cosmic_text::Metrics;
use glam::Vec2;
use glam::Vec3Swizzles;
use kurbo::Affine;
use kurbo::BezPath;
use kurbo::Circle;
use kurbo::Point;
use kurbo::Shape;
use merc_lts::LTS;
use merc_lts::LabelledTransitionSystem;

use crate::Viewer;
use crate::text_cache::TextCache;

/// The geometry of a single frame, shared between the vello backends.
#[derive(Default)]
pub(crate) struct FrameGeometry {
    /// Edge lines, edge handles and self loops.
    pub edges: BezPath,
    /// Arrow heads of the edges.
    pub arrows: BezPath,
    /// All states except the initial state.
    pub states: BezPath,
    /// The initial state.
    pub initial_state: BezPath,
    /// The label index and position (in graph space) for every label to draw.
    pub labels: Vec<(usize, Vec2)>,
}

/// Shared scene construction for the vello CPU and GPU renderers.
pub(crate) struct VelloScene {
    lts: Arc<LabelledTransitionSystem<String>>,
    text_cache: TextCache,

    /// The shaped label buffers.
    buffers: Vec<cosmic_text::Buffer>,

    /// The outlines of the labels, with the origin at the label position.
    outlines: Vec<BezPath>,

    /// The text size for which the outlines were generated.
    outline_size: f32,
}

impl VelloScene {
    pub(crate) fn new(lts: Arc<LabelledTransitionSystem<String>>) -> Self {
        let mut text_cache = TextCache::new();
        let buffers: Vec<_> = lts
            .labels()
            .iter()
            .map(|label| text_cache.create_buffer(label, Metrics::new(12.0, 12.0)))
            .collect();

        VelloScene {
            lts,
            text_cache,
            buffers,
            outlines: vec![],
            outline_size: 0.0,
        }
    }

    /// Returns the transform from graph space to screen space.
    pub(crate) fn view_transform(view_x: f32, view_y: f32, screen_x: u32, screen_y: u32, zoom_level: f32) -> Affine {
        Affine::translate((screen_x as f64 / 2.0, screen_y as f64 / 2.0))
            * Affine::scale(zoom_level as f64)
            * Affine::translate((view_x as f64, view_y as f64))
    }

    /// Returns the outline of the given label for the text size given to [`Self::update_labels`].
    pub(crate) fn label_outline(&self, label: usize) -> &BezPath {
        &self.outlines[label]
    }

    /// Regenerates the label outlines when the text size has changed.
    pub(crate) fn update_labels(&mut self, label_text_size: f32) {
        if self.outlines.len() == self.buffers.len() && self.outline_size == label_text_size {
            return;
        }

        let metrics = Metrics::new(label_text_size, label_text_size);
        self.outlines.clear();
        for buffer in &mut self.buffers {
            self.text_cache.resize(buffer, metrics);
            self.outlines.push(self.text_cache.outline(buffer));
        }
        self.outline_size = label_text_size;
    }

    /// Computes the geometry of the graph.
    pub(crate) fn geometry(&self, viewer: &Viewer, draw_actions: bool, state_radius: f32) -> FrameGeometry {
        let mut geometry = FrameGeometry::default();
        let circle = |path: &mut BezPath, x: f32, y: f32, r: f32| {
            path.extend(Circle::new((x as f64, y as f64), r as f64).path_elements(0.1));
        };
        let point = |v: Vec2| Point::new(v.x as f64, v.y as f64);

        for state_index in self.lts.iter_states() {
            let state_view = &viewer.state_view()[state_index];

            for (transition_index, transition) in self.lts.outgoing_transitions(state_index).enumerate() {
                let to_state_view = &viewer.state_view()[transition.to];
                let transition_view = &state_view.outgoing[transition_index];

                let label_position = if transition.to != state_index {
                    geometry.edges.move_to(point(state_view.position.xy()));
                    geometry.edges.line_to(point(to_state_view.position.xy()));

                    // The arrow head is just outside the target state, pointing along the edge.
                    let direction = (state_view.position - to_state_view.position).normalize();
                    let angle = -direction.xy().angle_to(Vec2::new(0.0, -1.0));
                    let transform =
                        Affine::translate(point(to_state_view.position.xy()).to_vec2()) * Affine::rotate(angle as f64);
                    let r = state_radius as f64;
                    geometry.arrows.move_to(transform * Point::new(0.0, -r - 0.5));
                    geometry.arrows.line_to(transform * Point::new(2.0, -r - 5.5));
                    geometry.arrows.line_to(transform * Point::new(-2.0, -r - 5.5));
                    geometry.arrows.close_path();

                    // The edge handle
                    let middle = (to_state_view.position + state_view.position) / 2.0;
                    circle(
                        &mut geometry.edges,
                        middle.x + transition_view.handle_offset.x,
                        middle.y + transition_view.handle_offset.y,
                        1.0,
                    );

                    middle
                } else {
                    // A self loop
                    let middle = (2.0 * state_view.position + transition_view.handle_offset) / 2.0;
                    circle(
                        &mut geometry.edges,
                        middle.x,
                        middle.y,
                        transition_view.handle_offset.length() / 2.0,
                    );
                    circle(
                        &mut geometry.edges,
                        state_view.position.x + transition_view.handle_offset.x,
                        state_view.position.y + transition_view.handle_offset.y,
                        1.0,
                    );
                    state_view.position + transition_view.handle_offset
                };

                if draw_actions {
                    geometry.labels.push((*transition.label, label_position.xy()));
                }
            }
        }

        for (index, state_view) in viewer.state_view().iter().enumerate() {
            let target = if index != *self.lts.initial_state_index() {
                &mut geometry.states
            } else {
                &mut geometry.initial_state
            };
            circle(target, state_view.position.x, state_view.position.y, state_radius);
        }

        geometry
    }
}

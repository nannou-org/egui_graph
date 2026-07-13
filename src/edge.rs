use crate::{bezier, EdgesCtx, NodeId};
use std::ops;

/// A simple bezier-curve Edge widget.
///
/// Handles interaction (selection, deselection, deletion) and painting the
/// bezier curve.
///
/// By default, the stroke for each state is adopted from the egui visuals:
///
/// - Selected: `ui.visuals().selection.stroke`.
/// - Hovered: `ui.visuals().widgets.hovered.fg_stroke`.
/// - Otherwise: `ui.visuals().widgets.noninteractive.fg_stroke`.
///
/// Each of these may be overridden per-edge via [`Edge::selected_stroke`],
/// [`Edge::hovered_stroke`] and [`Edge::stroke`].
pub struct Edge<'a> {
    edge: ((NodeId, OutputIx), (NodeId, InputIx)),
    waypoints: &'a [egui::Pos2],
    distance_per_point: f32,
    curvature: f32,
    stroke: Option<egui::Stroke>,
    hovered_stroke: Option<egui::Stroke>,
    selected_stroke: Option<egui::Stroke>,
    selected: &'a mut bool,
}

/// A response returned from the [`Edge`] widget.
///
/// Similar to [`egui::Response`], however as there's no clear rectangular space
/// allocated to the edge, we use a more minimal custom response.
pub struct EdgeResponse {
    response: egui::Response,
    changed: bool,
    deleted: bool,
    closest_point: egui::Pos2,
}

/// The resolved inputs for painting an edge, handed to the closure given to
/// [`Edge::show_with`].
///
/// All coordinates share the layer-local space of the edge's sockets.
#[non_exhaustive]
pub struct EdgePaintCtx<'a> {
    /// The edge's piecewise-cubic bezier path.
    pub path: &'a bezier::Path,
    /// The path flattened at the edge's distance-per-point, ready for
    /// [`egui::Shape::line`].
    pub points: &'a [egui::Pos2],
    /// Whether the edge is currently selected.
    pub selected: bool,
    /// Whether the edge is hover-highlighted, including the highlight shown
    /// while a pending selection rectangle covers the edge.
    pub hovered: bool,
    /// The stroke default painting would use for the current state, with the
    /// per-edge stroke overrides applied and falling back to the egui visuals.
    pub stroke: egui::Stroke,
}

/// An index of a node's input or output socket.
pub type SocketIx = usize;
/// An index of a node's input socket.
pub type InputIx = SocketIx;
/// An index of a node's output socket.
pub type OutputIx = SocketIx;

impl<'a> Edge<'a> {
    pub const DEFAULT_DISTANCE_PER_POINT: f32 = 5.0;

    /// An edge from node `a`'s output socket to node `b`'s input socket.
    pub fn new(a: (NodeId, OutputIx), b: (NodeId, InputIx), selected: &'a mut bool) -> Self {
        Self {
            edge: (a, b),
            waypoints: &[],
            distance_per_point: Self::DEFAULT_DISTANCE_PER_POINT,
            curvature: bezier::Cubic::DEFAULT_CURVATURE,
            stroke: None,
            hovered_stroke: None,
            selected_stroke: None,
            selected,
        }
    }

    /// Thread the edge's curve through the given intermediate waypoints,
    /// ordered from the output socket toward the input socket.
    ///
    /// Use this with the corridor routes produced by the automatic layout
    /// (`EdgeRoutes`, from `layout_routed`) to keep long edges from passing
    /// over unrelated nodes. Waypoints share the node layout's coordinate
    /// space.
    ///
    /// Routes are only meaningful while node positions match the layout that
    /// produced them - when nodes are arranged freely instead, omit the
    /// waypoints so edges fall back to direct curves.
    ///
    /// Default: none (a single curve directly between the sockets).
    pub fn waypoints(mut self, waypoints: &'a [egui::Pos2]) -> Self {
        self.waypoints = waypoints;
        self
    }

    /// The distance-per-point used to render the bezier curve path.
    ///
    /// This path is also used to check for selection interaction.
    ///
    /// The smaller the distance, the higher-quality rendering and interactions
    /// will be, at the cost of performance.
    ///
    /// Default: `Self::DEFAULT_DISTANCE_PER_POINT`
    pub fn distance_per_point(mut self, dist: f32) -> Self {
        self.distance_per_point = dist;
        self
    }

    /// Set the normalized curvature used when constructing the edge bezier.
    ///
    /// Values are clamped to `0.0..=1.0` and then scaled internally so the
    /// strongest curve uses at most half the socket-to-socket distance for its
    /// control points.
    ///
    /// Default: [`bezier::Cubic::DEFAULT_CURVATURE`].
    pub fn curvature_factor(mut self, curvature: f32) -> Self {
        self.curvature = curvature;
        self
    }

    /// Override the stroke used when the edge is in its default (unselected,
    /// unhovered) state.
    ///
    /// Default: `ui.visuals().widgets.noninteractive.fg_stroke`.
    pub fn stroke(mut self, stroke: egui::Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Override the stroke used when the edge is hovered.
    ///
    /// Default: `ui.visuals().widgets.hovered.fg_stroke`.
    pub fn hovered_stroke(mut self, stroke: egui::Stroke) -> Self {
        self.hovered_stroke = Some(stroke);
        self
    }

    /// Override the stroke used when the edge is selected.
    ///
    /// Default: `ui.visuals().selection.stroke`.
    pub fn selected_stroke(mut self, stroke: egui::Stroke) -> Self {
        self.selected_stroke = Some(stroke);
        self
    }

    /// Process any user interaction with the edge and present it.
    pub fn show(self, ectx: &mut EdgesCtx, ui: &mut egui::Ui) -> EdgeResponse {
        self.show_with(ectx, ui, |ui, cx| {
            ui.painter()
                .add(egui::Shape::line(cx.points.to_vec(), cx.stroke));
        })
    }

    /// As [`Edge::show`], but painting the edge via `paint` instead of the
    /// default solid line.
    ///
    /// Interaction (hover, selection, deletion) is identical to [`Edge::show`].
    /// The `paint` closure receives the resolved paint inputs - see
    /// [`EdgePaintCtx`]. It is not called when either socket position is
    /// unavailable, in which case there is nothing to paint.
    pub fn show_with(
        self,
        ectx: &mut EdgesCtx,
        ui: &mut egui::Ui,
        paint: impl FnOnce(&mut egui::Ui, EdgePaintCtx),
    ) -> EdgeResponse {
        let Self {
            edge: ((a, output), (b, input)),
            waypoints,
            distance_per_point,
            curvature,
            stroke,
            hovered_stroke,
            selected_stroke,
            selected,
        } = self;

        // Retrieve the location and direction of the node sockets.
        // If either socket position is unavailable (e.g. sparse explicit
        // layout), skip rendering entirely.
        let (a_out, b_in) = match (ectx.output(ui, a, output), ectx.input(ui, b, input)) {
            (Some(a_out), Some(b_in)) => (a_out, b_in),
            _ => {
                let edge_id = ui.id().with(("edge", a, output, b, input));
                let response = ui.interact(egui::Rect::NOTHING, edge_id, egui::Sense::click());
                return EdgeResponse {
                    response,
                    changed: false,
                    deleted: false,
                    closest_point: egui::Pos2::ZERO,
                };
            }
        };

        // TODO: Cache the curve and its points?
        let path = bezier::Path::from_edge_points_via(a_out, waypoints, b_in, curvature);

        // Get the mouse position for computing the closest point on the edge.
        let ui_response = ui.response();
        let mouse_pos = ui_response
            .interact_pointer_pos()
            .or(ui_response.hover_pos())
            .unwrap_or_default();
        let closest_point = path.closest_point(distance_per_point, mouse_pos);

        // Create a per-edge response for interaction and context menu support.
        // The interact area follows the mouse along the edge curve.
        let select_dist = ui.style().interaction.interact_radius;
        let edge_id = ui.id().with(("edge", a, output, b, input));
        let interact_rect = egui::Rect::from_center_size(
            closest_point,
            egui::vec2(select_dist * 2.0, select_dist * 2.0),
        );
        let response = ui.interact(interact_rect, edge_id, egui::Sense::click());

        // Determine if edge interactions should be processed.
        // Disable when drawing a new edge or when close to a socket.
        let edge_in_progress = ectx.in_progress(ui).is_some();
        let can_interact = !edge_in_progress && ectx.closest_socket.is_none();
        let clicked = can_interact && response.clicked();

        // Check if the edge intersects the selection rectangle.
        let under_selection_rect = ectx
            .selection_rect
            .map(|rect| path.intersects_rect(distance_per_point, rect))
            .unwrap_or(false);

        // Handle selection state changes.
        let old_selected = *selected;
        if *selected {
            // Deselect if: edge drawing started, ctrl+click, or click elsewhere without ctrl.
            if edge_in_progress
                || (clicked && ui.input(|i| i.modifiers.ctrl))
                || ui.input(|i| i.pointer.primary_pressed() && !i.modifiers.ctrl)
            {
                *selected = false;
            }
        } else if clicked
            || (under_selection_rect
                && ui.input(|i| i.modifiers.shift && i.pointer.primary_released()))
        {
            *selected = true;
        }

        // Check if the edge was deleted (skip when immutable).
        let mut deleted = false;
        // FIXME: We may only want to do this if `ui.id()` has focus
        // (Memory::has_focus) or similar, but we still need to setup proper
        // focus-requesting and consider how to handle nodes too.
        if !ectx.immutable && *selected && !ui.ctx().egui_wants_keyboard_input() {
            let del_keys = [egui::Key::Delete, egui::Key::Backspace];
            if ui.input(|i| del_keys.iter().any(|&k| i.key_pressed(k))) {
                deleted = true;
            }
        }

        // Determine hover styling (additional conditions beyond response.hovered()).
        let show_hover = can_interact
            && response.hovered()
            && ui.input(|i| !i.pointer.primary_down() || i.pointer.could_any_button_be_click());

        // Paint the edge.
        let pts: Vec<_> = path.flatten(distance_per_point).collect();
        let hovered = show_hover || (under_selection_rect && ui.input(|i| i.modifiers.shift));
        let stroke = if *selected {
            selected_stroke.unwrap_or(ui.style().visuals.selection.stroke)
        } else if hovered {
            hovered_stroke.unwrap_or(ui.style().visuals.widgets.hovered.fg_stroke)
        } else {
            stroke.unwrap_or(ui.style().visuals.widgets.noninteractive.fg_stroke)
        };
        paint(
            ui,
            EdgePaintCtx {
                path: &path,
                points: &pts,
                selected: *selected,
                hovered,
                stroke,
            },
        );

        // Construct and return the response.
        let changed = old_selected != *selected;
        EdgeResponse {
            response,
            changed,
            deleted,
            closest_point,
        }
    }
}

impl EdgeResponse {
    /// Whether or not the edge selected state changed.
    pub fn changed(&self) -> bool {
        self.changed
    }

    /// The edge was selected while `Delete` or `Backspace` were pressed.
    pub fn deleted(&self) -> bool {
        self.deleted
    }

    /// The position on the edge closest to the pointer.
    pub fn closest_point(&self) -> egui::Pos2 {
        self.closest_point
    }
}

impl ops::Deref for EdgeResponse {
    type Target = egui::Response;
    fn deref(&self) -> &Self::Target {
        &self.response
    }
}

impl From<EdgeResponse> for egui::Response {
    fn from(response: EdgeResponse) -> Self {
        response.response
    }
}

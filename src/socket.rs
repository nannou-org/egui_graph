pub mod layout;

use crate::node::NodeId;

/// Describes either an input or output.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SocketKind {
    Input,
    Output,
}

/// Uniquely identifies a socket.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Socket {
    /// The node that owns this socket.
    pub node: NodeId,
    /// Whether the socket is an input or output.
    pub kind: SocketKind,
    /// The index of the socket of this kind.
    pub index: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct PositionedSocket {
    pub socket: Socket,
    /// Screen-space position of the socket.
    pub pos: egui::Pos2,
    /// The normal of the edge along which this socket resides.
    pub normal: egui::Vec2,
}

/// Collected [`egui::Response`]s for all sockets on a node.
///
/// Each socket is allocated as an interactive widget (with [`egui::Sense::hover`]),
/// enabling standard egui interactions like tooltips and hover detection.
pub struct SocketResponses {
    inputs: std::collections::BTreeMap<usize, egui::Response>,
    outputs: std::collections::BTreeMap<usize, egui::Response>,
}

impl SocketResponses {
    /// The response for the input socket at the given index.
    pub fn input(&self, ix: usize) -> Option<&egui::Response> {
        self.inputs.get(&ix)
    }

    /// The response for the output socket at the given index.
    pub fn output(&self, ix: usize) -> Option<&egui::Response> {
        self.outputs.get(&ix)
    }

    /// Iterator over all input socket responses, yielding `(index, response)`.
    pub fn inputs(&self) -> impl Iterator<Item = (usize, &egui::Response)> {
        self.inputs.iter().map(|(&ix, r)| (ix, r))
    }

    /// Iterator over all output socket responses, yielding `(index, response)`.
    pub fn outputs(&self) -> impl Iterator<Item = (usize, &egui::Response)> {
        self.outputs.iter().map(|(&ix, r)| (ix, r))
    }
}

/// The padding from the ends of a node's edge within which its sockets are
/// laid out, as used by [`Node`](crate::node::Node).
///
/// Useful for deriving socket positions outside of node instantiation, e.g.
/// when providing socket offsets to the automatic layout.
pub fn socket_padding(style: &egui::Style) -> f32 {
    let min_interact_len = style
        .spacing
        .interact_size
        .x
        .min(style.spacing.interact_size.y);
    style.visuals.window_corner_radius.ne as f32 + min_interact_len * 0.5
}

/// Adaptive segment count for a semicircle, matching egui's circle tessellation
/// heuristic (halved, since a semicircle spans half the arc).
fn semicircle_segments(radius: f32) -> usize {
    if radius <= 2.0 {
        4
    } else if radius <= 5.0 {
        8
    } else if radius < 18.0 {
        16
    } else if radius < 50.0 {
        32
    } else {
        64
    }
}

/// Paint a filled semicircle facing outward along `normal`.
///
/// The flat edge is perpendicular to `normal` and passes through `center`.
/// The curved part extends outward from `center` by `radius` along `normal`.
fn paint_semicircle(
    painter: &egui::Painter,
    center: egui::Pos2,
    radius: f32,
    normal: egui::Vec2,
    color: egui::Color32,
) {
    let segments = semicircle_segments(radius);
    let perp = egui::Vec2::new(-normal.y, normal.x);
    let mut pts = Vec::with_capacity(segments + 1);
    for i in 0..=segments {
        let angle = std::f32::consts::PI * (i as f32) / (segments as f32);
        let (sin, cos) = angle.sin_cos();
        // Trace from +perp through +normal to -perp.
        pts.push(center + perp * (radius * cos) + normal * (radius * sin));
    }
    painter.add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// Paint and interact with all sockets for a node.
///
/// Phase A: extracts highlight state (pressed/closest socket) from graph memory, then drops the lock.
/// Phase B: creates a socket sublayer, paints each socket as an outward-facing semicircle
/// (using `Shape::convex_polygon`), and calls `ui.interact()` to produce per-socket responses.
///
/// By rendering semicircles that only extend outward from the frame border, sockets avoid
/// visual overlap with node frames regardless of sublayer ordering.
#[allow(clippy::too_many_arguments)]
pub(crate) fn show(
    ui: &mut egui::Ui,
    graph_id: egui::Id,
    node_id: NodeId,
    egui_id: egui::Id,
    socket_layer: egui::LayerId,
    frame_rect: egui::Rect,
    node_sockets: &crate::NodeSockets,
    socket_color: egui::Color32,
    socket_radius: f32,
) -> SocketResponses {
    // Phase A: Store resolved sockets and extract highlight state, then drop the lock.
    let (pressed_socket, closest_socket) = if !node_sockets.inputs.is_empty()
        || !node_sockets.outputs.is_empty()
    {
        let gmem_arc = crate::memory(ui, graph_id);
        let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
        gmem.sockets.insert(node_id, node_sockets.clone());

        let pressed_socket = gmem
            .pressed
            .as_ref()
            .and_then(|pressed| match pressed.action {
                crate::PressAction::Socket(socket) if socket.node == node_id => {
                    Some((socket.kind, socket.index))
                }
                _ => None,
            });

        let closest_socket = match gmem.closest_socket {
            Some(closest) if closest.node == node_id => {
                match gmem.pressed.as_ref().map(|p| &p.action) {
                    Some(crate::PressAction::Socket(socket)) if closest.kind == socket.kind => None,
                    _ => Some((closest.kind, closest.index)),
                }
            }
            _ => None,
        };

        (pressed_socket, closest_socket)
    } else {
        (None, None)
    };

    let paint_highlight =
        |kind, ix| pressed_socket == Some((kind, ix)) || closest_socket == Some((kind, ix));
    paint(
        ui,
        egui_id,
        socket_layer,
        frame_rect,
        node_sockets,
        socket_color,
        socket_radius,
        paint_highlight,
    )
}

/// Paint each socket on `socket_layer` and allocate its hover response.
///
/// `highlight` names the sockets that get the larger, faded semicircle
/// behind them, such as a pressed socket or the closest one to the pointer
/// while an edge is in progress. This is the graph-free half of [`show`].
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint(
    ui: &mut egui::Ui,
    egui_id: egui::Id,
    socket_layer: egui::LayerId,
    frame_rect: egui::Rect,
    node_sockets: &crate::NodeSockets,
    socket_color: egui::Color32,
    socket_radius: f32,
    highlight: impl Fn(SocketKind, usize) -> bool,
) -> SocketResponses {
    let hl_size = (socket_radius + 4.0).max(4.0);
    let interact_diameter = ui
        .spacing()
        .interact_size
        .x
        .min(ui.spacing().interact_size.y);

    let builder = egui::UiBuilder::new()
        .max_rect(frame_rect.expand(hl_size))
        .layer_id(socket_layer);

    let mut input_responses = std::collections::BTreeMap::new();
    let mut output_responses = std::collections::BTreeMap::new();

    ui.scope_builder(builder, |ui| {
        let painter = ui.painter();
        for (ix, pos, normal) in node_sockets.inputs() {
            if highlight(SocketKind::Input, ix) {
                paint_semicircle(
                    painter,
                    pos,
                    hl_size,
                    normal,
                    socket_color.linear_multiply(0.25),
                );
            }
            paint_semicircle(painter, pos, socket_radius, normal, socket_color);
            let id = egui_id.with("in").with(ix);
            let rect = egui::Rect::from_center_size(pos, egui::Vec2::splat(interact_diameter));
            input_responses.insert(ix, ui.interact(rect, id, egui::Sense::hover()));
        }
        for (ix, pos, normal) in node_sockets.outputs() {
            if highlight(SocketKind::Output, ix) {
                paint_semicircle(
                    painter,
                    pos,
                    hl_size,
                    normal,
                    socket_color.linear_multiply(0.25),
                );
            }
            paint_semicircle(painter, pos, socket_radius, normal, socket_color);
            let id = egui_id.with("out").with(ix);
            let rect = egui::Rect::from_center_size(pos, egui::Vec2::splat(interact_diameter));
            output_responses.insert(ix, ui.interact(rect, id, egui::Sense::hover()));
        }
    });

    SocketResponses {
        inputs: input_responses,
        outputs: output_responses,
    }
}

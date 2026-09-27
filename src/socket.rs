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

#[cfg(test)]
mod tests {
    use super::{Socket, SocketKind};
    use crate::node::{EdgeEvent, Node, NodeId};
    use crate::{Graph, View};

    /// A graph of nodes `A` and `B`, driven one pass at a time.
    struct TestGraph {
        ctx: egui::Context,
        view: View,
        immutable: bool,
    }

    /// The graph state observed at the end of a pass.
    struct Pass {
        /// The screen position and normal of the output of `A`.
        a_out: (egui::Pos2, egui::Vec2),
        /// The screen position and normal of the input of `B`.
        b_in: (egui::Pos2, egui::Vec2),
        closest_socket: Option<Socket>,
        b_edge_event: Option<EdgeEvent>,
    }

    const GRAPH: &str = "graph";
    const SCREEN: egui::Rect = egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(800.0, 600.0));
    /// Has one output on its right edge.
    const A: NodeId = NodeId(0);
    /// Has one input on its left edge, to the right of `A`.
    const B: NodeId = NodeId(1);
    const B_IN: Socket = Socket {
        node: B,
        kind: SocketKind::Input,
        index: 0,
    };
    /// The depth of an on-frame point inside the frame, within socket detection range.
    const INSET: f32 = 4.0;

    /// Lay out a new graph over two passes, since sizes settle after the first.
    fn test_graph(immutable: bool) -> (TestGraph, Pass) {
        let view = View {
            scene_rect: SCREEN,
            layout: [(A, egui::pos2(100.0, 100.0)), (B, egui::pos2(400.0, 100.0))].into(),
        };
        let mut g = TestGraph {
            ctx: egui::Context::default(),
            view,
            immutable,
        };
        pass(&mut g, vec![]);
        let p = pass(&mut g, vec![]);
        (g, p)
    }

    /// Run one pass of the graph with the given input events.
    fn pass(g: &mut TestGraph, events: Vec<egui::Event>) -> Pass {
        let input = egui::RawInput {
            screen_rect: Some(SCREEN),
            events,
            ..Default::default()
        };
        let mut out = None;
        let _ = g.ctx.run_ui(input, |ui| {
            let graph = Graph::new(GRAPH).immutable(g.immutable);
            graph.show(&mut g.view, ui, |ui, show| {
                let mut b_edge_event = None;
                show.nodes(ui, |nctx, ui| {
                    for (id, inputs, outputs) in [(A, 0, 1), (B, 1, 0)] {
                        let node = Node::from_id(id).inputs(inputs).outputs(outputs);
                        let res = node.show(nctx, ui, |c| c.framed(|ui, _| ui.label("x")));
                        if id == B {
                            b_edge_event = res.edge_event();
                        }
                    }
                });
                let to_screen = ui
                    .ctx()
                    .layer_transform_to_global(ui.layer_id())
                    .unwrap_or_default();
                let screen = |(pos, normal)| (to_screen.mul_pos(pos), normal);
                let gmem_arc = crate::memory(ui, crate::id(GRAPH));
                let gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
                out = Some(Pass {
                    a_out: screen(gmem.sockets[&A].output(0).expect("A has an output")),
                    b_in: screen(gmem.sockets[&B].input(0).expect("B has an input")),
                    closest_socket: gmem.closest_socket,
                    b_edge_event,
                });
            });
        });
        out.expect("the graph ran")
    }

    /// Move the pointer to `pos`, then run a second pass, since pointer
    /// coverage of node frames takes effect on the next pass.
    fn hover(g: &mut TestGraph, pos: egui::Pos2) -> Pass {
        pass(g, vec![egui::Event::PointerMoved(pos)]);
        pass(g, vec![])
    }

    /// A press or release of the primary button at `pos`.
    fn primary(pos: egui::Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    /// A point outside the node frame, within socket detection range. egui
    /// gives the frame any press within the interact radius of its edge, so
    /// the point lies beyond that radius.
    fn off_frame((pos, normal): (egui::Pos2, egui::Vec2)) -> egui::Pos2 {
        let style = egui::Style::default();
        let detect_radius = style.spacing.interact_size.min_elem();
        pos + normal * (style.interaction.interact_radius + detect_radius) * 0.5
    }

    /// A point just inside the node frame, near the socket.
    fn on_frame((pos, normal): (egui::Pos2, egui::Vec2)) -> egui::Pos2 {
        pos - normal * INSET
    }

    /// Off the frame, a press starts an edge, so the socket is detected.
    #[test]
    fn hover_off_frame_detects_socket() {
        let (mut g, p) = test_graph(false);
        let p = hover(&mut g, off_frame(p.b_in));
        assert_eq!(p.closest_socket, Some(B_IN));
    }

    /// On the frame, a press drags the node, so the socket is not detected.
    #[test]
    fn hover_on_frame_skips_socket() {
        let (mut g, p) = test_graph(false);
        let p = hover(&mut g, on_frame(p.b_in));
        assert_eq!(p.closest_socket, None);
    }

    /// An immutable graph cannot start an edge, so no socket is detected.
    #[test]
    fn immutable_hover_skips_socket() {
        let (mut g, p) = test_graph(true);
        let p = hover(&mut g, off_frame(p.b_in));
        assert_eq!(p.closest_socket, None);
    }

    /// While an edge is in progress, a socket is detected from inside its
    /// node frame, and a release there ends the edge on it.
    #[test]
    fn edge_drag_detects_socket_on_frame() {
        let (mut g, p) = test_graph(false);
        let start = off_frame(p.a_out);
        let end = on_frame(p.b_in);
        hover(&mut g, start);
        pass(&mut g, vec![primary(start, true)]);
        let p = hover(&mut g, end);
        assert_eq!(p.closest_socket, Some(B_IN));
        let p = pass(&mut g, vec![primary(end, false)]);
        assert_eq!(
            p.b_edge_event,
            Some(EdgeEvent::Ended {
                kind: SocketKind::Input,
                index: 0,
            })
        );
    }
}

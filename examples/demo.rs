use eframe::egui;
use egui_graph::node::EdgeEvent;
use egui_graph::SocketKind;
use petgraph::graph::{EdgeIndex, NodeIndex};
use petgraph::visit::EdgeRef;
use std::collections::{HashMap, HashSet};

fn main() -> Result<(), eframe::Error> {
    env_logger::init(); // Log to stderr (if you run with `RUST_LOG=debug`).
    let mut options = eframe::NativeOptions::default();
    // The default `AutoVsync` (Fifo) present mode can present unevenly on some
    // platforms (notably Wayland). `AutoNoVsync` prefers `Mailbox` for
    // low-latency, tear-free presentation and falls back to `Immediate` where
    // Mailbox is unavailable, so it stays robust across surfaces.
    options.wgpu_options.present_mode = eframe::wgpu::PresentMode::AutoNoVsync;
    let name = "`egui_graph` demo";
    eframe::run_native(name, options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}

struct App {
    state: State,
    view: egui_graph::View,
}

/// How the demo snaps node positions.
#[derive(Clone, Copy, PartialEq)]
enum SnapMode {
    /// No snapping.
    Off,
    /// Snap to the nearest whole graph-space unit (the default).
    Point,
    /// Snap to the grid step, aligning nodes with the dot grid.
    Grid,
}

struct State {
    graph: Graph,
    interaction: Interaction,
    flow: egui::Direction,
    socket_radius: f32,
    socket_color: egui::Color32,
    custom_edge_style: bool,
    edge_width: f32,
    edge_color: egui::Color32,
    edge_curvature: f32,
    #[cfg(feature = "layout")]
    auto_layout: bool,
    /// Whether the layout accounts for the socket each edge connects to;
    /// when off it behaves like a classic node-size-only layered layout.
    #[cfg(feature = "layout")]
    socket_aware: bool,
    /// Whether edges route around nodes (via the auto-layout's corridors, or
    /// best-effort against the current positions in freehand mode).
    #[cfg(feature = "layout")]
    route_edges: bool,
    #[cfg(feature = "layout")]
    layer_gap: f32,
    #[cfg(feature = "layout")]
    node_gap: f32,
    /// Corridor waypoints for edges, kept in sync with node positions.
    #[cfg(feature = "layout")]
    routes: egui_graph::EdgeRoutes,
    node_id_map: HashMap<egui_graph::NodeId, NodeIndex>,
    center_view: bool,
    auto_pan: bool,
    dot_grid: bool,
    immutable: bool,
    snap_mode: SnapMode,
    grid_step: f32,
    align: bool,
    align_edges: bool,
    align_centers: bool,
    align_threshold: f32,
    align_guides: bool,
    /// Which node feature the "Align Selection" button unifies.
    align_by: egui_graph::AlignBy,
}

#[derive(Default)]
struct Interaction {
    selection: Selection,
    edge_in_progress: Option<(NodeIndex, SocketKind, usize)>,
}

#[derive(Default)]
struct Selection {
    nodes: HashSet<NodeIndex>,
    edges: HashSet<EdgeIndex>,
}

type Graph = petgraph::stable_graph::StableGraph<Node, (usize, usize)>;

struct Node {
    name: String,
    kind: NodeKind,
    /// Per-node flow override; `None` follows the graph-wide `State::flow`.
    /// Set via the "Selected flow" buttons to showcase mixed-flow layout.
    flow: Option<egui::Direction>,
}

enum NodeKind {
    Label,
    Button,
    Slider(f32),
    DragValue(f32),
    Comment(String),
    /// A mixer-style node with content-aligned sockets.
    Mixer {
        color: f32,
        alpha: f32,
    },
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        ctx.set_fonts(egui::FontDefinitions::default());
        let weak_text_color = ctx.global_style().visuals.weak_text_color();
        let graph = new_graph();
        let state = State {
            graph,
            interaction: Default::default(),
            socket_color: weak_text_color,
            socket_radius: 3.0,
            custom_edge_style: false,
            edge_width: 1.0,
            edge_color: weak_text_color,
            edge_curvature: 0.5,
            flow: egui::Direction::TopDown,
            #[cfg(feature = "layout")]
            auto_layout: true,
            #[cfg(feature = "layout")]
            socket_aware: true,
            #[cfg(feature = "layout")]
            route_edges: false,
            #[cfg(feature = "layout")]
            layer_gap: egui_graph::LayoutParams::DEFAULT_LAYER_GAP,
            #[cfg(feature = "layout")]
            node_gap: egui_graph::LayoutParams::DEFAULT_NODE_GAP,
            #[cfg(feature = "layout")]
            routes: Default::default(),
            node_id_map: Default::default(),
            center_view: false,
            auto_pan: true,
            dot_grid: true,
            immutable: false,
            snap_mode: SnapMode::Point,
            // Matches the dot grid's previous default (style interact_size.y).
            grid_step: 18.0,
            align: true,
            align_edges: true,
            align_centers: false,
            align_threshold: 5.0,
            align_guides: true,
            align_by: egui_graph::AlignBy::Min,
        };
        let view = Default::default();
        App { view, state }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        #[cfg(feature = "layout")]
        if self.state.auto_layout {
            (self.view.layout, self.state.routes) = layout(&self.state, ui.ctx());
        } else if self.state.route_edges {
            // Freehand mode: best-effort routes against the current
            // positions, recomputed as nodes move.
            self.state.routes = freehand_routes(
                &self.state.graph,
                &self.view.layout,
                self.state.flow,
                self.state.node_gap,
                ui.ctx(),
            );
        }
        gui(ui, &mut self.view, &mut self.state);
    }
}

fn new_graph() -> Graph {
    // The graph we want to inspect/edit.
    let mut graph = Graph::new();
    let a = graph.add_node(node("Foo", NodeKind::Label));
    let b = graph.add_node(node("Bar", NodeKind::Button));
    let c = graph.add_node(node("Baz", NodeKind::Slider(0.5)));
    let d = graph.add_node(node("Qux", NodeKind::DragValue(20.0)));
    let comment = "Nodes are a thin wrapper around the `egui::Window`, \
        allowing you to set arbitrary widgets.";
    let e = graph.add_node(node("Fiz", NodeKind::Comment(comment.to_string())));
    let f = graph.add_node(node(
        "Mix",
        NodeKind::Mixer {
            color: 0.5,
            alpha: 1.0,
        },
    ));
    graph.add_edge(a, c, (0, 0));
    graph.add_edge(a, d, (1, 1));
    graph.add_edge(b, d, (0, 2));
    graph.add_edge(c, d, (0, 0));
    graph.add_edge(d, e, (0, 0));
    graph.add_edge(d, f, (0, 0));
    graph
}

fn node(name: impl ToString, kind: NodeKind) -> Node {
    let name = name.to_string();
    Node {
        name,
        kind,
        flow: None,
    }
}

/// A node's effective flow: its override, else the graph-wide default.
fn node_flow(node: &Node, default: egui::Direction) -> egui::Direction {
    node.flow.unwrap_or(default)
}

/// Set the flow override on every currently-selected node.
fn set_selected_flow(state: &mut State, flow: Option<egui::Direction>) {
    for &n in &state.interaction.selection.nodes {
        if let Some(node) = state.graph.node_weight_mut(n) {
            node.flow = flow;
        }
    }
}

/// Get the egui::Id for the demo graph widget.
fn graph_id() -> egui::Id {
    egui_graph::id("Demo Graph")
}

/// The number of input/output sockets for a node: one per socket index, up
/// to the max index used by its edges.
fn socket_counts(graph: &Graph, n: NodeIndex) -> (usize, usize) {
    let inputs = graph
        .edges_directed(n, petgraph::Incoming)
        .fold(0, |max, e| std::cmp::max(max, e.weight().1 + 1));
    let outputs = graph
        .edges_directed(n, petgraph::Outgoing)
        .fold(0, |max, e| std::cmp::max(max, e.weight().0 + 1));
    (inputs, outputs)
}

/// Each node's size (from graph memory), socket counts and flow, as layout
/// input.
///
/// Note: this feeds socket *counts*, not the exact rendered socket positions,
/// so the layout's socket anchors are evenly spaced approximations; cross-flow
/// edges between clusters therefore connect best-effort.
#[cfg(feature = "layout")]
fn layout_nodes(
    graph: &Graph,
    ctx: &egui::Context,
    default_flow: egui::Direction,
) -> Vec<(egui_graph::NodeId, egui_graph::LayoutNode)> {
    let socket_padding = egui_graph::socket_padding(&ctx.global_style());
    // Access graph memory once and iterate inside to avoid repeated locks
    egui_graph::with_graph_memory(ctx, graph_id(), |gmem| {
        let node_sizes = gmem.node_sizes();
        graph
            .node_indices()
            .map(|n| {
                let node_id = egui_graph::NodeId::from_u64(n.index() as u64);
                let size = node_sizes
                    .get(&node_id)
                    .cloned()
                    .unwrap_or_else(|| [200.0, 50.0].into());
                let (inputs, outputs) = socket_counts(graph, n);
                let node = egui_graph::LayoutNode::new(size)
                    .socket_padding(socket_padding)
                    .inputs(inputs)
                    .outputs(outputs)
                    .flow(node_flow(&graph[n], default_flow));
                (node_id, node)
            })
            .collect()
    })
}

/// The graph's edges as socket-indexed layout input.
#[cfg(feature = "layout")]
fn socket_edges(
    graph: &Graph,
) -> impl Iterator<Item = ((egui_graph::NodeId, usize), (egui_graph::NodeId, usize))> + '_ {
    graph.edge_indices().filter_map(|e| {
        let (a, b) = graph.edge_endpoints(e)?;
        let &(output, input) = graph.edge_weight(e)?;
        let a = egui_graph::NodeId::from_u64(a.index() as u64);
        let b = egui_graph::NodeId::from_u64(b.index() as u64);
        Some(((a, output), (b, input)))
    })
}

#[cfg(feature = "layout")]
fn layout(state: &State, ctx: &egui::Context) -> (egui_graph::Layout, egui_graph::EdgeRoutes) {
    let params = egui_graph::LayoutParams::new(state.flow)
        .layer_gap(state.layer_gap)
        .node_gap(state.node_gap)
        .socket_aware(state.socket_aware);
    egui_graph::layout_routed(
        layout_nodes(&state.graph, ctx, state.flow),
        socket_edges(&state.graph),
        params,
    )
}

/// Best-effort routes against the nodes' current freehand positions.
#[cfg(feature = "layout")]
fn freehand_routes(
    graph: &Graph,
    layout: &egui_graph::Layout,
    flow: egui::Direction,
    node_gap: f32,
    ctx: &egui::Context,
) -> egui_graph::EdgeRoutes {
    // Freehand routing assumes one flow; with mixed per-node flows the routes
    // are best-effort (see `route_edges`' single-flow limitation).
    let nodes = layout_nodes(graph, ctx, flow)
        .into_iter()
        .filter_map(|(id, node)| Some((id, *layout.get(&id)?, node)));
    let params = egui_graph::LayoutParams::new(flow).node_gap(node_gap);
    egui_graph::route_edges(nodes, socket_edges(graph), params)
}

fn gui(ui: &mut egui::Ui, view: &mut egui_graph::View, state: &mut State) {
    egui::containers::CentralPanel::default()
        .frame(egui::Frame::default())
        .show_inside(ui, |ui| {
            graph_config(ui, view, state);
            graph(ui, view, state);
        });
}

fn graph(ui: &mut egui::Ui, view: &mut egui_graph::View, state: &mut State) {
    let graph_response = egui_graph::Graph::from_id(graph_id())
        .center_view(state.center_view)
        .auto_pan(state.auto_pan.then_some(egui_graph::AutoPan::DEFAULT))
        .dot_grid(state.dot_grid)
        .immutable(state.immutable)
        // Snap and the dot grid are independent options. In `Grid` mode the
        // demo drives the snap step from the same value as the dot grid so
        // snapped nodes land on the visible grid; in `Point` mode it snaps to
        // whole units regardless of the grid spacing.
        .snap(match state.snap_mode {
            SnapMode::Off => None,
            SnapMode::Point | SnapMode::Grid => Some(egui_graph::Snap::Round),
        })
        .snap_step(match state.snap_mode {
            SnapMode::Grid => state.grid_step,
            _ => 1.0,
        })
        .dot_grid_step(state.grid_step)
        .align(state.align)
        .align_targets(egui_graph::AlignTargets {
            edges: state.align_edges,
            centers: state.align_centers,
        })
        .align_threshold(state.align_threshold)
        .align_guides(state.align_guides)
        .show(view, ui, |ui, show| {
            show.nodes(ui, |nctx, ui| nodes(nctx, ui, state))
                .edges(ui, |ectx, ui| {
                    if state.custom_edge_style {
                        set_edge_style(ui.style_mut(), state);
                    }
                    edges(ectx, ui, state)
                });
        });

    // Sync the demo's selection state when it changes.
    if let Some(selected) = graph_response.selection_changed {
        state.interaction.selection.nodes = selected
            .iter()
            .filter_map(|node_id| state.node_id_map.get(node_id).copied())
            .collect();
    }
}

fn set_edge_style(style: &mut egui::Style, state: &mut State) {
    let vis_mut = &mut style.visuals;
    // Edges use `noninteractive` by default, `hovered` when hovered.
    vis_mut.widgets.noninteractive.fg_stroke.color = state.edge_color;
    vis_mut.widgets.noninteractive.fg_stroke.width = state.edge_width;
    vis_mut.widgets.hovered.fg_stroke.color = state.edge_color.linear_multiply(1.25);
    vis_mut.widgets.hovered.fg_stroke.width = state.edge_width;
    // Exaggerate the color and width when selected.
    vis_mut.selection.stroke.color = state.edge_color.linear_multiply(1.5);
    vis_mut.selection.stroke.width = state.edge_width * 3.0;
}

fn nodes(nctx: &mut egui_graph::NodesCtx, ui: &mut egui::Ui, state: &mut State) {
    let indices: Vec<_> = state.graph.node_indices().collect();
    for n in indices {
        let (inputs, outputs) = socket_counts(&state.graph, n);
        let flow = node_flow(&state.graph[n], state.flow);
        let node = &mut state.graph[n];
        let node_id = egui_graph::NodeId::from_u64(n.index() as u64);
        state.node_id_map.insert(node_id, n);
        let response = egui_graph::node::Node::from_id(node_id)
            .inputs(inputs)
            .outputs(outputs)
            .flow(flow)
            .socket_radius(state.socket_radius)
            .socket_color(state.socket_color)
            .show_or_cull(nctx, ui, |node_ctx| {
                node_ctx.framed(|ui, sockets| match node.kind {
                    NodeKind::Label => {
                        ui.label(&node.name);
                    }
                    NodeKind::Button => {
                        ui.horizontal(|ui| {
                            if ui.button(&node.name).clicked() {
                                println!("{}", node.name);
                            }
                        });
                    }
                    NodeKind::DragValue(ref mut f) => {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(f).range(0.0..=255.0));
                        });
                    }
                    NodeKind::Slider(ref mut f) => {
                        ui.horizontal(|ui| ui.add(egui::Slider::new(f, 0.0..=1.0)));
                    }
                    NodeKind::Comment(ref mut text) => {
                        ui.text_edit_multiline(text);
                    }
                    NodeKind::Mixer {
                        ref mut color,
                        ref mut alpha,
                    } => {
                        if flow.is_horizontal() {
                            sockets.grid(egui::Grid::new("mixer"), ui, |grid, ui| {
                                grid.row(ui, Some(0), None, |ui| {
                                    ui.label("Color");
                                    ui.add(egui::Slider::new(color, 0.0..=1.0));
                                });
                                grid.row(ui, Some(1), Some(0), |ui| {
                                    ui.label("Alpha");
                                    ui.add(egui::Slider::new(alpha, 0.0..=1.0));
                                });
                            });
                        } else if flow.is_vertical() {
                            ui.horizontal(|ui| {
                                sockets.col(ui, Some(0), None, |ui| {
                                    ui.add(
                                        egui::Slider::new(color, 0.0..=1.0)
                                            .vertical()
                                            .show_value(false),
                                    );
                                });
                                sockets.col(ui, Some(1), Some(0), |ui| {
                                    ui.add(
                                        egui::Slider::new(alpha, 0.0..=1.0)
                                            .vertical()
                                            .show_value(false),
                                    );
                                });
                            });
                        }
                    }
                })
            });

        // Demonstrate socket tooltips.
        for (ix, r) in response.sockets().inputs() {
            r.clone().on_hover_text(format!("Input {ix}"));
        }
        for (ix, r) in response.sockets().outputs() {
            r.clone().on_hover_text(format!("Output {ix}"));
        }

        if response.changed() {
            // Check for an edge event.
            if let Some(ev) = response.edge_event() {
                match ev {
                    EdgeEvent::Started { kind, index } => {
                        state.interaction.edge_in_progress = Some((n, kind, index));
                    }
                    EdgeEvent::Ended { kind, index } => {
                        // Create the edge.
                        if let Some((src, _, ix)) = state.interaction.edge_in_progress.take() {
                            let (a, b, w) = match kind {
                                SocketKind::Input => (src, n, (ix, index)),
                                SocketKind::Output => (n, src, (index, ix)),
                            };
                            // Check that this edge doesn't already exist.
                            if !state
                                .graph
                                .edges(a)
                                .any(|e| e.target() == b && *e.weight() == w)
                            {
                                state.graph.add_edge(a, b, w);
                            }
                        }
                    }
                    EdgeEvent::Cancelled => {
                        state.interaction.edge_in_progress = None;
                    }
                }
            }

            // If the delete key was pressed while selected, remove it.
            if response.removed() {
                state.graph.remove_node(n);
                state.node_id_map.remove(&node_id);
            }
        }
    }
}

fn edges(ectx: &mut egui_graph::EdgesCtx, ui: &mut egui::Ui, state: &mut State) {
    // Count edge occurrences per socket pair to look up the matching route.
    let mut occurrences: HashMap<_, usize> = HashMap::new();
    // Instantiate all edges.
    for e in state.graph.edge_indices().collect::<Vec<_>>() {
        let (na, nb) = state.graph.edge_endpoints(e).unwrap();
        let (output, input) = *state.graph.edge_weight(e).unwrap();
        let a = egui_graph::NodeId::from_u64(na.index() as u64);
        let b = egui_graph::NodeId::from_u64(nb.index() as u64);
        let occurrence = occurrences.entry(((a, output), (b, input))).or_default();
        #[cfg(feature = "layout")]
        let waypoints = if state.route_edges {
            state
                .routes
                .route((a, output), (b, input), *occurrence)
                .unwrap_or(&[])
        } else {
            &[]
        };
        #[cfg(not(feature = "layout"))]
        let waypoints: &[egui::Pos2] = &[];
        *occurrence += 1;
        let mut selected = state.interaction.selection.edges.contains(&e);
        let response = egui_graph::edge::Edge::new((a, output), (b, input), &mut selected)
            .curvature_factor(state.edge_curvature)
            .waypoints(waypoints)
            .show(ectx, ui);

        if response.deleted() {
            state.graph.remove_edge(e);
            state.interaction.selection.edges.remove(&e);
        } else if response.changed() {
            if selected {
                state.interaction.selection.edges.insert(e);
            } else {
                state.interaction.selection.edges.remove(&e);
            }
        }
    }

    // Draw the in-progress edge if there is one.
    if let Some(edge) = ectx.in_progress(ui) {
        edge.show(ui, state.edge_curvature);
    }
}

fn graph_config(ui: &mut egui::Ui, view: &mut egui_graph::View, state: &mut State) {
    let mut frame = egui::Frame::window(ui.style());
    frame.shadow.spread = 0;
    frame.shadow.offset = [0, 0];
    egui::Window::new("Graph Config")
        .frame(frame)
        .anchor(
            egui::Align2::LEFT_TOP,
            ui.spacing().window_margin.left_top(),
        )
        .collapsible(false)
        .title_bar(false)
        .auto_sized()
        .show(ui.ctx(), |ui| {
            ui.label("GRAPH CONFIG");
            #[cfg(feature = "layout")]
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.auto_layout, "Automatic Layout");
                ui.separator();
                ui.add_enabled_ui(!state.auto_layout, |ui| {
                    if ui.button("Layout Once").clicked() {
                        (view.layout, state.routes) = layout(state, ui.ctx());
                    }
                });
            });
            #[cfg(feature = "layout")]
            ui.checkbox(&mut state.socket_aware, "Socket-Aware Layout");
            // With auto-layout the routes follow the layout's corridors;
            // in freehand mode they dodge nodes best-effort.
            #[cfg(feature = "layout")]
            ui.checkbox(&mut state.route_edges, "Edge Routing");
            ui.checkbox(&mut state.center_view, "Center View");
            ui.checkbox(&mut state.auto_pan, "Auto-Pan");
            ui.checkbox(&mut state.immutable, "Immutable");
            ui.horizontal(|ui| {
                ui.label("Flow:");
                ui.radio_value(&mut state.flow, egui::Direction::LeftToRight, "Right");
                ui.radio_value(&mut state.flow, egui::Direction::TopDown, "Down");
            });
            // Override the flow of the selected nodes to showcase mixed-flow
            // layout: differing flows split the graph into clusters laid out
            // each in their own direction.
            ui.horizontal(|ui| {
                ui.label("Selected flow:");
                let any_selected = !state.interaction.selection.nodes.is_empty();
                ui.add_enabled_ui(any_selected, |ui| {
                    if ui.button("Right").clicked() {
                        set_selected_flow(state, Some(egui::Direction::LeftToRight));
                    }
                    if ui.button("Down").clicked() {
                        set_selected_flow(state, Some(egui::Direction::TopDown));
                    }
                    if ui.button("Default").clicked() {
                        set_selected_flow(state, None);
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.label("Edge curvature:");
                ui.add(egui::Slider::new(&mut state.edge_curvature, 0.0..=1.0));
            });
            #[cfg(feature = "layout")]
            ui.horizontal(|ui| {
                ui.label("Layer gap:");
                ui.add(egui::Slider::new(&mut state.layer_gap, 10.0..=200.0));
            });
            #[cfg(feature = "layout")]
            ui.horizontal(|ui| {
                ui.label("Node gap:");
                ui.add(egui::Slider::new(&mut state.node_gap, 10.0..=200.0));
            });
            ui.checkbox(&mut state.custom_edge_style, "Custom Edge Style");
            ui.add_enabled_ui(state.custom_edge_style, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Edge width:");
                    ui.add(egui::Slider::new(&mut state.edge_width, 0.5..=10.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Socket radius:");
                    ui.add(egui::Slider::new(&mut state.socket_radius, 1.0..=10.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Edge color:");
                    ui.color_edit_button_srgba(&mut state.edge_color);
                    ui.label("Socket color:");
                    ui.color_edit_button_srgba(&mut state.socket_color);
                });
            });

            ui.add_space(8.0);
            ui.label("SNAP CONFIG");
            ui.horizontal(|ui| {
                ui.radio_value(&mut state.snap_mode, SnapMode::Off, "Off");
                ui.radio_value(&mut state.snap_mode, SnapMode::Point, "Snap to Point");
                ui.radio_value(&mut state.snap_mode, SnapMode::Grid, "Snap to Grid");
            });

            ui.add_space(8.0);
            ui.label("GRID CONFIG");
            ui.checkbox(&mut state.dot_grid, "Show Dot Grid");
            ui.horizontal(|ui| {
                ui.label("Step:");
                ui.add(egui::DragValue::new(&mut state.grid_step).range(1.0..=200.0));
            });

            ui.add_space(8.0);
            ui.label("ALIGN CONFIG");
            ui.checkbox(&mut state.align, "Snap-align (hold Alt to disable)");
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.align_edges, "Edges");
                ui.checkbox(&mut state.align_centers, "Centers");
                ui.checkbox(&mut state.align_guides, "Guides");
            });
            ui.horizontal(|ui| {
                ui.label("Threshold (px):");
                ui.add(egui::DragValue::new(&mut state.align_threshold).range(0.0..=50.0));
            });
            // One-shot: align the selection onto an inferred axis. The axis is
            // inferred from the selection's spread; "Align by" picks the feature
            // (edges vs centers) that is unified. Disabled for < 2 nodes.
            ui.horizontal(|ui| {
                ui.label("Align by:");
                ui.radio_value(&mut state.align_by, egui_graph::AlignBy::Min, "Min");
                ui.radio_value(&mut state.align_by, egui_graph::AlignBy::Center, "Center");
                ui.radio_value(&mut state.align_by, egui_graph::AlignBy::Max, "Max");
            });
            let can_align = state.interaction.selection.nodes.len() >= 2;
            ui.add_enabled_ui(can_align, |ui| {
                if ui.button("Align Selection").clicked() {
                    let ids: Vec<egui_graph::NodeId> = state
                        .interaction
                        .selection
                        .nodes
                        .iter()
                        .map(|idx| egui_graph::NodeId::from_u64(idx.index() as u64))
                        .collect();
                    let by = state.align_by;
                    egui_graph::with_graph_memory(ui.ctx(), graph_id(), |gmem| {
                        egui_graph::align_nodes(&mut view.layout, ids, gmem.node_sizes(), by, None);
                    });
                }
            });

            ui.add_space(8.0);
            ui.label(format!("Scene: {:?}", view.scene_rect));
        });
}

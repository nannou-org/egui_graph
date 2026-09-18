use crate::socket::{SocketKind, SocketResponses};
use crate::NodesCtx;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

/// Derive the `egui::Id` used internally for a node's egui state (layers, area_rect, etc).
pub fn egui_id(graph_id: egui::Id, node_id: NodeId) -> egui::Id {
    graph_id.with(node_id.0)
}

/// Describes the interaction state of a node.
///
/// This is passed to the frame closure to allow styling based on selection state.
#[derive(Clone, Copy, Debug, Default)]
pub struct NodeInteraction {
    /// Whether the node is currently selected.
    pub selected: bool,
    /// Whether the node is within the current selection rectangle.
    pub in_selection_rect: bool,
    /// Whether or not the pointer is hovered over the node.
    pub hovered: bool,
}

/// The default node widget.
///
/// A `Node` is a thin wrapper around a `Window` and allows for instantiating arbitrary widgets
/// internally.
pub struct Node {
    id: NodeId,
    inputs: usize,
    outputs: usize,
    collapsed: bool,
    flow: egui::Direction,
    socket_radius: f32,
    socket_color: Option<egui::Color32>,
    max_width: Option<f32>,
    animation_time: f32,
}

/// A unique identifier for a node within a graph.
///
/// This is decoupled from `egui::Id` to allow node identity to remain stable
/// even when the graph's egui ID context changes (e.g., when viewing the same
/// graph through different heads or paths).
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct NodeId(pub u64);

/// An extension around the `egui::Response` that indicates node selection.
pub struct NodeResponse<T> {
    response: egui::InnerResponse<T>,
    sockets: SocketResponses,
    selection_changed: bool,
    selected: bool,
    removed: bool,
    /// Some event occurred related to the creation of an edge.
    edge_event: Option<EdgeEvent>,
}

/// The response returned by [`NodeCtx::framed`] and [`NodeCtx::framed_with`].
///
/// Carries the content's [`egui::InnerResponse`] alongside the
/// [`SocketLayout`](crate::SocketLayout) describing socket positions for this node.
pub struct FramedResponse<T> {
    pub inner: egui::InnerResponse<T>,
    pub sockets: crate::SocketLayout,
}

/// The response returned by [`Node::show_static`].
pub struct StaticNodeResponse<T> {
    /// The framed content's response.
    pub inner: egui::InnerResponse<T>,
    /// The hover response of each socket.
    pub sockets: SocketResponses,
}

/// Events related to the creation of an edge to or from a node.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EdgeEvent {
    /// Occurs if a socket was just pressed to start creating an edge.
    Started { kind: SocketKind, index: usize },
    /// Occurs when the primary mouse button was released creating an edge ending at the specified
    /// socket.
    Ended { kind: SocketKind, index: usize },
    /// If there was an edge in progress starting from this node, this indicates that it was
    /// cancelled.
    Cancelled,
}

/// Context passed to the node's content closure.
///
/// Provides access to interaction state and style, and ensures content is properly framed.
pub struct NodeCtx<'a> {
    ui: &'a mut egui::Ui,
    interaction: NodeInteraction,
    min_size: egui::Vec2,
    graph_id: egui::Id,
    node_id: NodeId,
    immutable: bool,
    flow: egui::Direction,
    inputs: usize,
    outputs: usize,
}

impl Node {
    const COLLAPSED_SOCKET_GAP_FACTOR: f32 = 0.25;

    /// Begin instantiating a new node widget.
    pub fn new(id_src: impl Hash) -> Self {
        Self::from_id(NodeId::new(id_src))
    }

    /// Construct the node directly from its `NodeId`.
    pub fn from_id(id: NodeId) -> Self {
        Self {
            id,
            max_width: None,
            socket_color: None,
            inputs: 0,
            outputs: 0,
            collapsed: false,
            flow: egui::Direction::LeftToRight,
            socket_radius: 3.0,
            animation_time: 0.1,
        }
    }

    /// Optionally specify the max width of the `Node`'s window.
    ///
    /// By default, `ui.spacing().text_edit_width` is used.
    pub fn max_width(mut self, w: f32) -> Self {
        self.max_width = Some(w);
        self
    }

    pub fn inputs(mut self, n: usize) -> Self {
        self.inputs = n;
        self
    }

    pub fn outputs(mut self, n: usize) -> Self {
        self.outputs = n;
        self
    }

    /// Allow the node to shrink below the usual socket-spacing-driven minimum size
    /// when there are two or more sockets.
    ///
    /// Collapsed nodes still preserve a small amount of socket separation so
    /// multiple sockets do not fully overlap.
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// The direction of dataflow through the graph.
    ///
    /// This determines which edges the inputs and outputs are distributed across.
    ///
    /// E.g. `LeftToRight` will have inputs on the left and outputs on the right.
    ///
    /// On vertical edges, inputs/outputs start at the top and end at the bottom.
    ///
    /// On horizontal edges, inputs/outputs start at the left and end at the right.
    ///
    /// Default direction is `LeftToRight`.
    pub fn flow(mut self, flow: egui::Direction) -> Self {
        self.flow = flow;
        self
    }

    /// The color of the input and output sockets.
    pub fn socket_color(mut self, color: egui::Color32) -> Self {
        self.socket_color = Some(color);
        self
    }

    /// The radius of the input and output sockets.
    pub fn socket_radius(mut self, radius: f32) -> Self {
        self.socket_radius = radius;
        self
    }

    /// The time taken (seconds) for the node to interpolate toward a new location.
    ///
    /// Default: `0.1`.
    pub fn animation_time(mut self, time: f32) -> Self {
        self.animation_time = time;
        self
    }

    /// Present the `Node`'s `Window` and add the given contents.
    ///
    /// The content closure receives a [`NodeCtx`] which provides access to interaction state
    /// and ensures content is properly framed. Use [`NodeCtx::framed`] for default styling
    /// or [`NodeCtx::framed_with`] for custom frame styling:
    ///
    /// ```ignore
    /// Node::from_id(id).show(ctx, ui, |node_ctx| {
    ///     node_ctx.framed(|ui, _sockets| {
    ///         ui.label("Hello");
    ///     })
    /// });
    /// ```
    pub fn show<R>(
        self,
        ctx: &mut NodesCtx,
        ui: &mut egui::Ui,
        content: impl FnOnce(NodeCtx<'_>) -> FramedResponse<R>,
    ) -> NodeResponse<R> {
        self.show_impl(ctx, ui, Box::new(content) as Box<_>)
    }

    /// Show the node as a static widget at the ui cursor, outside any graph.
    ///
    /// The frame and sockets paint exactly as within a
    /// [`Graph`](crate::Graph), but there is no layout, selection, dragging
    /// or edge interaction. `content` must call [`NodeCtx::framed`] or
    /// [`NodeCtx::framed_with`], as within a graph. The socket responses
    /// sense hover only, for tooltips.
    pub fn show_static<R>(
        self,
        ui: &mut egui::Ui,
        content: impl FnOnce(NodeCtx<'_>) -> FramedResponse<R>,
    ) -> StaticNodeResponse<R> {
        let (min_size, content_min_size, socket_padding) = self.min_sizes(ui);
        let max_w = self.max_width.unwrap_or(ui.spacing().text_edit_width);
        let put_size = egui::Vec2::new(max_w, min_size.y);
        let put_rect = egui::Rect::from_min_size(ui.cursor().min, put_size);
        let graph_id = ui.id();
        let egui_id = egui_id(graph_id, self.id);
        let (socket_layer, frame_layer) = sublayers(ui, egui_id);

        let builder = egui::UiBuilder::new()
            .max_rect(put_rect)
            .layer_id(frame_layer);
        let inner_response = ui.scope_builder(builder, |ui| {
            let node_ctx = NodeCtx {
                ui,
                interaction: NodeInteraction::default(),
                min_size: content_min_size,
                graph_id,
                node_id: self.id,
                immutable: false,
                flow: self.flow,
                inputs: self.inputs,
                outputs: self.outputs,
            };
            content(node_ctx)
        });
        let FramedResponse {
            inner,
            sockets: socket_layout,
        } = inner_response.inner;

        let rect = inner.response.rect;
        let node_sockets = socket_layout.resolve(self.flow, rect, socket_padding);
        let socket_color = self.socket_color.unwrap_or(ui.visuals().text_color());
        let sockets = crate::socket::paint(
            ui,
            egui_id,
            socket_layer,
            rect,
            &node_sockets,
            socket_color,
            self.socket_radius,
            |_, _| false,
        );
        StaticNodeResponse { inner, sockets }
    }

    /// The minimum frame size that fits the sockets, the minimum content
    /// size within the frame's corner radius, and the socket padding.
    fn min_sizes(&self, ui: &egui::Ui) -> (egui::Vec2, egui::Vec2, f32) {
        // The window should always be at least the interaction size.
        let min_item_spacing = ui.spacing().item_spacing.x.min(ui.spacing().item_spacing.y);
        let min_interact_len = ui
            .spacing()
            .interact_size
            .x
            .min(ui.spacing().interact_size.y);
        let mut min_size = egui::Vec2::splat(min_interact_len);
        // However, it should also always be at least large enough to comfortably show all
        // inlets/outlets.
        let max_sockets = std::cmp::max(self.inputs, self.outputs);
        let min_socket_gap = min_interact_len + min_item_spacing;
        let win_corner_radius = ui.visuals().window_corner_radius.ne as f32;
        let socket_padding = crate::socket::socket_padding(ui.style());
        if max_sockets > 1 {
            let socket_gap_factor = if self.collapsed {
                Self::COLLAPSED_SOCKET_GAP_FACTOR
            } else {
                1.0
            };
            let min_len = (max_sockets - 1) as f32 * min_socket_gap * socket_gap_factor
                + socket_padding * 2.0;
            match self.flow {
                egui::Direction::LeftToRight | egui::Direction::RightToLeft => {
                    min_size.y = min_size.y.max(min_len);
                }
                egui::Direction::TopDown | egui::Direction::BottomUp => {
                    min_size.x = min_size.x.max(min_len);
                }
            }
        }
        // The content accounts for the frame corner radius.
        let gap = egui::Vec2::splat(win_corner_radius * 2.0);
        let content_min_size = min_size - gap;
        (min_size, content_min_size, socket_padding)
    }

    fn show_impl<'a, R>(
        self,
        ctx: &mut NodesCtx,
        ui: &mut egui::Ui,
        content: Box<dyn FnOnce(NodeCtx<'_>) -> FramedResponse<R> + 'a>,
    ) -> NodeResponse<R> {
        let snap = ctx.snap;
        let snap_step = ctx.snap_step;
        let layout = &mut ctx.layout;

        // Indicate that we've visited this node this update.
        ctx.visited.insert(self.id);

        // Determine the current position of the window relative to the graph origin.
        let target_pos_graph = layout.entry(self.id).or_insert_with(|| {
            // If the mouse is over the graph, add the node under the mouse.
            // Otherwise, add the node to the top-left.
            let clip_rect = ui.clip_rect();
            let mut pos = clip_rect.center();
            if ui.rect_contains_pointer(clip_rect) {
                if let Some(ptr) = ui.response().hover_pos() {
                    pos = ptr;
                }
            }
            // Snap the initial position so a freshly-spawned node is aligned on
            // its very first frame (later frames are snapped in `Graph::show`).
            match snap {
                Some(snap) => crate::snap_pos(snap, snap_step, pos),
                None => pos,
            }
        });

        // Interpolate toward the desired position over time for auto-layout.
        // Only do so if this node is not selected with the primary mouse down.
        let is_selected = crate::is_node_selected(ui, ctx.graph_id, self.id);
        let is_primary_down = ui.input(|i| i.pointer.primary_down());
        let animation_time = if is_selected && is_primary_down {
            0.0
        } else {
            self.animation_time
        };
        let pos_graph = {
            let idx = ctx.graph_id.with(self.id.0).with("x");
            let idy = ctx.graph_id.with(self.id.0).with("y");
            let ctx = ui.ctx();
            let x = ctx.animate_value_with_time(idx, target_pos_graph.x, animation_time);
            let y = ctx.animate_value_with_time(idy, target_pos_graph.y, animation_time);
            egui::Pos2::new(x, y)
        };

        let (min_size, content_min_size, socket_padding) = self.min_sizes(ui);

        let max_w = self.max_width.unwrap_or(ui.spacing().text_edit_width);
        let max_size = egui::Vec2::new(max_w, ctx.graph_rect.height());

        // Track changes in selection for the node response.
        let mut selection_changed = false;

        // Determine whether or not this node is within the selection rect.
        // If `shift` is down, rectangle selection is reserved for edges.
        // NOTE: We use the size from last frame as we don't know the size until
        // the user's content is added... Is there a better way to handle this?
        let (mut selected, in_selection_rect) = {
            let gmem_arc = crate::memory(ui, ctx.graph_id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            let in_selection_rect = match ctx.selection_rect {
                Some(sel_rect) if ui.input(|i| !i.modifiers.shift) => {
                    let size = gmem
                        .node_sizes
                        .get(&self.id)
                        .cloned()
                        .unwrap_or(egui::Vec2::ZERO);
                    let rect = egui::Rect::from_min_size(pos_graph, size);
                    sel_rect.intersects(rect)
                }
                _ => false,
            };

            // Update the selection if the primary mouse button was just released.
            if ctx.select {
                if in_selection_rect && ui.input(|i| !i.modifiers.shift) {
                    selection_changed |= gmem.selection.nodes.insert(self.id);
                } else if !ui.input(|i| i.modifiers.ctrl) {
                    selection_changed |= gmem.selection.nodes.remove(&self.id);
                }
            }

            let selected = gmem.selection.nodes.contains(&self.id);

            (selected, in_selection_rect)
        };

        // Custom framed node container that remains in the scene's layer
        let put_size = egui::Vec2::new(max_size.x, min_size.y);
        let put_rect = egui::Rect::from_min_size(pos_graph, put_size);

        let node_id = self.id;
        let egui_id = egui_id(ctx.graph_id, node_id);
        let (socket_layer, frame_layer) = sublayers(ui, egui_id);

        // A `Ui` scope for the node's layer.
        let builder = egui::UiBuilder::new()
            .max_rect(put_rect)
            .layer_id(frame_layer)
            .sense(egui::Sense::click_and_drag());
        let immutable = ctx.immutable;
        let inner_response = ui.scope_builder(builder, |ui| {
            let hovered = ui.response().hovered();
            // Create the NodeCtx and call the user's content closure.
            // The user is responsible for calling `framed` or `default_framed`
            // on the context.
            let node_ctx = NodeCtx {
                ui,
                interaction: NodeInteraction {
                    selected,
                    in_selection_rect,
                    hovered,
                },
                min_size: content_min_size,
                graph_id: ctx.graph_id,
                node_id,
                immutable,
                flow: self.flow,
                inputs: self.inputs,
                outputs: self.outputs,
            };
            content(node_ctx)
        });

        // Take the union of the ui scope and the frame response to monitor for
        // interactions.
        let FramedResponse {
            inner: content_inner_response,
            sockets: socket_layout,
        } = inner_response.inner;
        let mut response = inner_response
            .response
            .union(content_inner_response.response);
        let content_output = content_inner_response.inner;

        // Update the stored data for this node and check for edge events.
        let mut edge_event = None;
        {
            let gmem_arc = crate::memory(ui, ctx.graph_id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            // Snap the recorded size so auto-layout and selection see tidy
            // values. Sizes always round (never floor) so they can't shrink
            // below the rendered frame and break hit-testing or packing.
            let size = match snap {
                Some(_) => crate::snap_vec(crate::Snap::Round, snap_step, response.rect.size()),
                None => response.rect.size(),
            };
            gmem.node_sizes.insert(self.id, size);

            // Treat the pointer being over this node's frame as "over the graph"
            // for next frame's socket detection (see `lib.rs`). `contains_pointer`
            // stays true mid-drag (drag-and-drop target semantics) and is false
            // when a window fully occludes the frame.
            gmem.ptr_over_node |= response.contains_pointer();

            let ctrl_down = ui.input(|i| i.modifiers.ctrl);

            // If the window is pressed, select the node.
            let pointer = &ui.input(|i| i.pointer.clone());
            if response.is_pointer_button_down_on() && pointer.primary_pressed() {
                // If ctrl is down, check for deselection.
                let was_selected = gmem.selection.nodes.contains(&self.id);
                if ctrl_down && was_selected {
                    selection_changed |= gmem.selection.nodes.remove(&self.id);
                    selected = false;
                } else {
                    // Clear other selections if ctrl is not pressed and this is newly pressed.
                    if !ctrl_down && !was_selected {
                        gmem.selection.changed |= !gmem.selection.nodes.is_empty();
                        gmem.selection.nodes.clear();
                    }
                    selection_changed |= gmem.selection.nodes.insert(self.id);
                    selected = true;
                    // Initialize drag - skip when immutable (still allow click-to-select above).
                    if !immutable && gmem.pressed.is_none() {
                        let ptr_graph = response.hover_pos().unwrap_or_default();
                        gmem.pressed = Some(crate::Pressed {
                            over_selection_at_origin: true,
                            origin_pos: ptr_graph,
                            current_pos: ptr_graph,
                            action: crate::PressAction::DragNodes {
                                node: Some(crate::PressedNode {
                                    id: self.id,
                                    position_at_origin: pos_graph,
                                }),
                            },
                        });
                    }
                }

            // If the primary button was pressed, check for edge events (skip when immutable).
            } else if !immutable
                && !response.is_pointer_button_down_on()
                && pointer.primary_pressed()
            {
                // If this node's socket was pressed, create a start event.
                if let Some(ref pressed) = gmem.pressed {
                    if let crate::PressAction::Socket(socket) = pressed.action {
                        if self.id == socket.node {
                            let kind = socket.kind;
                            let index = socket.index;
                            edge_event = Some(EdgeEvent::Started { kind, index });
                        }
                    }
                }

                // Also check for deselection.
                if !ctrl_down
                    && !gmem
                        .pressed
                        .as_ref()
                        .map(|p| p.over_selection_at_origin)
                        .unwrap_or(false)
                    && !gmem.selection.nodes.contains(&self.id)
                {
                    selection_changed |= gmem.selection.nodes.remove(&self.id);
                    selected = false;
                }

            // Check for edge creation / cancellation events (skip when immutable).
            } else if !immutable {
                if let Some(r) = ctx.socket_press_released {
                    if let Some(c) = gmem.closest_socket {
                        if r.kind == c.kind && self.id == r.node {
                            edge_event = Some(EdgeEvent::Cancelled);
                        } else if self.id == c.node && ui.input(|i| i.pointer.primary_released()) {
                            let kind = c.kind;
                            let index = c.index;
                            edge_event = Some(EdgeEvent::Ended { kind, index });
                        }
                    } else if edge_event.is_none() && self.id == r.node {
                        edge_event = Some(EdgeEvent::Cancelled);
                    }
                }
            }
        }

        // Resolve the socket layout to concrete positions.
        let node_sockets = socket_layout.resolve(self.flow, response.rect, socket_padding);

        // Paint and interact with all sockets.
        let socket_color = self.socket_color.unwrap_or(ui.visuals().text_color());
        let socket_responses = crate::socket::show(
            ui,
            ctx.graph_id,
            self.id,
            egui_id,
            socket_layer,
            response.rect,
            &node_sockets,
            socket_color,
            self.socket_radius,
        );

        // If the delete or backspace key was pressed and the node is selected, remove it.
        // Skip when immutable.
        let removed = if !immutable
            && selected
            && !ui.ctx().egui_wants_keyboard_input()
            && ui.input(|i| i.key_pressed(egui::Key::Delete) | i.key_pressed(egui::Key::Backspace))
        {
            // Remove ourselves from the selection.
            let gmem_arc = crate::memory(ui, ctx.graph_id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            selection_changed |= gmem.selection.nodes.remove(&self.id);
            selected = false;
            true
        } else {
            false
        };

        // Propagate this node's selection change to the graph-level dirty flag.
        if selection_changed {
            let gmem_arc = crate::memory(ui, ctx.graph_id);
            let mut gmem = gmem_arc.lock().expect("failed to lock graph temp memory");
            gmem.selection.changed = true;
        }

        if selection_changed || removed || edge_event.is_some() {
            response.mark_changed();
        }

        NodeResponse {
            response: egui::InnerResponse::new(content_output, response),
            sockets: socket_responses,
            selection_changed,
            selected,
            removed,
            edge_event,
        }
    }
}

impl NodeId {
    /// Create a new NodeId by hashing any hashable value.
    pub fn new(id_src: impl Hash) -> Self {
        use std::hash::Hasher;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        id_src.hash(&mut hasher);
        NodeId(hasher.finish())
    }

    /// Create a NodeId directly from a u64.
    pub const fn from_u64(v: u64) -> Self {
        NodeId(v)
    }

    /// Get the raw u64 value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u64> for NodeId {
    fn from(v: u64) -> Self {
        NodeId(v)
    }
}

impl<R> NodeResponse<R> {
    /// Whether or not the selection changed and, if so, whether or not the node is now selected.
    pub fn selection(&self) -> Option<bool> {
        if self.selection_changed {
            Some(self.selected)
        } else {
            None
        }
    }

    /// Whether or not the node is selected.
    pub fn selected(&self) -> bool {
        self.selected
    }

    /// Whether or not the node was removed from the graph.
    ///
    /// This occurs if the `Delete` key is pressed while the node is selected.
    pub fn removed(&self) -> bool {
        self.removed
    }

    /// A reference to the inner value returned by the node's content.
    pub fn inner(&self) -> &R {
        &self.response.inner
    }

    /// A mutable reference to the inner value returned by the node's content.
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.response.inner
    }

    /// Consume the node response and produce the inner `egui::InnerResponse`.
    pub fn into_inner(self) -> egui::InnerResponse<R> {
        self.response
    }

    /// Whether or not any events occurred related to the creation of an edge.
    pub fn edge_event(&self) -> Option<EdgeEvent> {
        self.edge_event
    }

    /// The collected [`egui::Response`]s for each socket on this node.
    pub fn sockets(&self) -> &SocketResponses {
        &self.sockets
    }
}

impl<R> Deref for NodeResponse<R> {
    type Target = egui::Response;
    fn deref(&self) -> &Self::Target {
        &self.response.response
    }
}

impl<R> DerefMut for NodeResponse<R> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.response.response
    }
}

impl<'a> NodeCtx<'a> {
    /// The current interaction state of the node.
    pub fn interaction(&self) -> NodeInteraction {
        self.interaction
    }

    /// The current egui style.
    pub fn style(&self) -> &egui::Style {
        self.ui.style()
    }

    /// The ID of the graph with which this node is associated.
    pub fn graph_id(&self) -> egui::Id {
        self.graph_id
    }

    /// The unique node ID for this node (i.e. it's index within the graph).
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// The unique egui ID for this node (the graph ID combined with the node ID).
    ///
    /// This is useful for creating persistent IDs for sub-components like Resize containers.
    pub fn egui_id(&self) -> egui::Id {
        egui_id(self.graph_id(), self.node_id())
    }

    /// Show content with the default frame styling.
    ///
    /// This consumes the context, ensuring content is only added once.
    ///
    /// Returns a [`FramedResponse`] containing the combined response and
    /// socket layout. The response:
    /// - Has a rect covering the entire framed area.
    /// - Reports interactions (clicks, drags, hovers) on any part of the node.
    /// - Is used by [`Node::show`] for selection and drag handling.
    ///
    /// The content closure receives a `&mut SocketLayout` which can be used to
    /// register explicit socket positions (e.g. via [`SocketLayout::row`](crate::SocketLayout::row)).
    /// If left unmodified, sockets are evenly spaced along the node edge.
    ///
    /// For custom frame styling, use [`NodeCtx::framed_with`].
    pub fn framed<T>(
        self,
        content: impl FnOnce(&mut egui::Ui, &mut crate::SocketLayout) -> T,
    ) -> FramedResponse<T> {
        let frame = default_frame(self.style(), self.interaction);
        self.framed_with(frame, content)
    }

    /// Show content within a custom frame.
    ///
    /// This consumes the context, ensuring content is only added once.
    ///
    /// Returns a [`FramedResponse`] containing the combined response and
    /// socket layout. The response:
    /// - Has a rect covering the entire framed area.
    /// - Reports interactions (clicks, drags, hovers) on any part of the node.
    /// - Is used by [`Node::show`] for selection and drag handling.
    ///
    /// The content closure receives a `&mut SocketLayout` which can be used to
    /// register explicit socket positions (e.g. via [`SocketLayout::row`](crate::SocketLayout::row)).
    /// If left unmodified, sockets are evenly spaced along the node edge.
    ///
    /// For default frame styling, use [`NodeCtx::framed`].
    pub fn framed_with<T>(
        self,
        frame: egui::Frame,
        content: impl FnOnce(&mut egui::Ui, &mut crate::SocketLayout) -> T,
    ) -> FramedResponse<T> {
        let min_size = self.min_size;
        let immutable = self.immutable;
        let mut socket_layout =
            crate::SocketLayout::evenly_spaced(self.flow, self.inputs, self.outputs);
        let builder = egui::UiBuilder::new().sense(egui::Sense::click_and_drag());
        let inner_response = frame.show(self.ui, |ui| {
            ui.scope_builder(builder, |ui| {
                ui.set_min_size(min_size);
                // Disable content widgets when immutable (inside the frame
                // so that the frame itself retains normal styling).
                if immutable {
                    ui.disable();
                }
                content(ui, &mut socket_layout)
            })
        });
        let response = inner_response.response.union(inner_response.inner.response);
        let content_output = inner_response.inner.inner;
        FramedResponse {
            inner: egui::InnerResponse::new(content_output, response),
            sockets: socket_layout,
        }
    }
}

/// Register the node's two sublayers of the ui's layer: the socket layer
/// below the frame layer, so node content takes interaction precedence.
/// Both follow the ui layer's transform.
fn sublayers(ui: &egui::Ui, egui_id: egui::Id) -> (egui::LayerId, egui::LayerId) {
    let parent = ui.layer_id();
    let socket_layer = egui::LayerId::new(parent.order, egui_id.with("sockets"));
    let frame_layer = egui::LayerId::new(parent.order, egui_id);
    for layer in [socket_layer, frame_layer] {
        ui.ctx().set_sublayer(parent, layer);
        if let Some(transform) = ui.ctx().layer_transform_to_global(parent) {
            ui.ctx().set_transform_layer(layer, transform);
        }
    }
    (socket_layer, frame_layer)
}

/// The default frame styling used for the `Node`'s `Window`.
///
/// This applies selection styling based on the `NodeInteraction` state:
/// - Selected nodes get the selection stroke.
/// - Nodes in the selection rectangle get a dimmed selection stroke.
pub fn default_frame(style: &egui::Style, interaction: NodeInteraction) -> egui::Frame {
    let mut frame = egui::Frame::window(style);
    frame.shadow.offset = [2, 2];
    frame.shadow.spread = (frame.shadow.spread as f32 * 0.25) as u8;

    // Style the frame based on interaction.
    frame.stroke.width = style.visuals.selection.stroke.width;
    if interaction.selected {
        frame.stroke = style.visuals.selection.stroke;
    } else if interaction.in_selection_rect {
        let color = style.visuals.weak_text_color();
        frame.shadow.color = color;
        frame.stroke = style.visuals.selection.stroke;
        frame.stroke.color = color;
    }

    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A static node paints outside any graph and yields a hover response
    /// per socket. Inputs sit on the frame's top edge in index order and
    /// outputs on its bottom edge.
    #[test]
    fn static_node_paints_sockets_on_the_frame() {
        let ctx = egui::Context::default();
        let mut out = None;
        // Two passes, since fonts and sizes settle after the first.
        for _ in 0..2 {
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
                let res = Node::from_id(NodeId(0))
                    .inputs(2)
                    .outputs(1)
                    .flow(egui::Direction::TopDown)
                    .show_static(ui, |c| c.framed(|ui, _| ui.label("x")));
                let rect = res.inner.response.rect;
                let inputs: Vec<egui::Pos2> =
                    res.sockets.inputs().map(|(_, r)| r.rect.center()).collect();
                let outputs: Vec<egui::Pos2> = res
                    .sockets
                    .outputs()
                    .map(|(_, r)| r.rect.center())
                    .collect();
                out = Some((rect, inputs, outputs));
            });
        }
        let (rect, inputs, outputs) = out.expect("the panel ran");
        assert_eq!(inputs.len(), 2);
        assert_eq!(outputs.len(), 1);
        for pos in &inputs {
            assert!((pos.y - rect.top()).abs() < 1e-3, "{pos:?} off {rect:?}");
        }
        assert!((outputs[0].y - rect.bottom()).abs() < 1e-3);
        assert!(inputs[0].x < inputs[1].x);
    }
}

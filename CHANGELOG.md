# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.16.2](https://github.com/nannou-org/egui_graph/compare/v0.16.1...v0.16.2) - 2026-07-13

### Added

- *(paint)* polyline offset, parallel-strand and hatch helpers
- *(edge)* expose custom edge painting via show_with

## [0.16.1](https://github.com/nannou-org/egui_graph/compare/v0.16.0...v0.16.1) - 2026-06-27

### Added

- add align_nodes to align a selection onto an inferred axis
- draw subtle alignment guide lines while dragging
- snap-align dragged nodes to other nodes' edges/centers
- *(example)* add snap-mode and grid-step controls to demo
- expose configurable dot grid step
- snap node positions and frame sizes to graph units

### Fixed

- complete edge when released over a node frame
- keep selected node groups rigid when snapping during a drag
- *(example)* use AutoNoVsync present mode in demo

### Other

- deny rustdoc warnings and fix unresolved doc links

## [0.16.0](https://github.com/nannou-org/egui_graph/compare/v0.15.0...v0.16.0) - 2026-06-18

### Added

- *(example)* add a mixed-flow layout example
- *(demo)* showcase mixed-flow layout via per-node flow
- *(layout)* arrange mixed-flow clusters via a meta-graph
- *(layout)* per-node flow with uniform-flow clustering
- *(layout)* add socket_aware param for classic socket-blind layout
- *(layout)* add freehand edge routing via route_edges
- *(demo)* gate edge routing on auto-layout and add a toggle
- *(edge)* route edges through layout corridors
- *(layout)* emit edge routes through reserved corridors
- Add node_sockets accessor and randomized layout property tests
- *(layout)* replace layout-rs with in-crate socket-aware layered layout
- add ResizeBehavior to preserve zoom on viewport resize
- Expose per-edge stroke overrides

### Other

- migrate release flow to release-plz with trusted publishing
- document per-node flow and mixed-flow layout
- format merged dot-grid test (rustfmt)
- *(demo)* enable auto-layout by default, drop frame-time readout
- bound the dot grid's per-frame dot count
- Revert "chore: temporary diagnostics for startup slowdown investigation"
- temporary diagnostics for startup slowdown investigation
- *(demo)* default layout toggles off and show frame time
- Document layout_routed and Edge::waypoints in README
- *(socket)* extract shared socket geometry helpers

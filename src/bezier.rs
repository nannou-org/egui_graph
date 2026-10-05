/// A very basic cubic bezier type for presenting edges between nodes.
#[derive(Clone, Copy, Debug)]
pub struct Cubic {
    pub from: egui::Pos2,
    pub ctrl1: egui::Pos2,
    pub ctrl2: egui::Pos2,
    pub to: egui::Pos2,
}

/// A smooth, piecewise-cubic path threaded through a sequence of waypoints.
///
/// Used to route edges through the corridors reserved by the automatic
/// layout; without waypoints it is equivalent to a single [`Cubic`].
#[derive(Clone, Debug)]
pub struct Path {
    segments: Vec<Cubic>,
}

impl Cubic {
    /// Maximum proportion of the socket-to-socket distance used for control points.
    pub(crate) const MAX_CURVATURE_FACTOR: f32 = 0.5;
    /// Default normalized curvature value.
    pub const DEFAULT_CURVATURE: f32 = 0.5;

    /// Construct a cubic curve from the start and end points (and normals) of an edge.
    ///
    /// The normals of the associated input/output are required in order to determine ctrl points.
    ///
    /// `curvature` is a normalized value in the range `0.0..=1.0`.
    /// Internally this maps to a control-point distance factor capping the
    /// strongest curve at half of the total socket-to-socket distance.
    pub fn from_edge_points(
        a: (egui::Pos2, egui::Vec2),
        b: (egui::Pos2, egui::Vec2),
        curvature: f32,
    ) -> Self {
        let (from, a_norm) = a;
        let (to, b_norm) = b;
        let distance = from.distance(to);
        let curvature_factor = curvature.clamp(0.0, 1.0) * Self::MAX_CURVATURE_FACTOR;
        let ctrl_distance = distance * curvature_factor;
        let ctrl1 = from + a_norm * ctrl_distance;
        let ctrl2 = to + b_norm * ctrl_distance;
        Self {
            from,
            ctrl1,
            ctrl2,
            to,
        }
    }

    /// Sample the curve at `t`, where `t` is in the range 0..=1.
    pub fn sample(&self, t: f32) -> egui::Pos2 {
        let t2 = t * t;
        let t3 = t2 * t;
        let one_t = 1.0 - t;
        let one_t2 = one_t * one_t;
        let one_t3 = one_t2 * one_t;
        let v = self.from.to_vec2() * one_t3
            + self.ctrl1.to_vec2() * 3.0 * one_t2 * t
            + self.ctrl2.to_vec2() * 3.0 * one_t * t2
            + self.to.to_vec2() * t3;
        egui::Pos2::new(v.x, v.y)
    }

    /// Flatten the curve into a list of points, ready to draw a polyline.
    ///
    /// Determines the number of points by first calculating the total distance of the path *from
    /// -> ctrl1 -> ctrl2 -> to* and then dividing that distance by the given distance per point.
    ///
    /// **NOTE**: This should probably use a `tolerance`, however this involves calculating
    /// derivatives and is significantly more complicated than just estimating a number of points
    /// based on the distance between each of the points in the curve. This is imperfect, but
    /// hopefully does the job for drawing edges.
    pub fn flatten(self, distance_per_point: f32) -> impl Iterator<Item = egui::Pos2> {
        let distance = self.from.distance(self.ctrl1)
            + self.ctrl1.distance(self.ctrl2)
            + self.ctrl2.distance(self.to);
        let samples = ((distance / distance_per_point).round() as usize).max(1);
        (0..=samples).map(move |ix| {
            let t = ix as f32 / samples as f32;
            self.sample(t)
        })
    }

    /// Find the approximate distance of the given point `p` from the curve.
    ///
    /// **NOTE**: Currently, this does a brute-force search along a `flatten`ned curve. Eventually
    /// we should implement a more efficient approach, however this should be plenty efficient for
    /// simple edge selection (the main use-case for this method).
    pub fn closest_point(self, distance_per_point: f32, target: egui::Pos2) -> egui::Pos2 {
        self.flatten(distance_per_point)
            .fold((self.from, f32::MAX), |closest, p| {
                let dist_sq = p.distance_sq(target);
                if dist_sq < closest.1 {
                    (p, dist_sq)
                } else {
                    closest
                }
            })
            .0
    }

    /// Short-hand for producing the maximum possible bounds for the curve without performing any
    /// interpolation.
    fn max_bounds(&self) -> egui::Rect {
        let mut r = egui::Rect::from_min_max(self.from, self.to);
        r.extend_with(self.ctrl1);
        r.extend_with(self.ctrl2);
        r
    }

    /// Whether or not the curve intersects the given line..
    ///
    /// **NOTE**: Currently, this does a brute-force search along a `flatten`ned curve. Eventually
    /// we should implement a more efficient approach, however this should be plenty efficient for
    /// simple edge selection (the main use-case for this method).
    pub fn intersects_line(self, distance_per_point: f32, line: (egui::Pos2, egui::Pos2)) -> bool {
        if !rect_intersects_line(self.max_bounds(), line) {
            return false;
        }
        let mut pts = self.flatten(distance_per_point).peekable();
        while let Some(b1) = pts.next() {
            if let Some(&b2) = pts.peek() {
                if lines_intersect(line, (b1, b2)) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether or not the curve intersects the given rectangle.
    ///
    /// **NOTE**: Currently, this does a brute-force search along a `flatten`ned curve. Eventually
    /// we should implement a more efficient approach, however this should be plenty efficient for
    /// simple edge selection (the main use-case for this method).
    pub fn intersects_rect(self, distance_per_point: f32, rect: egui::Rect) -> bool {
        if !rect.intersects(self.max_bounds()) {
            return false;
        } else if rect.contains(self.from) || rect.contains(self.to) {
            return true;
        }
        let lt = rect.left_top();
        let lb = rect.left_bottom();
        let rt = rect.right_top();
        let rb = rect.right_bottom();
        let lines = [(lt, rt), (rt, rb), (rb, lb)];
        lines
            .iter()
            .any(|&l| self.intersects_line(distance_per_point, l))
    }
}

impl Path {
    /// Construct a path from an edge's endpoints (and their socket normals)
    /// threaded through the given intermediate waypoints, e.g. a corridor
    /// route produced by the automatic layout.
    ///
    /// The curve leaves each socket along its normal, exactly as in
    /// [`Cubic::from_edge_points`], and passes through each waypoint aligned
    /// with the flow axis (derived from the output socket's normal), keeping
    /// the curve within the corridor the waypoint marks.
    ///
    /// `curvature` is the normalised value described in
    /// [`Cubic::from_edge_points`].
    pub fn from_edge_points_via(
        a: (egui::Pos2, egui::Vec2),
        waypoints: &[egui::Pos2],
        b: (egui::Pos2, egui::Vec2),
        curvature: f32,
    ) -> Self {
        let (from, a_norm) = a;
        let (to, b_norm) = b;

        let mut points = Vec::with_capacity(waypoints.len() + 2);
        points.push(from);
        points.extend(waypoints.iter().copied());
        points.push(to);
        points.dedup();
        if points.len() < 3 {
            let segments = vec![Cubic::from_edge_points(a, b, curvature)];
            return Self { segments };
        }

        // Unit travel directions at every point: along the socket normals at
        // the ends, flow-aligned at the waypoints (the output normal points
        // along the flow axis).
        let flow = a_norm;
        let n = points.len();
        let mut tangents = Vec::with_capacity(n);
        tangents.push(a_norm);
        for i in 1..n - 1 {
            let chord = points[i + 1] - points[i - 1];
            let along = flow * chord.dot(flow);
            let dir = if along.length_sq() > f32::EPSILON {
                along
            } else if chord.length_sq() > f32::EPSILON {
                chord
            } else {
                flow
            };
            tangents.push(dir.normalized());
        }
        tangents.push(-b_norm);

        // One cubic per consecutive pair, sharing tangents at the joints for
        // a C1-continuous curve. Handle lengths scale with each segment's
        // own chord, avoiding overshoot on unevenly spaced waypoints.
        let factor = curvature.clamp(0.0, 1.0) * Cubic::MAX_CURVATURE_FACTOR;
        let segments = (0..n - 1)
            .map(|i| {
                let len = points[i].distance(points[i + 1]) * factor;
                Cubic {
                    from: points[i],
                    ctrl1: points[i] + tangents[i] * len,
                    ctrl2: points[i + 1] - tangents[i + 1] * len,
                    to: points[i + 1],
                }
            })
            .collect();
        Self { segments }
    }

    /// The path's piecewise cubic segments.
    pub fn segments(&self) -> &[Cubic] {
        &self.segments
    }

    /// The bounds of the control points of all segments, which contain the
    /// whole path.
    pub(crate) fn max_bounds(&self) -> egui::Rect {
        self.segments
            .iter()
            .fold(egui::Rect::NOTHING, |r, seg| r.union(seg.max_bounds()))
    }

    /// Flatten the path into a list of points, ready to draw a polyline.
    ///
    /// See [`Cubic::flatten`]. Joint points shared by consecutive segments
    /// are emitted once.
    pub fn flatten(&self, distance_per_point: f32) -> impl Iterator<Item = egui::Pos2> + '_ {
        self.segments.iter().enumerate().flat_map(move |(i, seg)| {
            let skip = if i == 0 { 0 } else { 1 };
            seg.flatten(distance_per_point).skip(skip)
        })
    }

    /// Find the approximate closest point on the path to the given point.
    ///
    /// See [`Cubic::closest_point`].
    pub fn closest_point(&self, distance_per_point: f32, target: egui::Pos2) -> egui::Pos2 {
        self.segments
            .iter()
            .map(|seg| seg.closest_point(distance_per_point, target))
            .min_by(|a, b| a.distance_sq(target).total_cmp(&b.distance_sq(target)))
            .unwrap_or(egui::Pos2::ZERO)
    }

    /// Whether or not the path intersects the given rectangle.
    ///
    /// See [`Cubic::intersects_rect`].
    pub fn intersects_rect(&self, distance_per_point: f32, rect: egui::Rect) -> bool {
        self.segments
            .iter()
            .any(|seg| seg.intersects_rect(distance_per_point, rect))
    }
}

// True if any of the area of the rect intersects the line.
fn rect_intersects_line(r: egui::Rect, (a, b): (egui::Pos2, egui::Pos2)) -> bool {
    if !r.intersects(egui::Rect::from_two_pos(a, b)) {
        return false;
    } else if r.contains(a) || r.contains(b) {
        return true;
    }
    let lt = r.left_top();
    let lb = r.left_bottom();
    let rt = r.right_top();
    let rb = r.right_bottom();
    lines_intersect((lt, rt), (a, b))
        || lines_intersect((rt, rb), (a, b))
        || lines_intersect((rb, lb), (a, b))
}

// Whether or not the given lines intersect.
fn lines_intersect(a: (egui::Pos2, egui::Pos2), b: (egui::Pos2, egui::Pos2)) -> bool {
    let (a1, a2) = a;
    let (b1, b2) = b;
    fn tri_area(a: egui::Pos2, b: egui::Pos2, c: egui::Pos2) -> f32 {
        (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y)
    }
    let t1 = tri_area(a1, a2, b1);
    let t2 = tri_area(a1, a2, b2);
    let res1 = (t1 > 0.0) != (t2 > 0.0) && !(t1 == 0.0 && t2 == 0.0);
    let t1 = tri_area(b1, b2, a1);
    let t2 = tri_area(b1, b2, a2);
    let res2 = (t1 > 0.0) != (t2 > 0.0) && !(t1 == 0.0 && t2 == 0.0);
    res1 && res2
}

#[cfg(test)]
mod tests {
    use super::{Cubic, Path};

    #[test]
    fn no_waypoints_matches_cubic() {
        let a = (egui::pos2(0.0, 0.0), egui::vec2(1.0, 0.0));
        let b = (egui::pos2(100.0, 50.0), egui::vec2(-1.0, 0.0));
        let path = Path::from_edge_points_via(a, &[], b, 0.5);
        let cubic = Cubic::from_edge_points(a, b, 0.5);
        assert_eq!(path.segments().len(), 1);
        let seg = path.segments()[0];
        assert_eq!(seg.from, cubic.from);
        assert_eq!(seg.ctrl1, cubic.ctrl1);
        assert_eq!(seg.ctrl2, cubic.ctrl2);
        assert_eq!(seg.to, cubic.to);
    }

    #[test]
    fn path_passes_through_waypoints() {
        let a = (egui::pos2(0.0, 0.0), egui::vec2(1.0, 0.0));
        let b = (egui::pos2(200.0, 0.0), egui::vec2(-1.0, 0.0));
        let waypoints = [egui::pos2(100.0, 60.0)];
        let path = Path::from_edge_points_via(a, &waypoints, b, 0.5);
        assert_eq!(path.segments().len(), 2);
        // The joint sits exactly on the waypoint.
        assert_eq!(path.segments()[0].to, waypoints[0]);
        assert_eq!(path.segments()[1].from, waypoints[0]);
    }

    #[test]
    fn joints_are_smooth() {
        // C1 continuity: the handles either side of a joint are collinear.
        let a = (egui::pos2(0.0, 0.0), egui::vec2(1.0, 0.0));
        let b = (egui::pos2(300.0, 40.0), egui::vec2(-1.0, 0.0));
        let waypoints = [egui::pos2(120.0, 80.0), egui::pos2(210.0, -30.0)];
        let path = Path::from_edge_points_via(a, &waypoints, b, 0.5);
        for pair in path.segments().windows(2) {
            let joint = pair[0].to;
            let into = (joint - pair[0].ctrl2).normalized();
            let out = (pair[1].ctrl1 - joint).normalized();
            assert!((into - out).length() < 1e-4, "{into:?} vs {out:?}");
        }
    }

    #[test]
    fn waypoint_tangents_are_flow_aligned() {
        // Flow along +x: the handles at each waypoint stay horizontal, so the
        // curve runs parallel to the corridor through the node band.
        let a = (egui::pos2(0.0, 0.0), egui::vec2(1.0, 0.0));
        let b = (egui::pos2(300.0, 0.0), egui::vec2(-1.0, 0.0));
        let waypoints = [egui::pos2(150.0, 100.0)];
        let path = Path::from_edge_points_via(a, &waypoints, b, 0.5);
        assert_eq!(path.segments()[0].ctrl2.y, waypoints[0].y);
        assert_eq!(path.segments()[1].ctrl1.y, waypoints[0].y);
    }

    #[test]
    fn degenerate_waypoints_collapse_to_direct_curve() {
        let a = (egui::pos2(0.0, 0.0), egui::vec2(1.0, 0.0));
        let b = (egui::pos2(100.0, 0.0), egui::vec2(-1.0, 0.0));
        // Waypoints coinciding with the endpoints are dropped.
        let waypoints = [egui::pos2(0.0, 0.0), egui::pos2(100.0, 0.0)];
        let path = Path::from_edge_points_via(a, &waypoints, b, 0.5);
        assert_eq!(path.segments().len(), 1);
        assert!(path
            .flatten(5.0)
            .all(|p| p.x.is_finite() && p.y.is_finite()));
    }
}

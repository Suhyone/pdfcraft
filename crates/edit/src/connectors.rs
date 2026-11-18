//! Callouts: a leader line that runs from a box (a text frame drawn as a closed path) to an arrowhead.
//!
//! The pieces are ordinary painted paths of the page. They are bound by touching: the leader's
//! tail lies on the frame's edge and the arrowhead (a small closed or filled path, or small barb
//! strokes) touches the leader's other end. Nothing here reads or writes the page; it works on the
//! paths [`crate::vector_paths_in_region`] returned and gives the groups as indices into them.

use crate::vectors::{PathSeg, VectorPath};

/// Points (page user space) within which two path points count as touching.
pub(crate) const JOIN_TOL: f64 = 1.0;
/// How far from a frame's edge a leader's tail may lie and still be attached to it.
const EDGE_TOL: f64 = 2.0;
/// The longest side (points) of the bounding box of an arrowhead.
pub(crate) const ARROW_MAX: f64 = 30.0;

/// A leader line with the box it leaves and the arrowhead it ends in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Connector {
    /// The leader's index in the path list.
    pub leader: usize,
    /// The arrowhead paths (barbs or a polygon) at the leader's head end.
    pub heads: Vec<usize>,
    /// The frame the tail lies on.
    pub frame: Option<usize>,
    /// Whether the leader's first point is its tail (the other end carries the arrowhead).
    pub tail_first: bool,
}

/// The points a path passes through (curve control points left out).
pub(crate) fn points_of(path: &VectorPath) -> Vec<[f64; 2]> {
    path.segments
        .iter()
        .filter_map(|s| match s {
            PathSeg::MoveTo(p) | PathSeg::LineTo(p) | PathSeg::CurveTo(_, _, p) => Some(*p),
            PathSeg::Close => None,
        })
        .collect()
}

fn lines_only(path: &VectorPath) -> bool {
    path.segments.iter().all(|s| !matches!(s, PathSeg::CurveTo(..)))
}

fn size_of(path: &VectorPath) -> f64 {
    (path.bbox[2] - path.bbox[0]).max(path.bbox[3] - path.bbox[1])
}

/// An open stroked polyline of straight pieces: what a leader line is drawn as.
pub(crate) fn is_polyline(path: &VectorPath) -> bool {
    path.paint_op == "S" && !path.closed && lines_only(path) && points_of(path).len() >= 2
}

/// A closed shape of straight edges with at least three corners, such as a text frame or a
/// filled arrow polygon.
fn is_closed_shape(path: &VectorPath) -> bool {
    path.closed && lines_only(path) && points_of(path).len() >= 3
}

pub(crate) fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= tol
}

/// Distance from `p` to the segment `a`-`b`.
pub(crate) fn distance_to_edge(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
    let len2 = abx * abx + aby * aby;
    if len2 <= f64::EPSILON {
        return (p[0] - a[0]).hypot(p[1] - a[1]);
    }
    let t = (((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / len2).clamp(0.0, 1.0);
    (p[0] - (a[0] + abx * t)).hypot(p[1] - (a[1] + aby * t))
}

/// The edges of a closed shape, the closing one included.
pub fn edges_of(path: &VectorPath) -> Vec<([f64; 2], [f64; 2])> {
    let pts = points_of(path);
    let mut out: Vec<([f64; 2], [f64; 2])> = pts.windows(2).map(|w| (w[0], w[1])).collect();
    if let (Some(first), Some(last)) = (pts.first(), pts.last())
        && path.closed
    {
        out.push((*last, *first));
    }
    out
}

/// The point of `frame`'s outline nearest to `p`.
pub fn nearest_on_outline(frame: &VectorPath, p: [f64; 2]) -> Option<[f64; 2]> {
    let mut best: Option<(f64, [f64; 2])> = None;
    for (a, b) in edges_of(frame) {
        let (abx, aby) = (b[0] - a[0], b[1] - a[1]);
        let len2 = abx * abx + aby * aby;
        let t = if len2 <= f64::EPSILON { 0.0 } else { (((p[0] - a[0]) * abx + (p[1] - a[1]) * aby) / len2).clamp(0.0, 1.0) };
        let q = [a[0] + abx * t, a[1] + aby * t];
        let d = (p[0] - q[0]).hypot(p[1] - q[1]);
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, q));
        }
    }
    best.map(|(_, q)| q)
}

fn on_outline(frame: &VectorPath, p: [f64; 2]) -> bool {
    edges_of(frame).into_iter().any(|(a, b)| distance_to_edge(p, a, b) <= EDGE_TOL)
}

/// A path that can be an arrowhead: small, and a closed or filled shape, or a short open stroke
/// (a barb).
pub(crate) fn is_arrowhead(path: &VectorPath) -> bool {
    size_of(path) <= ARROW_MAX && path.segments.len() <= 12 && (path.closed || path.paint_op != "S" || is_polyline(path))
}

/// The ends of the open polylines that touch nothing else of the list: where a leader may go on
/// beyond what was picked up.
pub(crate) fn dangling_ends(paths: &[VectorPath]) -> Vec<([f64; 2], usize)> {
    let mut out = Vec::new();
    for (i, path) in paths.iter().enumerate() {
        if !is_polyline(path) || size_of(path) <= ARROW_MAX {
            continue;
        }
        let pts = points_of(path);
        for end in [pts.first(), pts.last()].into_iter().flatten() {
            let joined = paths.iter().enumerate().any(|(j, other)| j != i && points_of(other).iter().any(|q| near(*q, *end, JOIN_TOL)));
            let on_frame = paths.iter().enumerate().any(|(j, other)| j != i && is_closed_shape(other) && on_outline(other, *end));
            if !joined && !on_frame {
                out.push((*end, i));
            }
        }
    }
    out
}

/// The callouts among `paths`: each long open polyline that has an arrowhead at one end and/or lies
/// with one end on the edge of a closed shape.
pub fn connectors(paths: &[VectorPath]) -> Vec<Connector> {
    let mut out = Vec::new();
    for (li, leader) in paths.iter().enumerate() {
        if !is_polyline(leader) || size_of(leader) <= ARROW_MAX {
            continue;
        }
        let pts = points_of(leader);
        let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) else { continue };
        let heads_at = |end: [f64; 2]| -> Vec<usize> {
            paths
                .iter()
                .enumerate()
                .filter(|(j, p)| *j != li && is_arrowhead(p) && points_of(p).iter().any(|q| near(*q, end, JOIN_TOL)))
                .map(|(j, _)| j)
                .collect()
        };
        let frame_at = |end: [f64; 2]| -> Option<usize> {
            paths.iter().enumerate().find(|(j, p)| *j != li && is_closed_shape(p) && size_of(p) > ARROW_MAX && on_outline(p, end)).map(|(j, _)| j)
        };
        let (head_first, head_last) = (heads_at(first), heads_at(last));
        let (frame_first, frame_last) = (frame_at(first), frame_at(last));
        // The head is the end carrying an arrowhead; the tail is the other one. Without any
        // arrowhead, the end on a frame is the tail.
        let tail_first = match (head_first.is_empty(), head_last.is_empty()) {
            (false, true) => false,
            (true, false) => true,
            (true, true) => match (frame_first.is_some(), frame_last.is_some()) {
                (true, false) => true,
                (false, true) => false,
                _ => continue,
            },
            (false, false) => continue,
        };
        let (heads, frame) = if tail_first { (head_last, frame_first) } else { (head_first, frame_last) };
        if heads.is_empty() && frame.is_none() {
            continue;
        }
        out.push(Connector { leader: li, heads, frame, tail_first });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(id: usize, op: &str, segments: Vec<PathSeg>, closed: bool) -> VectorPath {
        let pts: Vec<[f64; 2]> = segments
            .iter()
            .filter_map(|s| match s {
                PathSeg::MoveTo(p) | PathSeg::LineTo(p) => Some(*p),
                _ => None,
            })
            .collect();
        let bbox = pts.iter().fold([f64::MAX, f64::MAX, f64::MIN, f64::MIN], |b, p| [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])]);
        VectorPath {
            id,
            span: [0, 1],
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            paint_op: op.into(),
            segments,
            bbox,
            stroke_width: 1.0,
            color: [0.0; 3],
            closed,
        }
    }

    fn rect(id: usize, x0: f64, y0: f64, x1: f64, y1: f64) -> VectorPath {
        let s = vec![PathSeg::MoveTo([x0, y0]), PathSeg::LineTo([x1, y0]), PathSeg::LineTo([x1, y1]), PathSeg::LineTo([x0, y1]), PathSeg::Close];
        path(id, "S", s, true)
    }

    fn callout() -> Vec<VectorPath> {
        vec![
            rect(0, 0.0, 0.0, 60.0, 30.0),
            path(1, "S", vec![PathSeg::MoveTo([60.0, 15.0]), PathSeg::LineTo([160.0, 95.0])], false),
            path(2, "f", vec![PathSeg::MoveTo([160.0, 95.0]), PathSeg::LineTo([150.0, 93.0]), PathSeg::LineTo([156.0, 85.0]), PathSeg::Close], true),
        ]
    }

    #[test]
    fn a_leader_binds_its_frame_and_arrowhead() {
        let c = connectors(&callout());
        assert_eq!(c.len(), 1);
        assert_eq!(c[0], Connector { leader: 1, heads: vec![2], frame: Some(0), tail_first: true });
    }

    #[test]
    fn the_tail_end_is_found_whichever_way_the_leader_is_drawn() {
        let mut paths = callout();
        paths[1] = path(1, "S", vec![PathSeg::MoveTo([160.0, 95.0]), PathSeg::LineTo([60.0, 15.0])], false);
        let c = connectors(&paths);
        assert_eq!(c.len(), 1);
        assert!(!c[0].tail_first, "the arrowhead is at the first point");
    }

    #[test]
    fn a_plain_line_far_from_any_box_is_not_a_connector() {
        let paths = vec![path(0, "S", vec![PathSeg::MoveTo([0.0, 0.0]), PathSeg::LineTo([100.0, 0.0])], false)];
        assert!(connectors(&paths).is_empty());
    }

    #[test]
    fn dangling_ends_leave_out_the_end_on_the_frame() {
        let ends = dangling_ends(&callout()[..2]);
        assert_eq!(ends, vec![([160.0, 95.0], 1)]);
    }

    #[test]
    fn the_nearest_point_of_the_outline_is_on_an_edge() {
        let frame = rect(0, 0.0, 0.0, 60.0, 30.0);
        assert_eq!(nearest_on_outline(&frame, [80.0, 15.0]), Some([60.0, 15.0]));
        assert_eq!(nearest_on_outline(&frame, [30.0, 100.0]), Some([30.0, 30.0]));
    }
}

//! The geometry of moving and bending callouts (a leader line from a text frame to an arrowhead),
//! on the copies of the paths the vector tool holds. Nothing here reads input or the document.
//!
//! A callout is either straight (the leader has two points) or bent (it has more). Moving the
//! frame of a straight one re-aims the tail at the nearest point of the frame's outline and leaves
//! the head where it points; a bent one keeps its bends and head, and its tail travels with the
//! frame. Either way the arrowhead is turned about its tip to follow the leader's last piece.

use pdfcraft_engine::{Connector, PathSeg, VectorPatch, VectorPath, nearest_on_outline};

/// The paths a callout is made of, as they were before an edit began: (index in the list, path).
pub(crate) type Snapshot = Vec<(usize, VectorPath)>;

/// The points a polyline passes through.
pub(crate) fn points(path: &VectorPath) -> Vec<[f64; 2]> {
    path.segments
        .iter()
        .filter_map(|s| match s {
            PathSeg::MoveTo(p) | PathSeg::LineTo(p) | PathSeg::CurveTo(_, _, p) => Some(*p),
            PathSeg::Close => None,
        })
        .collect()
}

fn polyline(pts: &[[f64; 2]]) -> Vec<PathSeg> {
    pts.iter().enumerate().map(|(i, p)| if i == 0 { PathSeg::MoveTo(*p) } else { PathSeg::LineTo(*p) }).collect()
}

/// A callout whose leader has more than two points.
pub(crate) fn is_bent(paths: &[VectorPath], c: &Connector) -> bool {
    paths.get(c.leader).is_some_and(|p| points(p).len() > 2)
}

/// Keep the paths of `connectors` as they are now; the frame too when `with_frame`.
pub(crate) fn snapshot(paths: &[VectorPath], connectors: &[Connector], with_frame: bool) -> Snapshot {
    let mut want: Vec<usize> = Vec::new();
    for c in connectors {
        want.push(c.leader);
        want.extend(c.heads.iter().copied());
        if with_frame && let Some(f) = c.frame {
            want.push(f);
        }
    }
    want.sort_unstable();
    want.dedup();
    want.into_iter().filter_map(|i| paths.get(i).map(|p| (i, p.clone()))).collect()
}

fn original(snap: &Snapshot, index: usize) -> Option<&VectorPath> {
    snap.iter().find(|(i, _)| *i == index).map(|(_, p)| p)
}

fn refresh_bbox(path: &mut VectorPath) {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut add = |p: &[f64; 2]| {
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    };
    for s in &path.segments {
        match s {
            PathSeg::MoveTo(p) | PathSeg::LineTo(p) => add(p),
            PathSeg::CurveTo(a, c, d) => {
                add(a);
                add(c);
                add(d);
            }
            PathSeg::Close => {}
        }
    }
    if b[0] <= b[2] && b[1] <= b[3] {
        path.bbox = b;
    }
}

fn map_points(segments: &[PathSeg], f: impl Fn([f64; 2]) -> [f64; 2]) -> Vec<PathSeg> {
    segments
        .iter()
        .map(|s| match s {
            PathSeg::MoveTo(p) => PathSeg::MoveTo(f(*p)),
            PathSeg::LineTo(p) => PathSeg::LineTo(f(*p)),
            PathSeg::CurveTo(a, b, c) => PathSeg::CurveTo(f(*a), f(*b), f(*c)),
            PathSeg::Close => PathSeg::Close,
        })
        .collect()
}

/// The arrow's tip (the leader's head end) and the direction its last piece points in.
fn tip_and_angle(pts: &[[f64; 2]], tail_first: bool) -> Option<([f64; 2], f64)> {
    let (tip, prev) = if tail_first { (pts.last()?, pts.get(pts.len().checked_sub(2)?)?) } else { (pts.first()?, pts.get(1)?) };
    let (dx, dy) = (tip[0] - prev[0], tip[1] - prev[1]);
    (dx.hypot(dy) > 1e-9).then(|| (*tip, dy.atan2(dx)))
}

/// Turn and carry the arrowhead of `c` (as the snapshot has it) so it sits on the leader's head
/// end as the leader is now.
pub(crate) fn follow_head(paths: &mut [VectorPath], snap: &Snapshot, c: &Connector) {
    let Some(old) = original(snap, c.leader) else { return };
    let Some(new) = paths.get(c.leader) else { return };
    let (Some((t0, a0)), Some((t1, a1))) = (tip_and_angle(&points(old), c.tail_first), tip_and_angle(&points(new), c.tail_first)) else { return };
    let (sin, cos) = (a1 - a0).sin_cos();
    let carry = |p: [f64; 2]| {
        let (x, y) = (p[0] - t0[0], p[1] - t0[1]);
        [t1[0] + x * cos - y * sin, t1[1] + x * sin + y * cos]
    };
    for h in &c.heads {
        let (Some(orig), Some(head)) = (original(snap, *h), paths.get_mut(*h)) else { continue };
        head.segments = map_points(&orig.segments, carry);
        refresh_bbox(head);
    }
}

/// Move the frame of `c` by `delta` from where the snapshot has it, and bring the leader along:
/// a straight leader re-aims its tail at the frame's nearest edge point (the head stays), a bent
/// one keeps its bends and moves its tail by the same amount.
pub(crate) fn move_frame(paths: &mut [VectorPath], snap: &Snapshot, c: &Connector, delta: [f64; 2]) {
    let (Some(fi), Some(old_leader)) = (c.frame, original(snap, c.leader)) else { return };
    let Some(frame0) = original(snap, fi) else { return };
    if let Some(frame) = paths.get_mut(fi) {
        frame.segments = map_points(&frame0.segments, |p| [p[0] + delta[0], p[1] + delta[1]]);
        refresh_bbox(frame);
    }
    let mut pts = points(old_leader);
    let (Some(tail_i), Some(head_i)) =
        (if c.tail_first { Some(0) } else { pts.len().checked_sub(1) }, if c.tail_first { pts.len().checked_sub(1) } else { Some(0) })
    else {
        return;
    };
    let (Some(tail), Some(head)) = (pts.get(tail_i).copied(), pts.get(head_i).copied()) else { return };
    let moved_tail = if pts.len() == 2 {
        paths.get(fi).and_then(|f| nearest_on_outline(f, head)).unwrap_or([tail[0] + delta[0], tail[1] + delta[1]])
    } else {
        [tail[0] + delta[0], tail[1] + delta[1]]
    };
    if let Some(slot) = pts.get_mut(tail_i) {
        *slot = moved_tail;
    }
    if let Some(leader) = paths.get_mut(c.leader) {
        leader.segments = polyline(&pts);
        refresh_bbox(leader);
    }
    follow_head(paths, snap, c);
}

/// Take every bend out of the leader of `c`: a straight line from its first point to its last.
pub(crate) fn straighten(paths: &mut [VectorPath], snap: &Snapshot, c: &Connector) {
    let Some(leader) = paths.get_mut(c.leader) else { return };
    let pts = points(leader);
    let (Some(first), Some(last)) = (pts.first().copied(), pts.last().copied()) else { return };
    leader.segments = polyline(&[first, last]);
    refresh_bbox(leader);
    follow_head(paths, snap, c);
}

/// Put a new corner on the middle of the piece that ends at segment `seg` (a line), returning where
/// it is. The corner is sharp: the path stays a polyline.
pub(crate) fn insert_corner(path: &mut VectorPath, seg: usize) -> Option<[f64; 2]> {
    let end = match path.segments.get(seg)? {
        PathSeg::LineTo(p) => *p,
        _ => return None,
    };
    let start = match path.segments.get(seg.checked_sub(1)?)? {
        PathSeg::MoveTo(p) | PathSeg::LineTo(p) => *p,
        _ => return None,
    };
    let mid = [(start[0] + end[0]) / 2.0, (start[1] + end[1]) / 2.0];
    path.segments.insert(seg, PathSeg::LineTo(mid));
    Some(mid)
}

/// The paths of the snapshot that are no longer as it had them, as patches for one edit.
pub(crate) fn patches(paths: &[VectorPath], snap: &Snapshot) -> Vec<VectorPatch> {
    snap.iter()
        .filter_map(|(i, orig)| {
            let now = paths.get(*i)?;
            (now.segments != orig.segments).then(|| VectorPatch { id: now.id, segments: now.segments.clone(), stroke_width: None, color: None })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(id: usize, op: &str, segments: Vec<PathSeg>, closed: bool) -> VectorPath {
        let mut p = VectorPath {
            id,
            span: [0, 1],
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            paint_op: op.into(),
            segments,
            bbox: [0.0; 4],
            stroke_width: 1.0,
            color: [0.0; 3],
            closed,
        };
        refresh_bbox(&mut p);
        p
    }

    /// A 60×30 frame at the origin, a leader from its right edge to (160, 95), a head there.
    fn callout() -> (Vec<VectorPath>, Connector) {
        let frame = vec![
            PathSeg::MoveTo([0.0, 0.0]),
            PathSeg::LineTo([60.0, 0.0]),
            PathSeg::LineTo([60.0, 30.0]),
            PathSeg::LineTo([0.0, 30.0]),
            PathSeg::Close,
        ];
        let leader = vec![PathSeg::MoveTo([60.0, 15.0]), PathSeg::LineTo([160.0, 15.0])];
        let head = vec![PathSeg::MoveTo([160.0, 15.0]), PathSeg::LineTo([150.0, 18.0]), PathSeg::LineTo([150.0, 12.0]), PathSeg::Close];
        let paths = vec![path(0, "S", frame, true), path(1, "S", leader, false), path(2, "f", head, true)];
        (paths, Connector { leader: 1, heads: vec![2], frame: Some(0), tail_first: true })
    }

    #[test]
    fn a_straight_leader_re_aims_its_tail_at_the_nearest_edge_and_the_head_stays() {
        let (mut paths, c) = callout();
        let snap = snapshot(&paths, std::slice::from_ref(&c), true);
        move_frame(&mut paths, &snap, &c, [0.0, 60.0]);
        let leader = points(&paths[1]);
        assert_eq!(leader[1], [160.0, 15.0], "the head's target does not move");
        assert_eq!(leader[0][0], 60.0, "the tail is on the frame's right edge");
        assert_eq!(leader[0], [60.0, 60.0], "the frame's nearest corner to the head");
        let tip = points(&paths[2])[0];
        assert!((tip[0] - 160.0).abs() < 1e-9 && (tip[1] - 15.0).abs() < 1e-9, "the arrowhead tip stays at the target: {tip:?}");
    }

    #[test]
    fn a_bent_leader_keeps_its_bend_and_its_tail_travels_with_the_frame() {
        let (mut paths, c) = callout();
        paths[1].segments = polyline(&[[60.0, 15.0], [110.0, 80.0], [160.0, 15.0]]);
        let snap = snapshot(&paths, std::slice::from_ref(&c), true);
        assert!(is_bent(&paths, &c));
        move_frame(&mut paths, &snap, &c, [10.0, 20.0]);
        assert_eq!(points(&paths[1]), vec![[70.0, 35.0], [110.0, 80.0], [160.0, 15.0]]);
        assert_eq!(points(&paths[0])[0], [10.0, 20.0], "the frame moved by the delta");
    }

    #[test]
    fn straightening_removes_every_bend_and_turns_the_head_to_the_new_line() {
        let (mut paths, c) = callout();
        paths[1].segments = polyline(&[[60.0, 15.0], [110.0, 80.0], [160.0, 15.0]]);
        let snap = snapshot(&paths, std::slice::from_ref(&c), false);
        straighten(&mut paths, &snap, &c);
        assert_eq!(points(&paths[1]), vec![[60.0, 15.0], [160.0, 15.0]]);
        assert!(!is_bent(&paths, &c));
        let tip = points(&paths[2])[0];
        assert!((tip[0] - 160.0).abs() < 1e-9 && (tip[1] - 15.0).abs() < 1e-9, "the tip stays on the target: {tip:?}");
        assert!(patches(&paths, &snap).len() == 2, "the leader and the head changed, nothing else");
    }

    #[test]
    fn a_new_corner_is_a_sharp_vertex_in_the_middle_of_the_piece() {
        let (mut paths, _) = callout();
        let at = insert_corner(&mut paths[1], 1);
        assert_eq!(at, Some([110.0, 15.0]));
        assert_eq!(points(&paths[1]).len(), 3);
        assert!(paths[1].segments.iter().all(|s| !matches!(s, PathSeg::CurveTo(..))), "no curve is made");
        assert_eq!(insert_corner(&mut paths[1], 0), None, "a move has no piece before it");
    }
}

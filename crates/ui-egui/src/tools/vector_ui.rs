//! Edit ▸ Edit vector lines: drag a rectangle over a page to pick up the line art inside it, then
//! drag its points (and curve handles) to reshape it, or press Delete to remove the selected path.
//!
//! The paths come from `Document::vector_paths_in_region` (page user space, bottom-up). While a
//! point is dragged only the copy held here changes; releasing it commits one
//! `Edit::EditVectors`, so one Undo takes the whole drag back. After every edit, undo or redo
//! the paths are read again ([`refresh`]), so what is shown is what the page draws.

use egui::{Color32, Pos2, Rect, Shape, Stroke};
use pdfcraft_engine::{Edit, PathSeg, VectorEdits, VectorPatch, VectorPath};
use pdfcraft_render::DocInfo;

use crate::canvas::{DocView, PageXform};
use crate::page_cursor::Layer;
use crate::page_keys::PageKeys;
use crate::page_pointer::PagePointer;
use crate::page_router::{Hit, Owner, PagePress, VectorHandle};

/// How close (screen points) the pointer must be to pick up a point.
const GRAB: f32 = 9.0;
/// A dragged point within this many screen points of another one lands on it.
const SNAP: f32 = 6.0;
/// Points sampled along a curve to draw and to pick it.
const CURVE_STEPS: usize = 16;
/// How close (screen points) a press must be to a path to select it.
const PICK: f32 = 5.0;
const BLUE: Color32 = Color32::from_rgb(0x14, 0x73, 0xE6);

/// The paths picked up on one page, and the point being dragged.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorEditState {
    pub page: usize,
    /// The rectangle that was dragged, `[x0, y0, x1, y1]` in page user space.
    pub region: [f64; 4],
    pub paths: Vec<VectorPath>,
    /// The document's edit generation these paths were read at.
    pub generation: u64,
    /// The selected path (an index into `paths`).
    pub selected: Option<usize>,
    /// The point being dragged: path index, segment index and which of its points.
    pub(crate) grab: Option<(usize, usize, VectorHandle)>,
}

/// The point of a segment a handle names.
fn point_of(seg: &PathSeg, handle: VectorHandle) -> Option<[f64; 2]> {
    match (seg, handle) {
        (PathSeg::MoveTo(p) | PathSeg::LineTo(p), VectorHandle::Endpoint) => Some(*p),
        (PathSeg::CurveTo(_, _, p), VectorHandle::Endpoint) => Some(*p),
        (PathSeg::CurveTo(a, _, _), VectorHandle::Ctrl1) => Some(*a),
        (PathSeg::CurveTo(_, b, _), VectorHandle::Ctrl2) => Some(*b),
        _ => None,
    }
}

fn set_point(seg: &mut PathSeg, handle: VectorHandle, to: [f64; 2]) {
    match (seg, handle) {
        (PathSeg::MoveTo(p) | PathSeg::LineTo(p), VectorHandle::Endpoint) => *p = to,
        (PathSeg::CurveTo(_, _, p), VectorHandle::Endpoint) => *p = to,
        (PathSeg::CurveTo(a, _, _), VectorHandle::Ctrl1) => *a = to,
        (PathSeg::CurveTo(_, b, _), VectorHandle::Ctrl2) => *b = to,
        _ => {}
    }
}

/// Every draggable point of a path: (segment, which point, where).
fn handles(path: &VectorPath) -> Vec<(usize, VectorHandle, [f64; 2])> {
    let mut out = Vec::new();
    for (i, seg) in path.segments.iter().enumerate() {
        for h in [VectorHandle::Ctrl1, VectorHandle::Ctrl2, VectorHandle::Endpoint] {
            if let Some(p) = point_of(seg, h) {
                out.push((i, h, p));
            }
        }
    }
    out
}

fn to_screen(xf: &PageXform, info: &DocInfo, page: usize, p: [f64; 2]) -> Pos2 {
    crate::edit_text_ui::user_to_screen(xf, info, page, p[0], p[1])
}

fn to_user(xf: &PageXform, info: &DocInfo, page: usize, at: Pos2) -> Option<[f64; 2]> {
    let (vx, vy) = xf.screen_to_view(at);
    let u = info.pages.get(page)?.view_to_user(vx, vy);
    let p = [f64::from(u[0]), f64::from(u[1])];
    (p[0].is_finite() && p[1].is_finite()).then_some(p)
}

/// The path drawn as screen points: curves sampled.
fn outline(xf: &PageXform, info: &DocInfo, page: usize, path: &VectorPath) -> Vec<Vec<Pos2>> {
    let mut runs: Vec<Vec<Pos2>> = Vec::new();
    let mut cur: Option<[f64; 2]> = None;
    let mut start: Option<[f64; 2]> = None;
    for seg in &path.segments {
        match seg {
            PathSeg::MoveTo(p) => {
                runs.push(vec![to_screen(xf, info, page, *p)]);
                cur = Some(*p);
                start = Some(*p);
            }
            PathSeg::LineTo(p) => {
                if let Some(run) = runs.last_mut() {
                    run.push(to_screen(xf, info, page, *p));
                }
                cur = Some(*p);
            }
            PathSeg::CurveTo(a, b, c) => {
                if let (Some(run), Some(p0)) = (runs.last_mut(), cur) {
                    for i in 1..=CURVE_STEPS {
                        let t = i as f64 / CURVE_STEPS as f64;
                        let u = 1.0 - t;
                        let f = |k: usize| u * u * u * p0[k] + 3.0 * u * u * t * a[k] + 3.0 * u * t * t * b[k] + t * t * t * c[k];
                        run.push(to_screen(xf, info, page, [f(0), f(1)]));
                    }
                }
                cur = Some(*c);
            }
            PathSeg::Close => {
                if let (Some(run), Some(s)) = (runs.last_mut(), start) {
                    run.push(to_screen(xf, info, page, s));
                }
                cur = start;
            }
        }
    }
    runs
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= f32::EPSILON {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The point of the picked-up paths under `p` on `page`: the hit test of this tool, for the
/// cursor and for the press. The nearest point within reach; the selected path's points win ties
/// with the others.
pub(crate) fn hit(xf: &PageXform, info: &DocInfo, page: usize, view: &DocView, p: Pos2) -> Option<Hit> {
    let state = view.vector_edit.as_ref().filter(|s| s.page == page)?;
    if !xf.rect.contains(p) {
        return None;
    }
    let mut best: Option<(f32, usize, usize, VectorHandle)> = None;
    for (pi, path) in state.paths.iter().enumerate() {
        let bonus = if state.selected == Some(pi) { 0.5 } else { 0.0 };
        for (si, h, pt) in handles(path) {
            let d = to_screen(xf, info, page, pt).distance(p);
            if d <= GRAB && best.is_none_or(|b| d - bonus < b.0) {
                best = Some((d - bonus, pi, si, h));
            }
        }
    }
    let (_, path, seg, handle) = best?;
    Some(Hit::new(Owner::VectorNode { path, seg, handle }, Layer::Handle, egui::CursorIcon::Grab))
}

/// The path nearest to `p` within [`PICK`] screen points, to select it by a click.
fn path_at(xf: &PageXform, info: &DocInfo, page: usize, state: &VectorEditState, p: Pos2) -> Option<usize> {
    state
        .paths
        .iter()
        .enumerate()
        .filter_map(|(i, path)| {
            let d = outline(xf, info, page, path)
                .iter()
                .flat_map(|run| run.windows(2).map(|w| distance_to_segment(p, w[0], w[1])))
                .fold(f32::INFINITY, f32::min);
            (d <= PICK).then_some((i, d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// Pull `to` onto 0°, 45° or 90° from `from` (Shift).
fn snap_angle(from: [f64; 2], to: [f64; 2]) -> [f64; 2] {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let len = dx.hypot(dy);
    if len <= f64::EPSILON {
        return to;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let angle = (dy.atan2(dx) / step).round() * step;
    [from[0] + len * angle.cos(), from[1] + len * angle.sin()]
}

/// The page being worked on: where it is on screen, what is known of it, and its index.
#[derive(Clone, Copy)]
pub(crate) struct PageRef<'a> {
    pub xf: &'a PageXform,
    pub info: &'a DocInfo,
    pub page: usize,
}

/// Dragging: a point's own press ([`hit`]) picks it up; a click on a path selects it.
/// A press nobody holds is the region marquee's (`zoom_snap::page_input`).
pub(crate) fn page_input(resp: &egui::Response, ptr: &PagePointer, at: PageRef<'_>, view: &mut DocView, allowed: bool, press: PagePress) {
    let PageRef { xf, info, page } = at;
    let pointer = ptr.pos;
    if !allowed {
        if pointer.is_some_and(|p| xf.rect.contains(p)) {
            view.cursor.claim(Layer::ToolArea, egui::CursorIcon::NotAllowed);
        }
        return;
    }
    if ptr.drag_started
        && let PagePress::Mine(Owner::VectorNode { path, seg, handle }) = press
        && let Some(state) = view.vector_edit.as_mut().filter(|s| s.page == page)
    {
        state.selected = Some(path);
        state.grab = Some((path, seg, handle));
    }
    // A click on a path (not on one of its points) selects it, on nothing clears the selection.
    if resp.clicked()
        && press == PagePress::Free
        && let Some(p) = ptr.released.filter(|p| xf.rect.contains(*p))
        && let Some(state) = view.vector_edit.as_mut().filter(|s| s.page == page)
    {
        state.selected = path_at(xf, info, page, state, p);
    }
    let Some(state) = view.vector_edit.as_mut().filter(|s| s.page == page) else { return };
    let Some((pi, si, handle)) = state.grab else { return };
    view.cursor.claim(Layer::Drag, egui::CursorIcon::Grabbing);
    if let Some(at) = pointer
        && let Some(mut target) = to_user(xf, info, page, at)
    {
        let shift = ptr.modifiers.shift;
        // Endpoint snapping to another point of the picked-up paths within a few screen points.
        let snapped = state
            .paths
            .iter()
            .enumerate()
            .flat_map(|(i, p)| handles(p).into_iter().filter(move |(s, h, _)| !(i == pi && *s == si && *h == handle)).map(|(_, _, pt)| pt))
            .map(|pt| (to_screen(xf, info, page, pt).distance(at), pt))
            .filter(|(d, _)| *d <= SNAP)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, pt)| pt);
        if let Some(pt) = snapped {
            target = pt;
        } else if shift && let Some(path) = state.paths.get(pi) {
            // From the neighbouring point: the one before in the path, else the one after.
            let neighbour = si
                .checked_sub(1)
                .and_then(|j| path.segments.get(j))
                .and_then(|s| point_of(s, VectorHandle::Endpoint))
                .or_else(|| path.segments.get(si + 1).and_then(|s| point_of(s, VectorHandle::Endpoint)));
            if let Some(from) = neighbour {
                target = snap_angle(from, target);
            }
        }
        if let Some(seg) = state.paths.get_mut(pi).and_then(|p| p.segments.get_mut(si)) {
            set_point(seg, handle, target);
        }
    }
    if resp.drag_stopped() || !ptr.primary_down {
        state.grab = None;
        if let Some(path) = state.paths.get(pi) {
            let patch = VectorPatch { id: path.id, segments: path.segments.clone(), stroke_width: None, color: None };
            view.pending_edit = Some(Edit::EditVectors { page, edits: VectorEdits { patches: vec![patch], ..Default::default() } });
        }
    }
}

/// The region rectangle, every picked-up path and its points; the selected path is bold, its curve
/// handles joined to their end points by dashed tangents.
pub(crate) fn paint(painter: &egui::Painter, xf: &PageXform, info: &DocInfo, page: usize, view: &DocView) {
    let Some(state) = view.vector_edit.as_ref().filter(|s| s.page == page) else { return };
    let (a, b) = (to_screen(xf, info, page, [state.region[0], state.region[1]]), to_screen(xf, info, page, [state.region[2], state.region[3]]));
    let region = Rect::from_two_pos(a, b);
    painter.extend(Shape::dashed_line(
        &[region.left_top(), region.right_top(), region.right_bottom(), region.left_bottom(), region.left_top()],
        Stroke::new(1.0, BLUE),
        5.0,
        4.0,
    ));
    for (pi, path) in state.paths.iter().enumerate() {
        let selected = state.selected == Some(pi);
        let stroke = Stroke::new(if selected { 2.5 } else { 1.5 }, if selected { BLUE } else { BLUE.gamma_multiply(0.6) });
        for run in outline(xf, info, page, path) {
            painter.add(Shape::line(run, stroke));
        }
        for (si, h, p) in handles(path) {
            let at = to_screen(xf, info, page, p);
            match h {
                VectorHandle::Endpoint => {
                    painter.rect_filled(Rect::from_center_size(at, egui::vec2(7.0, 7.0)), 0.0, BLUE);
                }
                VectorHandle::Ctrl1 | VectorHandle::Ctrl2 => {
                    if selected {
                        // The tangent runs to the end point this handle belongs to.
                        let anchor = if h == VectorHandle::Ctrl1 {
                            si.checked_sub(1).and_then(|j| path.segments.get(j)).and_then(|s| point_of(s, VectorHandle::Endpoint))
                        } else {
                            path.segments.get(si).and_then(|s| point_of(s, VectorHandle::Endpoint))
                        };
                        if let Some(anchor) = anchor {
                            painter.extend(Shape::dashed_line(&[to_screen(xf, info, page, anchor), at], Stroke::new(1.0, BLUE), 3.0, 3.0));
                        }
                    }
                    painter.circle(at, 4.0, Color32::WHITE, Stroke::new(1.5, BLUE));
                }
            }
        }
    }
}

/// Delete removes the selected path, Escape puts the picked-up paths away. Keys come through
/// [`PageKeys`], so they are not read while something covers the pages.
pub(crate) fn keys(ctx: &egui::Context, keys: PageKeys, view: &mut DocView) {
    use egui::Key;
    let Some(state) = view.vector_edit.as_mut() else { return };
    if keys.take(ctx, Key::Escape) {
        view.vector_edit = None;
        return;
    }
    let delete = keys.take(ctx, Key::Delete) | keys.take(ctx, Key::Backspace);
    if delete
        && state.grab.is_none()
        && let Some(path) = state.selected.and_then(|i| state.paths.get(i))
    {
        let edits = VectorEdits { deleted: vec![path.id], ..Default::default() };
        view.pending_edit = Some(Edit::EditVectors { page: state.page, edits });
        state.selected = None;
    }
}

/// Read the paths again when the document changed under them (an edit, an undo or a redo), and
/// drop the selection that no longer exists. `read` is the document's answer for `page`/`region`.
pub(crate) fn refresh(view: &mut DocView, generation: u64, read: impl FnOnce(usize, [f64; 4]) -> Option<Vec<VectorPath>>) {
    let Some(state) = view.vector_edit.as_mut() else { return };
    if state.generation == generation || state.grab.is_some() {
        return;
    }
    let selected_id = state.selected.and_then(|i| state.paths.get(i)).map(|p| p.id);
    if let Some(paths) = read(state.page, state.region) {
        state.selected = selected_id.and_then(|id| paths.iter().position(|p| p.id == id));
        state.paths = paths;
    }
    state.generation = generation;
}

impl crate::PdfCraftApp {
    /// A rectangle dragged with the vector tool: pick up the line art inside it. A click is
    /// not a rectangle.
    pub(crate) fn finish_vector_region(&mut self, index: usize, done: crate::zoom_snap::Marquee) {
        let (page, _, v) = done;
        let Some(view) = self.views.get_mut(index) else { return };
        // A click is no rectangle: it selects or clears a selection in `page_input`.
        if v[2] - v[0] < 2.0 || v[3] - v[1] < 2.0 {
            return;
        }
        let Some(doc) = self.session.get(view.id).filter(|d| d.allows_modification()) else { return };
        let Some(info) = doc.info.pages.get(page) else { return };
        let (a, b) = (info.view_to_user(v[0], v[1]), info.view_to_user(v[2], v[3]));
        let region = [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])].map(f64::from);
        let paths = doc.vector_paths_in_region(page, region).unwrap_or_default();
        view.vector_edit = Some(VectorEditState { page, region, paths, generation: doc.edit_generation(), selected: None, grab: None });
    }

    /// Read the picked-up paths again after the document changed under them.
    pub(crate) fn refresh_vectors(&mut self, index: usize) {
        let Some(id) = self.views.get(index).map(|v| v.id) else { return };
        let Some(doc) = self.session.get(id) else { return };
        let generation = doc.edit_generation();
        if let Some(view) = self.views.get_mut(index) {
            refresh(view, generation, |page, region| doc.vector_paths_in_region(page, region).ok());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handles_list_every_draggable_point_once() {
        let path = VectorPath {
            id: 0,
            span: [0, 1],
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            paint_op: "S".into(),
            segments: vec![
                PathSeg::MoveTo([0.0, 0.0]),
                PathSeg::CurveTo([1.0, 1.0], [2.0, 1.0], [3.0, 0.0]),
                PathSeg::LineTo([4.0, 0.0]),
                PathSeg::Close,
            ],
            bbox: [0.0; 4],
            stroke_width: 1.0,
            color: [0.0; 3],
            closed: true,
        };
        let h = handles(&path);
        assert_eq!(h.len(), 5, "move, two control points and the end of the curve, line end");
        let mut moved = path.clone();
        set_point(&mut moved.segments[1], VectorHandle::Ctrl2, [9.0, 9.0]);
        assert_eq!(point_of(&moved.segments[1], VectorHandle::Ctrl2), Some([9.0, 9.0]));
        assert_eq!(point_of(&moved.segments[0], VectorHandle::Ctrl1), None, "a move has no control point");
    }

    #[test]
    fn shift_snaps_to_multiples_of_45_degrees() {
        let p = snap_angle([0.0, 0.0], [10.0, 1.0]);
        assert!((p[0] - 10.0_f64.hypot(1.0)).abs() < 1e-9 && p[1].abs() < 1e-9, "{p:?}");
        let q = snap_angle([0.0, 0.0], [5.0, 6.0]);
        assert!((q[0] - q[1]).abs() < 1e-9, "{q:?}");
        assert_eq!(snap_angle([1.0, 1.0], [1.0, 1.0]), [1.0, 1.0]);
    }

    #[test]
    fn distance_to_a_segment_clamps_to_its_ends() {
        let (a, b) = (Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0));
        assert!((distance_to_segment(Pos2::new(5.0, 3.0), a, b) - 3.0).abs() < 1e-6);
        assert!((distance_to_segment(Pos2::new(-4.0, 3.0), a, b) - 5.0).abs() < 1e-6);
        assert!((distance_to_segment(Pos2::new(1.0, 1.0), a, a) - 2.0_f32.sqrt()).abs() < 1e-6);
    }
}

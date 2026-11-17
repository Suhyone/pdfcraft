//! Region vector editing: the line art a page draws inside a rectangle, read out of its content
//! streams as editable paths, and edited by rewriting exactly those operators.
//!
//! A path is the run of construction operators (`m l c v y h re`) up to the operator that
//! paints it (`S s f F f* B B* b b*`). Its [`VectorPath::id`] is its 0-based position among the
//! painted paths of the page, so an id read from [`vector_paths_in_region`] is good for
//! [`update_vector_paths`] as long as the page content has not changed in between.
//!
//! Edits are byte-exact: a deleted path's operators are cut out of the stream (nothing is drawn
//! over it), a modified path's operators are replaced in place, and everything else is copied
//! as it was. Paths that can't be edited safely (they set a clip, change the matrix while being
//! built, or have more segments than [`MAX_SEGMENTS`]) are never offered.

use pdfcraft_content::{Matrix, Op, Pieces, num, serialize_ops};
use pdfcraft_cos::{Document, ObjRef};
use std::collections::HashSet;

use crate::EditError;
use crate::page_streams::PageStreams;
use crate::place::page_list;

/// The most paths [`vector_paths_in_region`] returns.
pub const MAX_PATHS: usize = 500;
/// The most segments a path may have to be offered for editing.
pub const MAX_SEGMENTS: usize = 20_000;
/// Nesting of `q` the scan accepts, as everywhere else.
const MAX_DEPTH: usize = 1024;
/// The largest coordinate (points) an edit may write.
const COORD_LIMIT: f64 = 1e9;
/// The widest stroke (points) an edit may write.
const MAX_WIDTH: f64 = 10_000.0;

/// One segment of a path, in PDF user space of the page (bottom-up points).
#[derive(Clone, Debug, PartialEq)]
pub enum PathSeg {
    MoveTo([f64; 2]),
    LineTo([f64; 2]),
    /// Two control points, then the end point (`v` and `y` are read as this).
    CurveTo([f64; 2], [f64; 2], [f64; 2]),
    Close,
}

/// A painted path of a page.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorPath {
    /// 0-based index among the painted paths of the page.
    pub id: usize,
    /// Where the path's operators are in the page's joined content: from its first
    /// construction operator to the end of its paint operator.
    pub span: [usize; 2],
    /// The matrix in force where the path was built.
    pub ctm: [f64; 6],
    /// The paint operator: `S s f F f* B B* b b*`.
    pub paint_op: String,
    /// The segments, in page user space.
    pub segments: Vec<PathSeg>,
    /// `[x0, y0, x1, y1]` of every point (control points included), in page user space.
    pub bbox: [f64; 4],
    /// The line width on the page (points, after the matrix).
    pub stroke_width: f64,
    /// The stroke colour for a stroked path, the fill colour for a fill-only one (RGB, 0..1).
    pub color: [f64; 3],
    pub closed: bool,
}

/// A new shape for a path that exists.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorPatch {
    /// The [`VectorPath::id`] to replace.
    pub id: usize,
    /// The new segments, in page user space. The first must be a `MoveTo`.
    pub segments: Vec<PathSeg>,
    /// A new line width (points on the page); `None` keeps the one in force.
    pub stroke_width: Option<f64>,
    /// A new colour (RGB, 0..1; the stroke's for a stroked path, the fill's otherwise); `None`
    /// keeps the one in force.
    pub color: Option<[f64; 3]>,
}

/// A path to add to the page, in page user space, stroked with `S`.
#[derive(Clone, Debug, PartialEq)]
pub struct NewVectorPath {
    pub segments: Vec<PathSeg>,
    pub stroke_width: f64,
    pub color: [f64; 3],
}

/// Everything one edit changes on a page.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorEdits {
    /// Paths to remove.
    pub deleted: Vec<usize>,
    /// Paths to reshape.
    pub patches: Vec<VectorPatch>,
    /// Paths to add after everything the page draws.
    pub added: Vec<NewVectorPath>,
}

impl VectorEdits {
    pub fn is_empty(&self) -> bool {
        self.deleted.is_empty() && self.patches.is_empty() && self.added.is_empty()
    }

    /// How many paths this edit touches.
    pub fn len(&self) -> usize {
        self.deleted.len() + self.patches.len() + self.added.len()
    }
}

#[derive(Clone)]
struct Gfx {
    ctm: Matrix,
    width: f64,
    stroke: [f64; 3],
    fill: [f64; 3],
}

struct Building {
    start: usize,
    ctm: Matrix,
    segs: Vec<PathSeg>,
    cur: Option<(f64, f64)>,
    begin: (f64, f64),
    bad: bool,
    clip: bool,
}

struct Scanned {
    streams: PageStreams,
    pieces: Pieces,
    page: ObjRef,
    paths: Vec<VectorPath>,
    /// The matrix in force after the last operator.
    end_ctm: Matrix,
    /// Inline images the parser could not place: byte spans after one are not trustworthy.
    unsure: bool,
}

fn rgb_of_cmyk(c: f64, m: f64, y: f64, k: f64) -> [f64; 3] {
    let f = |v: f64| ((1.0 - v.clamp(0.0, 1.0)) * (1.0 - k.clamp(0.0, 1.0))).clamp(0.0, 1.0);
    [f(c), f(m), f(y)]
}

fn is_paint(op: &[u8]) -> bool {
    matches!(op, b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*")
}

fn strokes(op: &[u8]) -> bool {
    matches!(op, b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*")
}

fn scale_of(m: &Matrix) -> f64 {
    let [a, b, c, d, _, _] = m.0;
    (a * d - b * c).abs().sqrt()
}

fn finite_point(p: (f64, f64)) -> bool {
    p.0.is_finite() && p.1.is_finite() && p.0.abs() <= COORD_LIMIT && p.1.abs() <= COORD_LIMIT
}

impl Building {
    fn new(start: usize, ctm: Matrix) -> Self {
        Self { start, ctm, segs: Vec::new(), cur: None, begin: (0.0, 0.0), bad: false, clip: false }
    }

    fn point(&mut self, g: &Gfx, x: f64, y: f64) -> (f64, f64) {
        let p = g.ctm.apply(x, y);
        if !finite_point(p) {
            self.bad = true;
        }
        p
    }

    fn push(&mut self, seg: PathSeg) {
        if self.segs.len() >= MAX_SEGMENTS {
            self.bad = true;
        } else {
            self.segs.push(seg);
        }
    }
}

fn bbox_of(segs: &[PathSeg]) -> Option<[f64; 4]> {
    let mut b = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut any = false;
    let mut add = |p: &[f64; 2]| {
        any = true;
        b = [b[0].min(p[0]), b[1].min(p[1]), b[2].max(p[0]), b[3].max(p[1])];
    };
    for s in segs {
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
    any.then_some(b)
}

/// The drawing state while a content stream is walked.
struct Walk {
    g: Gfx,
    stack: Vec<Gfx>,
    building: Option<Building>,
}

impl Walk {
    /// Graphics-state operators: nesting, matrix, line width and colours.
    fn state_op(&mut self, op: &Op) -> Result<(), EditError> {
        match op.op.as_slice() {
            b"q" => {
                if self.stack.len() >= MAX_DEPTH {
                    return Err(EditError::Invalid("the page graphics state is nested too deeply".into()));
                }
                self.stack.push(self.g.clone());
            }
            b"Q" => {
                if let Some(s) = self.stack.pop() {
                    self.g = s;
                }
            }
            b"cm" => {
                if let Some(m) = op.nums::<6>() {
                    if let Some(b) = self.building.as_mut() {
                        b.bad = true;
                    }
                    self.g.ctm = Matrix(m).then(&self.g.ctm);
                }
            }
            b"w" => {
                if let Some([w]) = op.nums::<1>() {
                    self.g.width = w;
                }
            }
            b"RG" => self.g.stroke = op.nums::<3>().unwrap_or(self.g.stroke),
            b"rg" => self.g.fill = op.nums::<3>().unwrap_or(self.g.fill),
            b"G" => self.g.stroke = op.nums::<1>().map_or(self.g.stroke, |[v]| [v; 3]),
            b"g" => self.g.fill = op.nums::<1>().map_or(self.g.fill, |[v]| [v; 3]),
            b"K" => self.g.stroke = op.nums::<4>().map_or(self.g.stroke, |[c, m, y, k]| rgb_of_cmyk(c, m, y, k)),
            b"k" => self.g.fill = op.nums::<4>().map_or(self.g.fill, |[c, m, y, k]| rgb_of_cmyk(c, m, y, k)),
            _ => {}
        }
        Ok(())
    }

    /// Path construction operators (and the ones that end a path without painting it).
    fn path_op(&mut self, op: &Op) {
        let g = &self.g;
        match op.op.as_slice() {
            b"m" => {
                if let Some([x, y]) = op.nums::<2>() {
                    let b = self.building.get_or_insert_with(|| Building::new(op.span.start, g.ctm));
                    let pt = b.point(g, x, y);
                    b.cur = Some(pt);
                    b.begin = pt;
                    b.push(PathSeg::MoveTo([pt.0, pt.1]));
                }
            }
            b"l" => {
                if let (Some(b), Some([x, y])) = (self.building.as_mut(), op.nums::<2>())
                    && b.cur.is_some()
                {
                    let pt = b.point(g, x, y);
                    b.cur = Some(pt);
                    b.push(PathSeg::LineTo([pt.0, pt.1]));
                }
            }
            b"c" => {
                if let (Some(b), Some([x1, y1, x2, y2, x3, y3])) = (self.building.as_mut(), op.nums::<6>())
                    && b.cur.is_some()
                {
                    let (p1, p2, p3) = (b.point(g, x1, y1), b.point(g, x2, y2), b.point(g, x3, y3));
                    b.cur = Some(p3);
                    b.push(PathSeg::CurveTo([p1.0, p1.1], [p2.0, p2.1], [p3.0, p3.1]));
                }
            }
            b"v" => {
                if let (Some(b), Some([x2, y2, x3, y3])) = (self.building.as_mut(), op.nums::<4>())
                    && let Some(c0) = b.cur
                {
                    let (p2, p3) = (b.point(g, x2, y2), b.point(g, x3, y3));
                    b.cur = Some(p3);
                    b.push(PathSeg::CurveTo([c0.0, c0.1], [p2.0, p2.1], [p3.0, p3.1]));
                }
            }
            b"y" => {
                if let (Some(b), Some([x1, y1, x3, y3])) = (self.building.as_mut(), op.nums::<4>())
                    && b.cur.is_some()
                {
                    let (p1, p3) = (b.point(g, x1, y1), b.point(g, x3, y3));
                    b.cur = Some(p3);
                    b.push(PathSeg::CurveTo([p1.0, p1.1], [p3.0, p3.1], [p3.0, p3.1]));
                }
            }
            b"h" => {
                if let Some(b) = self.building.as_mut()
                    && b.cur.is_some()
                {
                    b.cur = Some(b.begin);
                    b.push(PathSeg::Close);
                }
            }
            b"re" => {
                if let Some([x, y, w, h]) = op.nums::<4>() {
                    let b = self.building.get_or_insert_with(|| Building::new(op.span.start, g.ctm));
                    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
                    let pts: Vec<(f64, f64)> = corners.iter().map(|&(cx, cy)| b.point(g, cx, cy)).collect();
                    if let [p0, p1, p2, p3] = pts[..] {
                        b.cur = Some(p0);
                        b.begin = p0;
                        b.push(PathSeg::MoveTo([p0.0, p0.1]));
                        for p in [p1, p2, p3] {
                            b.push(PathSeg::LineTo([p.0, p.1]));
                        }
                        b.push(PathSeg::Close);
                    }
                }
            }
            b"W" | b"W*" => {
                if let Some(b) = self.building.as_mut() {
                    b.clip = true;
                }
            }
            b"n" => self.building = None,
            _ => {}
        }
    }
}

/// A painted path of the walk, if it can be offered: not a clip, not built across a matrix
/// change, not too long. Also its bounding box widened by half its stroke.
fn finished(b: Building, id: usize, op: &Op, g: &Gfx) -> Option<(VectorPath, [f64; 4])> {
    if b.bad || b.clip {
        return None;
    }
    let bbox = bbox_of(&b.segs)?;
    let name = op.op.as_slice();
    let width = (g.width * scale_of(&b.ctm)).abs();
    let pad = (width / 2.0).max(0.5);
    let wide = [bbox[0] - pad, bbox[1] - pad, bbox[2] + pad, bbox[3] + pad];
    let closed = matches!(name, b"s" | b"b" | b"b*") || b.segs.iter().any(|s| matches!(s, PathSeg::Close));
    let path = VectorPath {
        id,
        span: [b.start, op.span.end],
        ctm: b.ctm.0,
        paint_op: String::from_utf8_lossy(name).into_owned(),
        segments: b.segs,
        bbox,
        stroke_width: width,
        color: if strokes(name) { g.stroke } else { g.fill },
        closed,
    };
    Some((path, wide))
}

/// Read the page's painted paths, keeping those `want` accepts (it gets the path's id and its
/// bounding box widened by half its stroke) until `limit` are kept.
fn scan(doc: &Document, page: usize, limit: usize, want: &dyn Fn(usize, [f64; 4]) -> bool) -> Result<Scanned, EditError> {
    let p = page_list(doc).into_iter().nth(page).ok_or(EditError::NoSuchPage(page))?;
    let streams = PageStreams::read(doc, &p.dict).ok_or_else(|| EditError::Invalid("the page content can't be read".into()))?;
    let pieces = Pieces::join(&streams.data);
    let parsed = pieces.parse();
    let mut walk = Walk { g: Gfx { ctm: Matrix::IDENTITY, width: 1.0, stroke: [0.0; 3], fill: [0.0; 3] }, stack: Vec::new(), building: None };
    let mut next_id = 0usize;
    let mut paths = Vec::new();
    for op in &parsed.ops {
        if is_paint(op.op.as_slice()) {
            let Some(b) = walk.building.take() else { continue };
            let id = next_id;
            next_id += 1;
            if b.segs.is_empty() {
                continue;
            }
            if let Some((path, wide)) = finished(b, id, op, &walk.g)
                && want(id, wide)
            {
                paths.push(path);
                if paths.len() >= limit {
                    break;
                }
            }
        } else {
            walk.state_op(op)?;
            walk.path_op(op);
        }
    }
    Ok(Scanned { page: p.obj, streams, pieces, paths, end_ctm: walk.g.ctm, unsure: parsed.ambiguous_images > 0 })
}

fn valid_region(r: [f64; 4]) -> Option<[f64; 4]> {
    r.iter().all(|v| v.is_finite()).then_some([r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])])
}

/// The painted paths of page `page` (0-based) that touch `region` (`[x0, y0, x1, y1]` in page
/// user space), at most [`MAX_PATHS`], in drawing order.
pub fn vector_paths_in_region(doc: &Document, page: usize, region: [f64; 4]) -> Result<Vec<VectorPath>, EditError> {
    let r = valid_region(region).ok_or_else(|| EditError::Invalid("the region is not a finite rectangle".into()))?;
    let touches = move |_: usize, b: [f64; 4]| b[0] <= r[2] && b[2] >= r[0] && b[1] <= r[3] && b[3] >= r[1];
    Ok(scan(doc, page, MAX_PATHS, &touches)?.paths)
}

fn finite_seg_points(segs: &[PathSeg]) -> bool {
    let ok = |p: &[f64; 2]| finite_point((p[0], p[1]));
    segs.iter().all(|s| match s {
        PathSeg::MoveTo(p) | PathSeg::LineTo(p) => ok(p),
        PathSeg::CurveTo(a, b, c) => ok(a) && ok(b) && ok(c),
        PathSeg::Close => true,
    })
}

fn check_segments(segs: &[PathSeg]) -> Result<(), EditError> {
    if !matches!(segs.first(), Some(PathSeg::MoveTo(_))) || segs.len() > MAX_SEGMENTS {
        return Err(EditError::Invalid("a vector path must start with a move and have a sensible length".into()));
    }
    if !finite_seg_points(segs) {
        return Err(EditError::Invalid("a vector path has a coordinate that is not a usable number".into()));
    }
    Ok(())
}

fn check_style(width: Option<f64>, color: Option<[f64; 3]>) -> Result<(), EditError> {
    if width.is_some_and(|w| !(0.0..=MAX_WIDTH).contains(&w)) || color.is_some_and(|c| c.iter().any(|v| !(0.0..=1.0).contains(v))) {
        return Err(EditError::Invalid("a vector path has a line width or colour out of range".into()));
    }
    Ok(())
}

/// The operators that draw `segs` (page user space) under a matrix whose inverse is `inv`.
fn path_ops(segs: &[PathSeg], inv: &Matrix) -> Vec<Op> {
    let local = |p: &[f64; 2]| inv.apply(p[0], p[1]);
    let pair = |p: &[f64; 2]| {
        let (x, y) = local(p);
        [num(x), num(y)]
    };
    segs.iter()
        .map(|s| match s {
            PathSeg::MoveTo(p) => Op::new("m", pair(p).to_vec()),
            PathSeg::LineTo(p) => Op::new("l", pair(p).to_vec()),
            PathSeg::CurveTo(a, b, c) => Op::new("c", [pair(a), pair(b), pair(c)].concat()),
            PathSeg::Close => Op::new("h", Vec::new()),
        })
        .collect()
}

fn colour_ops(op: &str, c: [f64; 3]) -> Op {
    Op::new(op, c.iter().map(|v| num(*v)).collect())
}

/// Apply `edits` to page `page` (0-based): remove the deleted paths' operators, reshape the
/// patched paths in place, and add the new ones after everything else. One call is one change
/// of the page content; nothing is written when an edit is refused.
pub fn update_vector_paths(doc: &mut Document, page: usize, edits: &VectorEdits) -> Result<(), EditError> {
    if edits.is_empty() {
        return Err(EditError::Invalid("there is no vector edit to apply".into()));
    }
    let mut wanted: HashSet<usize> = HashSet::new();
    for id in edits.deleted.iter().chain(edits.patches.iter().map(|p| &p.id)) {
        if !wanted.insert(*id) {
            return Err(EditError::Invalid(format!("vector path {id} is named twice")));
        }
    }
    for p in &edits.patches {
        check_segments(&p.segments)?;
        check_style(p.stroke_width, p.color)?;
    }
    for a in &edits.added {
        check_segments(&a.segments)?;
        check_style(Some(a.stroke_width), Some(a.color))?;
    }
    let scanned = scan(doc, page, usize::MAX, &|id, _| wanted.contains(&id))?;
    if scanned.unsure {
        return Err(EditError::Invalid("the page has an inline image this editor can't place, so its paths can't be edited safely".into()));
    }
    for id in &wanted {
        if !scanned.paths.iter().any(|p| p.id == *id) {
            return Err(EditError::Invalid(format!("vector path {id} is not on the page or can't be edited")));
        }
    }
    let mut splices: Vec<(std::ops::Range<usize>, Vec<u8>)> = Vec::new();
    for path in &scanned.paths {
        let range = path.span[0]..path.span[1];
        if edits.deleted.contains(&path.id) {
            splices.push((range, Vec::new()));
            continue;
        }
        let Some(patch) = edits.patches.iter().find(|p| p.id == path.id) else { continue };
        let ctm = Matrix(path.ctm);
        let inv = ctm.invert().ok_or_else(|| EditError::Invalid("a vector path has no area to edit under".into()))?;
        let scale = scale_of(&ctm);
        let styled = patch.stroke_width.is_some() || patch.color.is_some();
        let mut ops = Vec::new();
        if styled {
            ops.push(Op::new("q", Vec::new()));
            if let Some(w) = patch.stroke_width {
                ops.push(Op::new("w", vec![num(w / scale)]));
            }
            if let Some(c) = patch.color {
                ops.push(colour_ops(if strokes(path.paint_op.as_bytes()) { "RG" } else { "rg" }, c));
            }
        }
        ops.extend(path_ops(&patch.segments, &inv));
        ops.push(Op::new(&path.paint_op, Vec::new()));
        if styled {
            ops.push(Op::new("Q", Vec::new()));
        }
        let mut bytes = serialize_ops(&ops);
        if bytes.last() == Some(&b'\n') {
            bytes.pop();
        }
        splices.push((range, bytes));
    }
    if !edits.added.is_empty() {
        if scanned.pieces.is_empty() {
            return Err(EditError::Invalid("the page has no content stream to add to".into()));
        }
        let inv = scanned.end_ctm.invert().ok_or_else(|| EditError::Invalid("the page ends under a matrix that can't be undone".into()))?;
        let scale = scale_of(&scanned.end_ctm);
        let mut ops = Vec::new();
        for a in &edits.added {
            ops.push(Op::new("q", Vec::new()));
            ops.push(Op::new("w", vec![num(a.stroke_width / scale)]));
            ops.push(colour_ops("RG", a.color));
            ops.extend(path_ops(&a.segments, &inv));
            ops.push(Op::new("S", Vec::new()));
            ops.push(Op::new("Q", Vec::new()));
        }
        let mut bytes = b"\n".to_vec();
        bytes.extend(serialize_ops(&ops));
        let at = scanned.pieces.data().len().saturating_sub(1);
        splices.push((at..at, bytes));
    }
    splices.sort_by_key(|(r, _)| r.start);
    if splices.windows(2).any(|w| matches!(w, [a, b] if a.0.end > b.0.start)) {
        return Err(EditError::Invalid("vector paths overlap in the page content".into()));
    }
    let data = scanned.pieces.splice(splices);
    let new: Vec<Option<Vec<u8>>> = data.into_iter().zip(&scanned.streams.data).map(|(n, old)| (n != *old).then_some(n)).collect();
    scanned.streams.write(doc, scanned.page, new)?;
    Ok(())
}

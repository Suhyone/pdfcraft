//! The page's line art as JSON for a host page or an agent, and its edits back (`vector.extract`
//! and `vector.apply` of the control channel, `vector_list` and `vector_edit` of the automation
//! tools). Coordinates are PDF user space (bottom-up points), as in the editing code.

use crate::{NewVectorPath, PathSeg, VectorEdits, VectorPatch, VectorPath};
use serde_json::{Value, json};

/// Points sampled along each curve segment for display.
const CURVE_STEPS: usize = 8;
/// Entries one call may carry, so a request can't ask for an unbounded edit.
const MAX_ENTRIES: usize = 10_000;
/// Points one path may carry.
const MAX_POINTS: usize = 20_000;

fn hex(c: [f64; 3]) -> String {
    let b = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

fn flatten(segs: &[PathSeg]) -> (Vec<[f64; 2]>, bool) {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let mut curved = false;
    for s in segs {
        match s {
            PathSeg::MoveTo(p) | PathSeg::LineTo(p) => pts.push(*p),
            PathSeg::CurveTo(a, b, c) => {
                curved = true;
                let Some(p0) = pts.last().copied() else {
                    pts.push(*c);
                    continue;
                };
                for i in 1..=CURVE_STEPS {
                    let t = i as f64 / CURVE_STEPS as f64;
                    let u = 1.0 - t;
                    let f = |k: usize| u * u * u * p0[k] + 3.0 * u * u * t * a[k] + 3.0 * u * t * t * b[k] + t * t * t * c[k];
                    pts.push([f(0), f(1)]);
                }
            }
            PathSeg::Close => {}
        }
    }
    (pts, curved)
}

/// The reply of `vector.extract`.
pub fn paths_json(paths: &[VectorPath]) -> Value {
    let list: Vec<Value> = paths
        .iter()
        .map(|p| {
            let (points, curved) = flatten(&p.segments);
            json!({
                "id": p.id,
                "points": points,
                "strokeWidth": p.stroke_width,
                "color": hex(p.color),
                "closed": p.closed,
                "curved": curved,
                "paintOp": p.paint_op,
                "bbox": p.bbox,
            })
        })
        .collect();
    json!({ "paths": list })
}

fn number(p: &Value, key: &str) -> Result<f64, String> {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| format!("{key} must be a finite number"))
}

/// The 0-based `page` of a control request.
pub fn page_of(p: &Value) -> Result<usize, String> {
    p.get("page").and_then(Value::as_u64).and_then(|v| usize::try_from(v).ok()).ok_or_else(|| "page must be a 0-based page number".to_string())
}

/// The page and region of a `vector.extract` request: `{page, x, y, w, h}`.
pub fn parse_region(p: &Value) -> Result<(usize, [f64; 4]), String> {
    let (x, y, w, h) = (number(p, "x")?, number(p, "y")?, number(p, "w")?, number(p, "h")?);
    Ok((page_of(p)?, [x, y, x + w, y + h]))
}

/// A region given as `[x0, y0, x1, y1]`.
pub fn parse_rect(v: &Value) -> Result<[f64; 4], String> {
    match v.as_array().map(|a| a.iter().map(Value::as_f64).collect::<Vec<_>>()).as_deref() {
        Some([Some(a), Some(b), Some(c), Some(d)]) if [a, b, c, d].iter().all(|n| n.is_finite()) => Ok([*a, *b, *c, *d]),
        _ => Err("rect must be four finite numbers [x0, y0, x1, y1] in page user space".into()),
    }
}

fn colour(v: &Value) -> Result<[f64; 3], String> {
    if let Some(s) = v.as_str() {
        let h = s.strip_prefix('#').filter(|h| h.len() == 6 && h.is_ascii()).ok_or("a colour must be \"#rrggbb\" or [r, g, b] in 0..1")?;
        let part = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok()).map(|b| f64::from(b) / 255.0);
        return match (part(0), part(2), part(4)) {
            (Some(r), Some(g), Some(b)) => Ok([r, g, b]),
            _ => Err("a colour must be \"#rrggbb\" or [r, g, b] in 0..1".into()),
        };
    }
    match v.as_array().map(|a| a.iter().map(Value::as_f64).collect::<Vec<_>>()).as_deref() {
        Some([Some(r), Some(g), Some(b)]) => Ok([*r, *g, *b]),
        _ => Err("a colour must be \"#rrggbb\" or [r, g, b] in 0..1".into()),
    }
}

fn segments(entry: &Value) -> Result<Vec<PathSeg>, String> {
    let pts = entry.get("points").and_then(Value::as_array).ok_or("a path needs points: [[x, y], ...]")?;
    if pts.len() < 2 || pts.len() > MAX_POINTS {
        return Err(format!("a path needs between 2 and {MAX_POINTS} points"));
    }
    let mut segs = Vec::with_capacity(pts.len() + 1);
    for (i, pt) in pts.iter().enumerate() {
        let xy = match pt.as_array().map(Vec::as_slice) {
            Some([x, y]) => [x.as_f64(), y.as_f64()],
            _ => return Err("every point must be [x, y]".into()),
        };
        let [Some(x), Some(y)] = xy else { return Err("every point must be [x, y] numbers".into()) };
        segs.push(if i == 0 { PathSeg::MoveTo([x, y]) } else { PathSeg::LineTo([x, y]) });
    }
    if entry.get("closed").and_then(Value::as_bool) == Some(true) {
        segs.push(PathSeg::Close);
    }
    Ok(segs)
}

/// The edits of a request `{deleted: [id], modified: [{id?, points, strokeWidth?, color?,
/// closed?}]}`. A modified entry with an `id` reshapes that path; one without adds a new stroked
/// path.
pub fn parse_edits(p: &Value) -> Result<VectorEdits, String> {
    let mut edits = VectorEdits::default();
    if let Some(d) = p.get("deleted") {
        let list = d.as_array().ok_or("deleted must be a list of path ids")?;
        if list.len() > MAX_ENTRIES {
            return Err(format!("at most {MAX_ENTRIES} paths per call"));
        }
        for id in list {
            edits.deleted.push(id.as_u64().and_then(|v| usize::try_from(v).ok()).ok_or("deleted must be a list of path ids")?);
        }
    }
    if let Some(m) = p.get("modified") {
        let list = m.as_array().ok_or("modified must be a list of paths")?;
        if list.len() > MAX_ENTRIES {
            return Err(format!("at most {MAX_ENTRIES} paths per call"));
        }
        for entry in list {
            let segs = segments(entry)?;
            let width = entry.get("strokeWidth").map(|w| w.as_f64().filter(|v| v.is_finite()).ok_or("strokeWidth must be a number")).transpose()?;
            let color = entry.get("color").map(colour).transpose()?;
            match entry.get("id") {
                Some(id) => {
                    let id = id.as_u64().and_then(|v| usize::try_from(v).ok()).ok_or("a path id must be a whole number")?;
                    edits.patches.push(VectorPatch { id, segments: segs, stroke_width: width, color });
                }
                None => edits.added.push(NewVectorPath { segments: segs, stroke_width: width.unwrap_or(1.0), color: color.unwrap_or([0.0; 3]) }),
            }
        }
    }
    if edits.is_empty() {
        return Err("nothing to apply: give deleted ids or modified paths".into());
    }
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_region_request_becomes_a_rectangle() {
        assert_eq!(parse_region(&json!({"page": 2, "x": 10, "y": 20, "w": 30, "h": 40})).unwrap(), (2, [10.0, 20.0, 40.0, 60.0]));
        assert!(parse_region(&json!({"page": 0, "x": 1, "y": 2, "w": 3})).is_err());
        assert!(parse_region(&json!({"page": -1, "x": 1, "y": 2, "w": 3, "h": 4})).is_err());
    }

    #[test]
    fn edits_split_into_deleted_patched_and_added() {
        let req = json!({"page": 0, "deleted": [4], "modified": [
            {"id": 1, "points": [[0, 0], [5, 5]], "strokeWidth": 2, "color": "#ff0000"},
            {"points": [[1, 1], [2, 2], [3, 1]], "closed": true}
        ]});
        let e = parse_edits(&req).unwrap();
        assert_eq!(e.deleted, vec![4]);
        assert_eq!(e.patches.len(), 1);
        assert_eq!(e.patches[0].color, Some([1.0, 0.0, 0.0]));
        assert_eq!(e.added.len(), 1);
        assert_eq!(e.added[0].segments.last(), Some(&PathSeg::Close));
    }

    #[test]
    fn bad_requests_are_refused_with_a_reason() {
        for bad in [
            json!({"page": 0}),
            json!({"page": 0, "deleted": ["a"]}),
            json!({"page": 0, "modified": [{"id": 1, "points": [[0, 0]]}]}),
            json!({"page": 0, "modified": [{"id": 1, "points": [[0, 0], [1]]}]}),
            json!({"page": 0, "modified": [{"id": 1, "points": [[0, 0], [1, 1]], "color": "red"}]}),
            json!({"page": "x", "deleted": [1]}),
        ] {
            assert!(parse_edits(&bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_curve_is_sampled_for_display() {
        let segs = vec![PathSeg::MoveTo([0.0, 0.0]), PathSeg::CurveTo([0.0, 10.0], [10.0, 10.0], [10.0, 0.0])];
        let (pts, curved) = flatten(&segs);
        assert!(curved);
        assert_eq!(pts.len(), 1 + CURVE_STEPS);
        assert_eq!(pts.last(), Some(&[10.0, 0.0]));
    }
}

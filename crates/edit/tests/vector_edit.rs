//! Region vector editing: paths are read with the matrix they are drawn under, deleted by cutting
//! their operators out (nothing is drawn over them), reshaped in place, and added at the end.

use pdfcraft_cos::{Document, Object};
use pdfcraft_edit::{NewVectorPath, PathSeg, VectorEdits, VectorPatch, update_vector_paths, vector_paths_in_region};
use std::sync::Arc;

fn one_page(content: &[u8]) -> Document {
    let mut objs: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
    ];
    let mut s = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    s.extend_from_slice(content);
    s.extend_from_slice(b"\nendstream");
    objs.push(s);
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offs = Vec::new();
    for (i, o) in objs.iter().enumerate() {
        offs.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(o);
        out.extend_from_slice(b"\nendobj\n");
    }
    let x = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for o in offs {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{x}\n%%EOF\n", objs.len() + 1).as_bytes());
    Document::open(Arc::new(out)).unwrap()
}

fn content_text(doc: &Document) -> String {
    let page = pdfcraft_model::pages(doc).swap_remove(0);
    let c = page.dict.get(b"Contents").unwrap();
    let list: Vec<Object> = match &*doc.resolve(c) {
        Object::Array(a) => a.clone(),
        _ => vec![c.clone()],
    };
    let mut text = String::new();
    for e in list {
        if let Object::Stream(s) = &*doc.resolve(&e) {
            text.push_str(&String::from_utf8_lossy(&s.decoded().unwrap()));
            text.push(' ');
        }
    }
    text
}

const ALL: [f64; 4] = [-1e6, -1e6, 1e6, 1e6];

fn line(a: [f64; 2], b: [f64; 2]) -> Vec<PathSeg> {
    vec![PathSeg::MoveTo(a), PathSeg::LineTo(b)]
}

fn only(edits: VectorEdits) -> VectorEdits {
    edits
}

#[test]
fn vector_reads_paths_under_their_matrix_with_width_and_colour() {
    let doc = one_page(b"1 0 0 RG 3 w q 2 0 0 2 10 10 cm 0 0 m 10 0 l S Q 0 0 0 RG 300 300 m 400 300 l S");
    let all = vector_paths_in_region(&doc, 0, ALL).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].segments, line([10.0, 10.0], [30.0, 10.0]));
    assert!((all[0].stroke_width - 6.0).abs() < 1e-9, "width is scaled by the matrix");
    assert_eq!(all[0].color, [1.0, 0.0, 0.0]);
    assert_eq!(all[1].id, 1);
    let near = vector_paths_in_region(&doc, 0, [0.0, 0.0, 50.0, 50.0]).unwrap();
    assert_eq!(near.len(), 1, "a horizontal line has no height but is still found");
    assert_eq!(near[0].id, 0);
    // Corners given in the other order mean the same rectangle.
    assert_eq!(vector_paths_in_region(&doc, 0, [50.0, 50.0, 0.0, 0.0]).unwrap().len(), 1);
}

#[test]
fn vector_a_rectangle_is_four_corners_and_a_close() {
    let doc = one_page(b"10 20 30 40 re f");
    let p = vector_paths_in_region(&doc, 0, ALL).unwrap();
    assert_eq!(p[0].segments.len(), 5);
    assert_eq!(p[0].segments.last(), Some(&PathSeg::Close));
    assert_eq!(p[0].bbox, [10.0, 20.0, 40.0, 60.0]);
    assert!(p[0].closed);
}

#[test]
fn vector_clipping_and_matrix_changes_inside_a_path_are_not_offered() {
    let doc = one_page(b"0 0 10 10 re W n 0 0 m 5 5 l 2 0 0 2 0 0 cm 9 9 l S 20 20 m 30 30 l S");
    let p = vector_paths_in_region(&doc, 0, ALL).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].id, 1, "ids still count the path that is not offered");
}

#[test]
fn vector_more_than_100_paths_in_a_region_is_refused_with_a_count() {
    let lines = |n: usize| (0..n).map(|i| format!("{i} 0 m {i} 5 l S ")).collect::<String>();
    let doc = one_page(lines(101).as_bytes());
    let err = vector_paths_in_region(&doc, 0, ALL).unwrap_err();
    assert!(matches!(err, pdfcraft_edit::EditError::TooManyObjects(101)), "{err:?}");
    assert_eq!(err.to_string(), "TOO_MANY_OBJECTS:101");
    // A smaller region of the same page is fine, and exactly 100 are allowed.
    assert_eq!(vector_paths_in_region(&doc, 0, [0.0, 0.0, 10.2, 10.0]).unwrap().len(), 11);
    assert_eq!(vector_paths_in_region(&one_page(lines(100).as_bytes()), 0, ALL).unwrap().len(), 100);
}

#[test]
fn vector_a_leader_leaving_the_region_brings_its_arrowhead_along() {
    // A frame, a leader from its edge to far away, and a filled arrowhead at the far end.
    let doc = one_page(b"10 10 60 30 re S 70 25 m 170 105 l S 170 105 m 160 103 l 166 95 l h f 400 400 m 450 450 l S");
    let near_frame = vector_paths_in_region(&doc, 0, [0.0, 0.0, 80.0, 40.0]).unwrap();
    let ids: Vec<usize> = near_frame.iter().map(|p| p.id).collect();
    assert_eq!(ids, vec![0, 1, 2], "the frame, the leader and the arrowhead past the region, not the far line");
    let c = pdfcraft_edit::connectors(&near_frame);
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].leader, c[0].heads.clone(), c[0].frame, c[0].tail_first), (1, vec![2], Some(0), true));
}

#[test]
fn vector_deleting_cuts_the_operators_out_and_draws_nothing_over_them() {
    let mut doc = one_page(b"q 1 0 0 1 5 5 cm 0 0 m 10 10 l S Q 100 100 m 200 200 l S");
    update_vector_paths(&mut doc, 0, &VectorEdits { deleted: vec![0], ..Default::default() }).unwrap();
    let text = content_text(&doc);
    assert!(!text.contains("10 10 l"), "{text}");
    assert!(text.contains("100 100 m 200 200 l S"), "{text}");
    assert!(!text.contains(" re") && !text.contains(" rg"), "no cover-up is drawn: {text}");
    let left = vector_paths_in_region(&doc, 0, ALL).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].segments, line([100.0, 100.0], [200.0, 200.0]));
}

#[test]
fn vector_reshaping_is_written_back_through_the_inverse_matrix() {
    let mut doc = one_page(b"q 2 0 0 2 10 10 cm 0 0 m 10 0 l S Q");
    let patch = VectorPatch { id: 0, segments: line([10.0, 10.0], [50.0, 90.0]), stroke_width: Some(4.0), color: Some([0.0, 0.0, 1.0]) };
    update_vector_paths(&mut doc, 0, &VectorEdits { patches: vec![patch], ..Default::default() }).unwrap();
    let p = vector_paths_in_region(&doc, 0, ALL).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(p[0].segments, line([10.0, 10.0], [50.0, 90.0]));
    assert!((p[0].stroke_width - 4.0).abs() < 1e-9);
    assert_eq!(p[0].color, [0.0, 0.0, 1.0]);
}

#[test]
fn vector_a_new_path_is_added_after_everything_even_when_the_page_ends_scaled() {
    let mut doc = one_page(b"q 0.5 0 0 0.5 0 0 cm 0 0 m 10 10 l S");
    let new = NewVectorPath { segments: line([100.0, 100.0], [200.0, 100.0]), stroke_width: 2.0, color: [0.0, 1.0, 0.0] };
    update_vector_paths(&mut doc, 0, &VectorEdits { added: vec![new], ..Default::default() }).unwrap();
    let p = vector_paths_in_region(&doc, 0, ALL).unwrap();
    assert_eq!(p.len(), 2);
    assert_eq!(p[1].segments, line([100.0, 100.0], [200.0, 100.0]));
    assert_eq!(p[1].color, [0.0, 1.0, 0.0]);
    assert!((p[1].stroke_width - 2.0).abs() < 1e-9);
}

#[test]
fn vector_refused_edits_change_nothing() {
    let mut doc = one_page(b"0 0 m 10 10 l S");
    let before = content_text(&doc);
    let nan = VectorPatch { id: 0, segments: line([f64::NAN, 0.0], [1.0, 1.0]), stroke_width: None, color: None };
    assert!(update_vector_paths(&mut doc, 0, &only(VectorEdits { patches: vec![nan], ..Default::default() })).is_err());
    assert!(update_vector_paths(&mut doc, 0, &VectorEdits { deleted: vec![7], ..Default::default() }).is_err(), "unknown id");
    assert!(update_vector_paths(&mut doc, 0, &VectorEdits { deleted: vec![0, 0], ..Default::default() }).is_err(), "duplicate id");
    assert!(update_vector_paths(&mut doc, 0, &VectorEdits::default()).is_err(), "empty edit");
    assert!(update_vector_paths(&mut doc, 3, &VectorEdits { deleted: vec![0], ..Default::default() }).is_err(), "no such page");
    let no_move = VectorPatch { id: 0, segments: vec![PathSeg::LineTo([1.0, 1.0])], stroke_width: None, color: None };
    assert!(update_vector_paths(&mut doc, 0, &VectorEdits { patches: vec![no_move], ..Default::default() }).is_err());
    assert!(vector_paths_in_region(&doc, 0, [f64::NAN, 0.0, 1.0, 1.0]).is_err());
    assert_eq!(content_text(&doc), before);
}

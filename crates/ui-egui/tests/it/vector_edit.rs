//! Edit ▸ Edit vector lines in the real interface: drag a rectangle over line art, drag one of its
//! points, delete a path with the keyboard, and undo each as one step.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use pdfcraft_ui_egui::{PdfCraftApp, QuickTool};

/// One 300×200 page drawing an L-shaped red line and a separate green one.
fn lines() -> Vec<u8> {
    page_with("1 0 0 RG 2 w 50 50 m 150 50 l 150 120 l S 0 1 0 RG 200 30 m 280 30 l S")
}

/// The same page size with other content.
fn page_with(content: &str) -> Vec<u8> {
    format!(
        "%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] /Contents 4 0 R >> endobj
4 0 obj << /Length {} >> stream
{content}
endstream endobj
trailer << /Root 1 0 R >>
%%EOF",
        content.len()
    )
    .into_bytes()
}

fn harness() -> Harness<'static, PdfCraftApp> {
    harness_on(lines())
}

fn harness_on(pdf: Vec<u8>) -> Harness<'static, PdfCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("lines.pdf", None, pdf.clone()).expect("opens");
        app.set_option("left", "closed").unwrap();
        app.set_option("author", "Tester").unwrap();
        assert!(app.execute("edit.vectors"));
        app
    });
    crate::support::wait_until(&mut h, "the page on screen", |app| {
        app.views[0].view_point_screen_pos(0, 10.0, 10.0).is_some() && !app.render_pending()
    });
    h
}

/// A point of the page in view space (the page is 200 high, y down).
fn at(h: &Harness<'static, PdfCraftApp>, user_x: f32, user_y: f32) -> Pos2 {
    h.state().views[0].view_point_screen_pos(0, user_x, 200.0 - user_y).expect("page on screen")
}

fn drag(h: &mut Harness<'static, PdfCraftApp>, from: Pos2, to: Pos2) {
    h.hover_at(from);
    h.run_steps(1);
    h.event(Event::PointerButton { pos: from, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    for k in 1..=4 {
        h.hover_at(from + (to - from) * (k as f32 / 4.0));
        h.run_steps(1);
    }
    h.event(Event::PointerButton { pos: to, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
}

/// Drag from one point of the page (user space) to another.
fn drag_user(h: &mut Harness<'static, PdfCraftApp>, from: [f32; 2], to: [f32; 2]) {
    let (a, b) = (at(h, from[0], from[1]), at(h, to[0], to[1]));
    drag(h, a, b);
}

fn picked(h: &Harness<'static, PdfCraftApp>) -> Vec<pdfcraft_engine::VectorPath> {
    h.state().views[0].vector_edit.as_ref().map(|s| s.paths.clone()).unwrap_or_default()
}

fn all_paths(h: &Harness<'static, PdfCraftApp>) -> Vec<pdfcraft_engine::VectorPath> {
    let app = h.state();
    let doc = app.session.get(app.views[0].id).expect("document");
    doc.vector_paths_in_region(0, [-1e5, -1e5, 1e5, 1e5]).expect("paths")
}

#[test]
fn a_rectangle_picks_up_line_art_and_a_dragged_point_is_one_undo_step() {
    let mut h = harness();
    assert_eq!(h.state().quick_tool, QuickTool::VectorEdit);
    // Around the red line only.
    drag_user(&mut h, [30.0, 140.0], [170.0, 40.0]);
    let found = picked(&h);
    assert_eq!(found.len(), 1, "only the red line is inside the rectangle");
    assert_eq!(found[0].color, [1.0, 0.0, 0.0]);
    // Drag the corner (150, 120) up and to the right.
    let before = all_paths(&h);
    drag_user(&mut h, [150.0, 120.0], [190.0, 160.0]);
    let after = all_paths(&h);
    assert_eq!(after.len(), before.len());
    let moved = after[0].segments.last().cloned();
    assert!(matches!(moved, Some(pdfcraft_engine::PathSeg::LineTo(p)) if (p[0] - 190.0).abs() < 3.0 && (p[1] - 160.0).abs() < 3.0), "{moved:?}");
    assert_eq!(after[1], before[1], "the green line is untouched");
    // One Undo takes the whole drag back, and the picked-up copy follows.
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(3);
    assert_eq!(all_paths(&h)[0].segments, before[0].segments);
    assert_eq!(picked(&h)[0].segments, before[0].segments);
}

#[test]
fn delete_removes_the_selected_path_and_escape_puts_the_rest_away() {
    let mut h = harness();
    drag_user(&mut h, [30.0, 140.0], [170.0, 40.0]);
    // Click on the line to select it.
    let on_line = at(&h, 100.0, 50.0);
    h.hover_at(on_line);
    h.run_steps(1);
    h.event(Event::PointerButton { pos: on_line, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerButton { pos: on_line, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(3);
    assert!(h.state().views[0].vector_edit.as_ref().is_some_and(|s| s.selected.is_some()), "the click selected the path");
    let before = all_paths(&h).len();
    h.key_press(Key::Delete);
    h.run_steps(3);
    assert_eq!(all_paths(&h).len(), before - 1, "the path's operators are gone from the page");
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(3);
    assert_eq!(all_paths(&h).len(), before);
    h.key_press(Key::Escape);
    h.run_steps(3);
    assert!(h.state().views[0].vector_edit.is_none());
}

/// A 60x30 frame, a leader from its right edge to (200, 60) and a small filled arrowhead there.
const CALLOUT: &str = "0 0 0 RG 1 w 20 120 60 30 re S 80 135 m 200 60 l S 0 g 200 60 m 190 62 l 194 69 l h f";

fn click(h: &mut Harness<'static, PdfCraftApp>, at: Pos2) {
    h.hover_at(at);
    h.run_steps(1);
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
    h.run_steps(1);
    h.event(Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
    h.run_steps(1);
}

fn points_of(path: &pdfcraft_engine::VectorPath) -> Vec<[f64; 2]> {
    path.segments
        .iter()
        .filter_map(|s| match s {
            pdfcraft_engine::PathSeg::MoveTo(p) | pdfcraft_engine::PathSeg::LineTo(p) => Some(*p),
            _ => None,
        })
        .collect()
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() < 3.0 && (a[1] - b[1]).abs() < 3.0
}

#[test]
fn a_callout_is_picked_up_whole_and_its_frame_drags_the_leader_along() {
    let mut h = harness_on(page_with(CALLOUT));
    drag_user(&mut h, [10.0, 100.0], [90.0, 160.0]);
    let state = h.state().views[0].vector_edit.clone().expect("picked up");
    assert_eq!(state.paths.len(), 3, "frame, leader, and the arrowhead past the rectangle");
    assert_eq!(state.connectors.len(), 1);
    let before = all_paths(&h);
    // Move the frame up by 40: a straight leader re-aims its tail at the frame's nearest edge point.
    drag_user(&mut h, [50.0, 135.0], [50.0, 175.0]);
    let after = all_paths(&h);
    assert_eq!(after.len(), 3);
    let leader = points_of(&after[1]);
    assert!(near(leader[0], [80.0, 160.0]), "tail on the moved frame's nearest point: {leader:?}");
    assert!(near(leader[1], [200.0, 60.0]), "the head stays where it points: {leader:?}");
    assert!(near(points_of(&after[2])[0], [200.0, 60.0]), "the arrowhead stays pinned");
    // The whole move is one undo step.
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(3);
    for (a, b) in all_paths(&h).iter().zip(&before) {
        assert_eq!(a.segments, b.segments);
    }
}

#[test]
fn a_dragged_middle_makes_a_sharp_corner_that_stays_when_the_frame_moves_and_a_triple_click_straightens() {
    let mut h = harness_on(page_with(CALLOUT));
    drag_user(&mut h, [10.0, 100.0], [90.0, 160.0]);
    // Select the leader, then drag its middle handle up.
    let mid = at(&h, 140.0, 97.5);
    click(&mut h, mid);
    assert!(h.state().views[0].vector_edit.as_ref().is_some_and(|s| s.selected == Some(1)), "the click selected the leader");
    drag_user(&mut h, [140.0, 97.5], [140.0, 150.0]);
    let bent = points_of(&all_paths(&h)[1]);
    assert_eq!(bent.len(), 3, "{bent:?}");
    assert!(near(bent[1], [140.0, 150.0]), "{bent:?}");
    assert!(near(bent[0], [80.0, 135.0]) && near(bent[2], [200.0, 60.0]));
    // Moving the frame down keeps the corner and the head; the tail goes with the frame.
    drag_user(&mut h, [50.0, 135.0], [50.0, 115.0]);
    let moved = points_of(&all_paths(&h)[1]);
    assert_eq!(moved.len(), 3, "{moved:?}");
    assert!(near(moved[0], [80.0, 115.0]) && near(moved[1], [140.0, 150.0]) && near(moved[2], [200.0, 60.0]), "{moved:?}");
    // Three clicks on the bent leader take the corner out.
    let on_piece = at(&h, 95.0, 123.75);
    h.hover_at(on_piece);
    h.run_steps(1);
    for _ in 0..3 {
        h.event(Event::PointerButton { pos: on_piece, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE });
        h.run_steps(1);
        h.event(Event::PointerButton { pos: on_piece, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE });
        h.run_steps(1);
    }
    h.run_steps(3);
    let straight = points_of(&all_paths(&h)[1]);
    assert_eq!(straight.len(), 2, "{straight:?}");
    assert!(near(straight[0], [80.0, 115.0]) && near(straight[1], [200.0, 60.0]), "{straight:?}");
}

#[test]
fn more_than_100_lines_in_a_rectangle_are_refused_with_a_message() {
    let content: String = (0..101).map(|i| format!("{} 10 m {} 20 l S ", 4.0 + f64::from(i) * 2.8, 4.0 + f64::from(i) * 2.8)).collect();
    let mut h = harness_on(page_with(&content));
    drag_user(&mut h, [2.0, 198.0], [298.0, 2.0]);
    assert!(h.state().views[0].vector_edit.is_none(), "nothing is picked up");
    let said = h.state().toast.as_ref().map(|t| t.0.clone()).unwrap_or_default();
    assert!(said.contains("more than 100") && said.contains("101"), "{said}");
    // A smaller rectangle of the same page works.
    drag_user(&mut h, [2.0, 198.0], [40.0, 2.0]);
    assert!(h.state().views[0].vector_edit.as_ref().is_some_and(|s| !s.paths.is_empty()));
}

//! Edit ▸ Edit vector lines in the real interface: drag a rectangle over line art, drag one of its
//! points, delete a path with the keyboard, and undo each as one step.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use pdfcraft_ui_egui::{PdfCraftApp, QuickTool};

/// One 300×200 page drawing an L-shaped red line and a separate green one.
fn lines() -> Vec<u8> {
    let content = "1 0 0 RG 2 w 50 50 m 150 50 l 150 120 l S 0 1 0 RG 200 30 m 280 30 l S";
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
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).with_step_dt(1.0 / 60.0).build_eframe(move |_cc| {
        let mut app = PdfCraftApp::new();
        app.set_option("language", "en").unwrap();
        app.open_bytes("lines.pdf", None, lines()).expect("opens");
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

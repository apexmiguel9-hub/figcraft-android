use egui::{Rect, pos2, vec2};

use super::loupe_rect;

#[test]
fn the_magnifier_goes_above_the_finger_and_stays_on_the_canvas() {
    let canvas = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 500.0));
    let r = loupe_rect(pos2(500.0, 300.0), canvas, 150.0);
    assert!(r.bottom() < 300.0, "above the finger: {r:?}");
    assert!(canvas.contains_rect(r), "inside the canvas: {r:?}");

    // Near the top there is no room above: it goes below instead.
    let r = loupe_rect(pos2(500.0, 20.0), canvas, 150.0);
    assert!(r.top() > 20.0, "below the finger: {r:?}");
    assert!(canvas.contains_rect(r), "inside the canvas: {r:?}");

    // Near an edge it is pushed back inside.
    for at in [pos2(10.0, 300.0), pos2(990.0, 300.0), pos2(10.0, 10.0), pos2(990.0, 490.0)] {
        let r = loupe_rect(at, canvas, 150.0);
        assert!(canvas.contains_rect(r), "at {at:?}: {r:?}");
    }
}

#[test]
fn the_magnifier_shrinks_to_fit_a_small_canvas() {
    let canvas = Rect::from_min_size(pos2(0.0, 0.0), vec2(80.0, 60.0));
    let r = loupe_rect(pos2(40.0, 30.0), canvas, 150.0);
    assert!(canvas.contains_rect(r), "{r:?}");
    assert!(r.width() < 80.0 && r.height() < 60.0, "{r:?}");
}

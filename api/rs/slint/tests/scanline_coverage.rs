// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use slint::platform::software_renderer::{
    LineBufferProvider, MinimalSoftwareWindow, RenderingRotation, RepaintBufferType, Rgb565Pixel,
};
use slint::platform::{PlatformError, WindowAdapter};
use std::{cell::RefCell, rc::Rc};
thread_local! { static NEXT: RefCell<Option<Rc<dyn WindowAdapter>>> = RefCell::new(None); }
struct Platform;
impl slint::platform::Platform for Platform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(NEXT.with(|next| next.borrow_mut().take().unwrap()))
    }
}
slint::slint! {
    export component Ui inherits Window {
        width: 96px; height: 96px; background: #102030;
        in property <int> step;
        Rectangle { x: 0; y: root.step * 2px; width: 96px; height: 13px; background: red; }
        Rectangle { x: 17px; y: root.step * 3px; width: 50px; height: 42px; background: green; }
        if mod(root.step, 2) == 0: Rectangle {
            x: 0; y: 50px - root.step * 1px; width: 96px; height: 18px; background: blue;
        }
        Rectangle { x: 10px; y: 4px; width: 60px; height: 80px; background: #ffff0080; }
        Rectangle {
            x: 5px; y: 8px; width: 70px; height: 80px; clip: true;
            Rectangle { x: -10px; y: root.step * 4px; width: 90px; height: 10px; background: #ff00ff; }
        }
    }
}
struct Lines<'a> {
    pixels: &'a mut [Rgb565Pixel],
    touched: usize,
}
impl LineBufferProvider for &mut Lines<'_> {
    type TargetPixel = Rgb565Pixel;
    fn process_line(
        &mut self,
        line: usize,
        range: std::ops::Range<usize>,
        draw: impl FnOnce(&mut [Rgb565Pixel]),
    ) {
        self.touched += range.len();
        draw(&mut self.pixels[line * 96 + range.start..line * 96 + range.end]);
    }
}
#[test]
fn scanlines_match_independent_full_frame_rendering() {
    slint::platform::set_platform(Box::new(Platform)).unwrap();
    for rotation in [
        RenderingRotation::NoRotation,
        RenderingRotation::Rotate90,
        RenderingRotation::Rotate180,
        RenderingRotation::Rotate270,
    ] {
        let partial = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        NEXT.with(|next| *next.borrow_mut() = Some(partial.clone()));
        let ui_partial = Ui::new().unwrap();
        let full = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        NEXT.with(|next| *next.borrow_mut() = Some(full.clone()));
        let ui_full = Ui::new().unwrap();
        for window in [&partial, &full] {
            window.set_size(slint::PhysicalSize::new(96, 96));
        }
        ui_partial.show().unwrap();
        ui_full.show().unwrap();
        let mut a = vec![Rgb565Pixel(0); 96 * 96];
        let mut b = a.clone();
        let mut partial_frames = 0;
        for step in 0..16 {
            ui_partial.set_step(step);
            ui_full.set_step(step);
            partial.draw_if_needed(|renderer| {
                if step == 0 {
                    renderer.set_rendering_rotation(rotation);
                }
                let mut lines = Lines { pixels: &mut a, touched: 0 };
                renderer.render_by_line(&mut lines);
                if lines.touched > 0 && lines.touched < 96 * 96 {
                    partial_frames += 1;
                }
            });
            full.draw_if_needed(|renderer| {
                if step == 0 {
                    renderer.set_rendering_rotation(rotation);
                }
                renderer.render(&mut b, 96);
            });
            let first = a.iter().zip(&b).position(|(a, b)| a.0 != b.0);
            assert!(first.is_none(), "step {step}, first differing pixel {first:?}");
        }
        assert!(partial_frames > 0, "partial rendering was not exercised");
    }
}

// Copyright © SixtyFPS GmbH <info@slint.dev>

// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use slint::ComponentHandle;
use slint::platform::software_renderer::{
    LineBufferProvider, MinimalSoftwareWindow, RenderingRotation, RepaintBufferType,
    Rgb565BigEndianPixel,
};
use slint::platform::{PlatformError, WindowAdapter};
use std::{cell::RefCell, rc::Rc};
thread_local! { static NEXT: RefCell<Option<Rc<dyn WindowAdapter>>> = RefCell::default(); }

struct Platform;

impl slint::platform::Platform for Platform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(NEXT.with(|n| n.borrow_mut().take().unwrap()))
    }
}
slint::include_modules!();

struct Lines<'a> {
    pixels: &'a mut [Rgb565BigEndianPixel],
}

impl LineBufferProvider for Lines<'_> {
    type TargetPixel = Rgb565BigEndianPixel;
    fn process_line(
        &mut self,
        line: usize,
        range: std::ops::Range<usize>,
        render: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        render(&mut self.pixels[line * 240 + range.start..line * 240 + range.end]);
    }
}

fn compare_cached_and_uncached(repaint: RepaintBufferType) {
    slint::platform::set_platform(Box::new(Platform)).unwrap();
    let mut saw_cache = false;
    let mut saw_hit = false;
    let mut saw_mask = false;
    for rotation in [
        RenderingRotation::NoRotation,
        RenderingRotation::Rotate90,
        RenderingRotation::Rotate180,
        RenderingRotation::Rotate270,
    ] {
        let mut windows = Vec::new();
        let mut uis = Vec::new();
        for _ in 0..2 {
            let w = MinimalSoftwareWindow::new(repaint);
            NEXT.with(|n| *n.borrow_mut() = Some(w.clone()));
            let ui = Ui::new().unwrap();
            w.set_size(slint::PhysicalSize::new(240, 240));
            ui.show().unwrap();
            windows.push(w);
            uis.push(ui);
        }
        let mut images = vec![vec![Rgb565BigEndianPixel::default(); 240 * 240]; 2];
        for step in 0..180 {
            for ui in &uis {
                ui.set_dy(-72.75 + (step % 80) as f32 * 2.25);
                ui.set_dx([-50.5, -1.5, 0.5, 7.5, 31.25][step % 5]);
                ui.set_spacing([-2.0, 0.0, 1.0, 2.0][step % 4]);
                ui.set_line_factor([0.0, 0.5, 1.0, 1.5, 2.0][step % 5]);
                ui.set_label(if step < 25 {
                    "Pocket mode".into()
                } else if step < 130 {
                    format!("AVATAR\nLabel {} with wrapping and elision\nPocket mode", step / 3)
                        .into()
                } else {
                    "x".repeat(256).into()
                });
                ui.set_ink(if step % 13 < 6 {
                    slint::Color::from_rgb_u8(240, 70, 80)
                } else {
                    slint::Color::from_rgb_u8(70, 230, 140)
                });
                ui.set_row_fill(if step % 11 < 5 {
                    slint::Color::from_rgb_u8(20, 55, 85)
                } else {
                    slint::Color::from_rgb_u8(100, 25, 45)
                });
                ui.set_size(if step % 17 < 8 { 23.0 } else { 29.0 });
                ui.set_row_width(if step % 11 < 5 { 178.0 } else { 87.0 });
                ui.set_fade(if step % 19 < 9 { 1.0 } else { 0.45 });
                ui.set_italic(step % 23 < 11);
                ui.set_wrap(step % 7 < 3);
                ui.set_right(step % 9 < 4);
                ui.set_limit(if step % 5 < 2 { 1 } else { 2 });
            }
            for (i, w) in windows.iter().enumerate() {
                let pixels = &mut images[i];
                w.draw_if_needed(|r| {
                    if step == 0 {
                        r.set_rendering_rotation(rotation);
                        r.set_embedded_text_cache_enabled(i == 0);
                    }
                    r.render_by_line(Lines { pixels });
                    let (entries, bytes) = r.embedded_text_cache_stats();
                    assert!(entries <= 24 && bytes <= 8192);
                    let (masks, mask_bytes) = r.embedded_text_mask_stats();
                    assert!(masks <= entries && mask_bytes <= 65536);
                    saw_mask |= masks > 0;
                    saw_cache |= entries > 0;
                    saw_hit |= r.embedded_text_cache_hits() > 0;
                    if i == 1 {
                        assert_eq!(entries, 0);
                    }
                });
            }
            assert!(images[0] == images[1], "pixel mismatch at {rotation:?}, step {step}");
        }
        for ui in &uis {
            ui.hide().unwrap();
        }
        drop(uis);
        for w in windows {
            w.request_redraw();
            w.draw_if_needed(|r| {
                assert_eq!(r.embedded_text_cache_stats(), (0, 0));
                assert_eq!(r.embedded_text_mask_stats(), (0, 0));
            });
        }
    }
    if !cfg!(feature = "caches") || cfg!(any(feature = "vector-fonts", feature = "sdf-fonts")) {
        assert!(
            !saw_cache && !saw_hit && !saw_mask,
            "disabled or unsupported text entered the bitmap cache"
        );
    } else {
        assert!(saw_cache, "embedded-font cache path was never exercised");
        assert!(saw_hit, "no cached layout was reused");
        assert!(saw_mask, "no rasterized text mask was used");
    }
}

#[test]
fn cached_and_uncached_full_pixels_match() {
    compare_cached_and_uncached(RepaintBufferType::NewBuffer);
}

#[test]
fn cached_and_uncached_partial_pixels_match() {
    compare_cached_and_uncached(RepaintBufferType::ReusedBuffer);
}

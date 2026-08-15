// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! A dial screen whose value climbs, rendered twice in lockstep: once through the partial
//! renderer that reuses its buffer and once with a full repaint every frame. The two
//! buffers must stay identical. Any divergence is a pixel the narrowed invalidation owed
//! and never paid, which on hardware shows up as a gap in the arc.
//!
//! Both passes go through `render_by_line`, because that is the path the firmware runs;
//! `SoftwareRenderer::render` takes a different route through `foreach_ranges`.

use slint::platform::software_renderer::{
    LineBufferProvider, MinimalSoftwareWindow, RenderingRotation, RepaintBufferType,
    Rgb565BigEndianPixel, Rgb565Pixel, TargetPixel,
};
use slint::platform::{PlatformError, WindowAdapter};
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    static NEXT_WINDOW_CHOICE: Rc<RefCell<Option<Rc<dyn WindowAdapter>>>> =
        Rc::new(RefCell::new(None));
}

struct TestPlatform;
impl slint::platform::Platform for TestPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(NEXT_WINDOW_CHOICE
            .with(|choice| choice.borrow_mut().take())
            .expect("a window must be chosen before creating a component"))
    }
}

const W: usize = 466;
const H: usize = 466;

slint::slint! {
    // One gauge, factored so the screen below can hold several at different geometry.
    component Gauge inherits Rectangle {
        in property <float> q: 0.0;
        in property <length> cx;
        in property <length> cy;
        in property <length> r;
        in property <length> thickness;
        in property <angle> start-angle: 135deg;
        in property <angle> span-angle: 270deg;
        in property <color> track: #202020;
        in property <color> indicator: #00ff00;

        // Track: static, with the indicator drawn over it. Overlapping arcs are what the
        // real dial does, and a narrowed band has to restore the track under the tip.
        Path {
            width: 100%;
            height: 100%;
            fit: preserve;
            arc-center-x: root.cx;
            arc-center-y: root.cy;
            arc-radius: root.r;
            arc-start-angle: root.start-angle;
            arc-sweep-angle: root.span-angle;
            stroke: root.track;
            stroke-width: root.thickness;
            stroke-line-cap: round;
        }
        Path {
            width: 100%;
            height: 100%;
            fit: preserve;
            arc-center-x: root.cx;
            arc-center-y: root.cy;
            arc-radius: root.r;
            arc-start-angle: root.start-angle;
            arc-sweep-angle: root.span-angle * root.q;
            stroke: root.indicator;
            stroke-width: root.thickness;
            stroke-line-cap: round;
        }
    }

    export component Ui inherits Window {
        in property <float> q: 0.0;
        background: #000000;

        // Mirrors the stats screen: a full-size outer dial, an inset dial offset from the
        // window origin (the geometry that exposed the double-counted item offset), and a
        // text that changes every frame so the dirty region carries more than one rect.
        // The centres are deliberately fractional - integer geometry hid this class of bug
        // from five earlier test suites.
        Gauge {
            x: 0phx; y: 0phx; width: 100%; height: 100%;
            q: root.q;
            cx: 233.5phx; cy: 233.5phx; r: 200phx; thickness: 18phx;
        }
        Gauge {
            x: 150phx; y: 260phx; width: 160phx; height: 160phx;
            q: 1.0 - root.q;
            cx: 80.25phx; cy: 80.75phx; r: 62phx; thickness: 11phx;
            indicator: #ff8800;
            start-angle: 90deg;
            span-angle: 300deg;
        }
        Text {
            x: 180phx; y: 200phx;
            text: Math.round(root.q * 100) + "%";
            color: #ffffff;
            font-size: 40phx;
        }
    }
}

/// The device renders `Rgb565BigEndianPixel` so the panel needs no byte swap, and that type
/// has its own `blend_slice`/`blend`. Narrowed spans are short, odd-length and arbitrarily
/// aligned where full-width spans are long, even and aligned, so the two pixel types have to
/// be exercised separately.
trait TestPixel: TargetPixel + Copy + 'static {
    fn zero() -> Self;
    /// The native-endian 565 word, so both types compare on the same scale.
    fn logical(self) -> u16;
    fn name() -> &'static str;
}

impl TestPixel for Rgb565Pixel {
    fn zero() -> Self {
        Rgb565Pixel(0)
    }
    fn logical(self) -> u16 {
        self.0
    }
    fn name() -> &'static str {
        "le"
    }
}

impl TestPixel for Rgb565BigEndianPixel {
    fn zero() -> Self {
        Rgb565BigEndianPixel(0)
    }
    fn logical(self) -> u16 {
        u16::from_be(self.0)
    }
    fn name() -> &'static str {
        "be"
    }
}

/// Writes each line the renderer hands it into a full-screen buffer, which is exactly what
/// slint-esp.cpp does.
struct LineWriter<'a, P: TestPixel> {
    buffer: &'a mut [P],
    /// Every (line, range) the renderer asked for, so the test can tell a line that was
    /// never offered from one that was offered and painted wrong.
    touched: Vec<(usize, core::ops::Range<usize>)>,
}

impl<P: TestPixel> LineBufferProvider for &mut LineWriter<'_, P> {
    type TargetPixel = P;

    fn process_line(
        &mut self,
        line: usize,
        range: core::ops::Range<usize>,
        render_fn: impl FnOnce(&mut [Self::TargetPixel]),
    ) {
        self.touched.push((line, range.clone()));
        render_fn(&mut self.buffer[line * W + range.start..line * W + range.end]);
    }
}

/// Splits an Rgb565 word into channels so differences can be judged by magnitude, with
/// green scaled to the same 0..31 range as red and blue.
fn channels(p: u16) -> (i32, i32, i32) {
    (((p >> 11) & 0x1f) as i32, (((p >> 5) & 0x3f) / 2) as i32, (p & 0x1f) as i32)
}

fn max_delta(a: u16, b: u16) -> i32 {
    let (ar, ag, ab) = channels(a);
    let (br, bg, bb) = channels(b);
    (ar - br).abs().max((ag - bg).abs()).max((ab - bb).abs())
}

struct Diff {
    count: usize,
    visible: usize,
    worst: i32,
    first: (usize, usize),
    first_visible: Option<(usize, usize)>,
    bbox: (usize, usize, usize, usize),
}

fn compare<P: TestPixel>(partial: &[P], full: &[P]) -> Option<Diff> {
    let mut d = Diff {
        count: 0,
        visible: 0,
        worst: 0,
        first: (0, 0),
        first_visible: None,
        bbox: (usize::MAX, usize::MAX, 0, 0),
    };
    for y in 0..H {
        for x in 0..W {
            let (p, f) = (partial[y * W + x].logical(), full[y * W + x].logical());
            if p == f {
                continue;
            }
            let delta = max_delta(p, f);
            if d.count == 0 {
                d.first = (x, y);
            }
            d.count += 1;
            d.worst = d.worst.max(delta);
            // Two levels out of 31 is about 6% of full scale, well below what is visible on
            // the panel and far below a gap showing the background through the arc.
            if delta > 2 {
                d.visible += 1;
                d.first_visible.get_or_insert((x, y));
            }
            d.bbox.0 = d.bbox.0.min(x);
            d.bbox.1 = d.bbox.1.min(y);
            d.bbox.2 = d.bbox.2.max(x);
            d.bbox.3 = d.bbox.3.max(y);
        }
    }
    (d.count > 0).then_some(d)
}

struct Divergence {
    step: usize,
    q: f32,
    diff: Diff,
    /// Whether the renderer offered the first diverging pixel to `process_line` this frame.
    /// This is the whole diagnosis: not offered means the invalidation never asked for the
    /// pixel, offered means the invalidation was right and the drawing was wrong.
    first_was_offered: bool,
    partial_px: u16,
    full_px: u16,
    partial_ranges: Vec<core::ops::Range<usize>>,
    full_ranges: Vec<core::ops::Range<usize>>,
}

fn run<P: TestPixel>(
    steps: impl Iterator<Item = f32>,
    rotation: RenderingRotation,
) -> (Vec<Divergence>, u64) {
    slint::platform::set_platform(Box::new(TestPlatform)).ok();

    let win_partial = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
    NEXT_WINDOW_CHOICE.with(|c| *c.borrow_mut() = Some(win_partial.clone()));
    let ui_partial = Ui::new().unwrap();

    let win_full = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    NEXT_WINDOW_CHOICE.with(|c| *c.borrow_mut() = Some(win_full.clone()));
    let ui_full = Ui::new().unwrap();

    for w in [&win_partial, &win_full] {
        w.set_size(slint::PhysicalSize::new(W as u32, H as u32));
    }
    ui_partial.show().unwrap();
    ui_full.show().unwrap();

    let mut buf_p = vec![P::zero(); W * H];
    let mut buf_f = vec![P::zero(); W * H];
    let mut out = Vec::new();
    let mut partial_lines: Vec<usize> = Vec::new();

    for (step, q) in steps.enumerate() {
        ui_partial.set_q(q);
        ui_full.set_q(q);

        let mut lines_p = 0usize;
        let mut touched_p: Vec<(usize, core::ops::Range<usize>)> = Vec::new();
        let mut touched_f: Vec<(usize, core::ops::Range<usize>)> = Vec::new();
        let first_frame = step == 0;

        win_partial.draw_if_needed(|r| {
            // The panel drivers run with SW_ROTATE, so the device never renders unrotated.
            // Set once: doing it every frame re-dirties the whole screen and would quietly
            // turn off the narrowing this test exists to exercise.
            if first_frame {
                r.set_rendering_rotation(rotation);
            }
            let mut w = LineWriter { buffer: buf_p.as_mut_slice(), touched: Vec::new() };
            r.render_by_line(&mut w);
            lines_p = w.touched.len();
            touched_p = w.touched;
        });
        // NewBuffer repaints everything every frame, so this is the reference.
        win_full.draw_if_needed(|r| {
            if first_frame {
                r.set_rendering_rotation(rotation);
            }
            let mut w = LineWriter { buffer: buf_f.as_mut_slice(), touched: Vec::new() };
            r.render_by_line(&mut w);
            touched_f = w.touched;
        });
        partial_lines.push(lines_p);

        // The panel needs every draw-window coordinate even, so the region handed to the
        // firmware has to be even on both axes. x is visible directly in the ranges; y shows
        // up as the runs of consecutive lines, which must start even and have an even length.
        for (l, r) in &touched_p {
            assert_eq!(r.start % 2, 0, "odd range start {} on line {l} at step {step}", r.start);
            assert_eq!(r.end % 2, 0, "odd range end {} on line {l} at step {step}", r.end);
        }
        let mut lines: Vec<usize> = touched_p.iter().map(|(l, _)| *l).collect();
        lines.sort_unstable();
        lines.dedup();
        let mut i = 0;
        while i < lines.len() {
            let start = lines[i];
            let mut end = start;
            while i + 1 < lines.len() && lines[i + 1] == end + 1 {
                i += 1;
                end = lines[i];
            }
            i += 1;
            assert_eq!(start % 2, 0, "line run {start}..={end} starts odd at step {step}");
            assert_eq!((end - start + 1) % 2, 0, "line run {start}..={end} has odd height at step {step}");
        }

        if let Some(diff) = compare(&buf_p, &buf_f) {
            let first = diff.first;
            let rng = |t: &Vec<(usize, core::ops::Range<usize>)>| {
                t.iter().filter(|(l, _)| *l == first.1).map(|(_, r)| r.clone()).collect::<Vec<_>>()
            };
            out.push(Divergence {
                step,
                q,
                first_was_offered: touched_p
                    .iter()
                    .any(|(l, r)| *l == first.1 && r.contains(&first.0)),
                partial_px: buf_p[first.1 * W + first.0].logical(),
                full_px: buf_f[first.1 * W + first.0].logical(),
                partial_ranges: rng(&touched_p),
                full_ranges: rng(&touched_f),
                diff,
            });
        }
    }

    // Guard against a vacuous pass: if the partial renderer repainted every line every
    // frame there was no narrowing to test, and matching the reference proves nothing.
    let full_frames = partial_lines.iter().skip(2).filter(|&&n| n >= H).count();
    let total = partial_lines.len().saturating_sub(2);
    assert!(
        full_frames * 2 < total,
        "partial renderer repainted the whole screen on {full_frames} of {total} frames - narrowing is not active, so this test proves nothing"
    );

    let checksum = buf_f
        .iter()
        .enumerate()
        .fold(0u64, |acc, (i, p)| acc.wrapping_mul(31).wrapping_add((p.logical() as u64) ^ (i as u64)));
    (out, checksum)
}

fn report(divergences: &[Divergence], label: &str, steps: usize) {
    let cosmetic = divergences.iter().filter(|d| d.diff.visible == 0).count();
    let worst = divergences.iter().map(|d| d.diff.worst).max().unwrap_or(0);
    if cosmetic > 0 {
        println!(
            "[{label}] {cosmetic} of {steps} steps differ only below the visible threshold (worst channel delta {worst}/31)"
        );
    }
    let Some(d) = divergences.iter().find(|d| d.diff.visible > 0) else { return };
    panic!(
        "[{label}] partial render diverged visibly from a full repaint at step {} (q={:.4}): \
         {} pixels differ, {} of them visibly, worst channel delta {}/31.\n\
         first visible at {:?}; first differing at ({},{}) offered_to_process_line={}\n\
         bbox ({},{})-({},{}); {} of {steps} steps diverged\n\
         partial px=0x{:04x} full px=0x{:04x}\n\
         partial ranges for that line: {:?}\n\
         full    ranges for that line: {:?}",
        d.step,
        d.q,
        d.diff.count,
        d.diff.visible,
        d.diff.worst,
        d.diff.first_visible,
        d.diff.first.0,
        d.diff.first.1,
        d.first_was_offered,
        d.diff.bbox.0,
        d.diff.bbox.1,
        d.diff.bbox.2,
        d.diff.bbox.3,
        divergences.len(),
        d.partial_px,
        d.full_px,
        d.partial_ranges,
        d.full_ranges,
    );
}

const ROTATIONS: [(RenderingRotation, &str); 4] = [
    (RenderingRotation::NoRotation, "0"),
    (RenderingRotation::Rotate90, "90"),
    (RenderingRotation::Rotate180, "180"),
    (RenderingRotation::Rotate270, "270"),
];

#[test]
fn climbing_arc_partial_matches_full_repaint() {
    // 400 steps over the full sweep: fine enough that consecutive frames differ by well
    // under a pixel of arc length near the tip, which is the regime the artifact appears in.
    let mut sums: Vec<(&str, u64)> = Vec::new();
    for (rot, name) in ROTATIONS {
        let (d, sum) = run::<Rgb565Pixel>((0..=400).map(|i| i as f32 / 400.0), rot);
        report(&d, &format!("smooth le rot={name}"), 401);
        let (d, _) = run::<Rgb565BigEndianPixel>((0..=400).map(|i| i as f32 / 400.0), rot);
        report(&d, &format!("smooth be rot={name}"), 401);
        sums.push((name, sum));
    }
    // Prove the rotation actually reached the renderer. If every orientation produced the
    // same pixels, three quarters of this matrix was testing nothing.
    for w in sums.windows(2) {
        assert_ne!(
            w[0].1, w[1].1,
            "rotations {} and {} rendered identical buffers - rotation never took effect",
            w[0].0, w[1].0
        );
    }
}

#[test]
fn stalling_arc_partial_matches_full_repaint() {
    // The device does not get a frame per value change. When rendering stalls and catches
    // up, one frame's sweep covers what several would have, which is the regime the
    // reported artifact appears in: "it stalls, then catches up but doesn't render the
    // updates it missed".
    for (rot, name) in ROTATIONS {
        for jump in [3usize, 7, 17, 40] {
            let (d, _) = run::<Rgb565Pixel>((0..=400).step_by(jump).map(|i| i as f32 / 400.0), rot);
            report(&d, &format!("jump={jump} le rot={name}"), 400 / jump + 1);
            let (d, _) =
                run::<Rgb565BigEndianPixel>((0..=400).step_by(jump).map(|i| i as f32 / 400.0), rot);
            report(&d, &format!("jump={jump} be rot={name}"), 400 / jump + 1);
        }
    }
}

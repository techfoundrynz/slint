// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

/*!
This module contains the builtin Path related items.

When adding an item or a property, it needs to be kept in sync with different place.
Lookup the [`crate::items`] module documentation.
*/

use super::{
    FillRule, Item, ItemConsts, ItemRc, ItemRendererRef, LineCap, LineJoin, RenderingResult,
};
use crate::graphics::{Brush, FittedPath, PathData, PathDataIterator};
use crate::input::{
    FocusEvent, FocusEventResult, InputEventFilterResult, InputEventResult, InternalKeyEvent,
    KeyEventResult, MouseEvent,
};
use crate::item_rendering::CachedRenderingData;

use crate::items::ImageFit;
use crate::layout::{LayoutInfo, Orientation};
use crate::lengths::{
    LogicalBorderRadius, LogicalLength, LogicalPx, LogicalRect, LogicalSize, LogicalVector,
    RectLengths,
};
#[cfg(feature = "rtti")]
use crate::rtti::*;
use crate::window::WindowAdapter;
use crate::{Coord, Property};
use alloc::boxed::Box;
use alloc::rc::Rc;
use const_field_offset::FieldOffsets;
use core::cell::RefCell;
use core::pin::Pin;
use euclid::Point2D;
use euclid::num::Zero;
use i_slint_core_macros::*;
#[cfg(not(feature = "std"))]
use num_traits::Float;

/// The implementation of the `Path` element
#[repr(C)]
#[derive(FieldOffsets, Default, SlintElement)]
#[pin]
pub struct Path {
    pub elements: Property<PathData>,
    pub fill: Property<Brush>,
    pub fill_rule: Property<FillRule>,
    pub stroke: Property<Brush>,
    pub stroke_width: Property<LogicalLength>,
    pub stroke_line_cap: Property<LineCap>,
    pub stroke_line_join: Property<LineJoin>,
    pub stroke_miter_limit: Property<f32>,
    /// When `arc_radius` is greater than zero this path is a stroked circular arc, and a
    /// renderer that can draw one directly may use these instead of stroking `elements`.
    pub arc_center_x: Property<LogicalLength>,
    pub arc_center_y: Property<LogicalLength>,
    pub arc_radius: Property<LogicalLength>,
    pub arc_start_angle: Property<f32>,
    pub arc_sweep_angle: Property<f32>,
    pub viewbox_x: Property<f32>,
    pub viewbox_y: Property<f32>,
    pub viewbox_width: Property<f32>,
    pub viewbox_height: Property<f32>,
    pub fit: Property<ImageFit>,
    pub clip: Property<bool>,
    pub anti_alias: Property<bool>,
    pub cached_rendering_data: CachedRenderingData,
    fitted_path: FittedPathBox,
    tracker: crate::properties::PropertyTracker,
    /// Last arc geometry a dirty region was computed for, so that a sweep that only grew
    /// or shrank can invalidate the difference instead of the whole element.
    arc_snapshot: ArcSnapshotCell,
}

impl Item for Path {
    fn init(self: Pin<&Self>, _self_rc: &ItemRc) {}

    fn deinit(self: Pin<&Self>, _window_adapter: &Rc<dyn WindowAdapter>) {}

    fn layout_info(
        self: Pin<&Self>,
        _orientation: Orientation,
        _cross_axis_constraint: Coord,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
    ) -> LayoutInfo {
        LayoutInfo { stretch: 1., ..LayoutInfo::default() }
    }

    fn input_event_filter_before_children(
        self: Pin<&Self>,
        _: &MouseEvent,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
        _: &mut super::MouseCursorInner,
    ) -> InputEventFilterResult {
        InputEventFilterResult::ForwardAndIgnore
    }

    fn input_event(
        self: Pin<&Self>,
        _: &MouseEvent,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
        _: &mut super::MouseCursorInner,
    ) -> InputEventResult {
        InputEventResult::EventIgnored
    }

    fn capture_key_event(
        self: Pin<&Self>,
        _: &InternalKeyEvent,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
    ) -> KeyEventResult {
        KeyEventResult::EventIgnored
    }

    fn key_event(
        self: Pin<&Self>,
        _: &InternalKeyEvent,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
    ) -> KeyEventResult {
        KeyEventResult::EventIgnored
    }

    fn focus_event(
        self: Pin<&Self>,
        _: &FocusEvent,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
    ) -> FocusEventResult {
        FocusEventResult::FocusIgnored
    }

    fn render(
        self: Pin<&Self>,
        backend: &mut ItemRendererRef,
        self_rc: &ItemRc,
        size: LogicalSize,
    ) -> RenderingResult {
        let clip = self.clip();
        if clip {
            (*backend).save_state();
            (*backend).combine_clip(size.into(), LogicalBorderRadius::zero());
        }
        (*backend).draw_path(self, self_rc, size);
        if clip {
            (*backend).restore_state();
        }
        RenderingResult::ContinueRenderingChildren
    }

    fn bounding_rect(
        self: core::pin::Pin<&Self>,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
        geometry: LogicalRect,
    ) -> LogicalRect {
        geometry
    }

    fn clips_children(self: core::pin::Pin<&Self>) -> bool {
        false
    }
}



impl Path {
    /// Returns an iterator of the events of the path and an offset, so that the
    /// shape fits into the width/height of the path while respecting the stroke
    /// width.
    pub fn fitted_path_events(
        self: Pin<&Self>,
        self_rc: &ItemRc,
    ) -> Option<(LogicalVector, PathDataIterator)> {
        let mut elements_iter = self.elements().iter()?;

        let stroke_width = self.stroke_width();
        let geometry = self_rc.geometry();
        let bounds_width = (geometry.width_length() - stroke_width).max(LogicalLength::zero());
        let bounds_height = (geometry.height_length() - stroke_width).max(LogicalLength::zero());
        let offset =
            LogicalVector::from_lengths(stroke_width / 2 as Coord, stroke_width / 2 as Coord);

        let viewbox_width = self.viewbox_width();
        let viewbox_height = self.viewbox_height();

        let maybe_viewbox = if viewbox_width > 0. && viewbox_height > 0. {
            Some(
                euclid::rect(self.viewbox_x(), self.viewbox_y(), viewbox_width, viewbox_height)
                    .to_box2d(),
            )
        } else {
            None
        };

        elements_iter.fit(
            bounds_width.get() as _,
            bounds_height.get() as _,
            maybe_viewbox,
            self.fit(),
        );
        (offset, elements_iter).into()
    }

    fn sample_at(
        self: Pin<&Self>,
        self_rc: &ItemRc,
        t: f32,
    ) -> Option<(Point2D<f32, LogicalPx>, f32)> {
        if let Some(new_path) =
            Path::FIELD_OFFSETS.tracker().apply_pin(self).evaluate_if_dirty(|| {
                let (offset, elements_iter) = self.fitted_path_events(self_rc)?;
                Some(elements_iter.to_fitted_path(offset))
            })
        {
            *self.fitted_path.borrow_mut() = new_path;
        }
        self.fitted_path.borrow().as_ref()?.sample_at(t)
    }

    pub fn point_at(self: Pin<&Self>, self_rc: &ItemRc, t: f32) -> Point2D<f32, LogicalPx> {
        self.sample_at(self_rc, t).map(|(pos, _)| pos).unwrap_or_default()
    }
    pub fn angle_at(self: Pin<&Self>, self_rc: &ItemRc, t: f32) -> f32 {
        self.sample_at(self_rc, t).map(|(_, tangent)| tangent).unwrap_or_default()
    }
}

/// Everything about a stroked arc that affects the pixels it produces. Two snapshots that
/// differ only in `sweep` describe arcs whose drawn pixels are identical except over the
/// angles between the two sweeps.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq)]
struct ArcSnapshot {
    valid: bool,
    center_x: f32,
    center_y: f32,
    radius: f32,
    stroke_width: f32,
    start: f32,
    sweep: f32,
    stroke: u32,
    fill: u32,
    cap: u32,
    width: f32,
    height: f32,
}

/// The arc geometry the last dirty region was measured against.
#[repr(C)]
#[derive(Default)]
struct ArcSnapshotCell {
    last: core::cell::Cell<ArcSnapshot>,
}

/// How far outside its exact extremes the renderer can actually paint the arc, in pixels.
///
/// `annular_sector_bounds` is exact, but the software renderer truncates the centre and both
/// radii to `i16` when it builds the `ArcCommand`, shifting the figure by up to (1,1)px and
/// shortening each radius, and it anti-aliases half a pixel past every span end. Round caps
/// are discs on the same truncated centre line, and the angular cap allowance below cannot
/// absorb a linear truncation. That measures 2.8px on the ring and 5px at a cap;
/// `bounds_cover_what_the_renderer_actually_paints` pins the floor at 5.
const ARC_BAND_SLACK: Coord = 6 as Coord;

impl Path {
    /// The region to invalidate when this Path is dirty, if a narrower one than the whole
    /// element can be justified.
    ///
    /// Returns `Some(rect)` only when the arc is unchanged apart from its sweep, in which case
    /// every pixel outside the swept difference is identical to the previous frame. Anything
    /// else returns `None` and the caller invalidates the full bounding rect. Under-invalidating
    /// leaves stale pixels on screen indefinitely, so the comparison is deliberately total.
    pub fn arc_dirty_rect(self: Pin<&Self>, geometry: LogicalRect) -> Option<LogicalRect> {
        let _ = geometry;
        let now = self.arc_snapshot_now(geometry.size)?;
        let before = self.arc_snapshot.last.replace(now);

        // Only the two ends may differ, and neither by more than half a turn - past that the
        // band bounds nothing useful, as `lv_arc_set_start_angle` also decides. Anything else,
        // including the first frame, owes the whole element.
        let ends =
            [(before.start, now.start), (before.start + before.sweep, now.start + now.sweep)];
        if !before.valid
            || (ArcSnapshot { start: now.start, sweep: now.sweep, ..before }) != now
            || ends.iter().any(|(a, b)| (b - a).abs() > 180.)
        {
            return None;
        }

        let half = now.stroke_width / 2.;
        let radius = now.radius;
        // A round cap is a disc on the stroke's centre line, reaching past the end by
        // asin(half / radius); atan of the same ratio bounds it from above.
        let cap_slack = if radius > 0. { (half / radius).atan().to_degrees() } else { 0. } + 1.;
        let mut owed = LogicalRect::default();
        for (a, b) in ends {
            if a == b {
                continue;
            }
            let band = annular_sector_bounds(
                now.center_x,
                now.center_y,
                (radius - half).max(0.),
                radius + half,
                a.min(b) - cap_slack,
                a.max(b) + cap_slack,
            );
            owed = if owed.is_empty() { band } else { owed.union(&band) };
        }
        if owed.is_empty() {
            return None;
        }
        // Deliberately not translated by `geometry.origin`: the caller applies the same
        // transform it uses for the item's own bounding rect, which already carries the offset.
        Some(owed.inflate(ARC_BAND_SLACK, ARC_BAND_SLACK))
    }

    fn arc_snapshot_now(self: Pin<&Self>, size: LogicalSize) -> Option<ArcSnapshot> {
        let radius = self.arc_radius().get();
        if radius <= 0. {
            return None;
        }
        Some(ArcSnapshot {
            valid: true,
            center_x: self.arc_center_x().get(),
            center_y: self.arc_center_y().get(),
            radius,
            stroke_width: self.stroke_width().get(),
            start: self.arc_start_angle(),
            sweep: self.arc_sweep_angle(),
            stroke: self.stroke().color().as_argb_encoded(),
            fill: self.fill().color().as_argb_encoded(),
            cap: self.stroke_line_cap() as u32,
            width: size.width as f32,
            height: size.height as f32,
        })
    }

}

/// Bounding rectangle of the part of a ring between two angles, in degrees measured
/// clockwise from 3 o'clock.
fn annular_sector_bounds(
    cx: f32,
    cy: f32,
    inner: f32,
    outer: f32,
    from: f32,
    to: f32,
) -> LogicalRect {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    let mut include = |x: f32, y: f32| {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    };

    // Both ends of the stroke at both radii.
    for a in [from, to] {
        let (s, c) = (a.to_radians().sin(), a.to_radians().cos());
        for r in [inner, outer] {
            include(cx + r * c, cy + r * s);
        }
    }
    // Plus wherever the outer edge touches an axis inside the range, which is where the
    // extremes of a wide sector actually are.
    let span = to - from;
    for k in 0..=4 {
        let axis = 90. * k as f32;
        // Smallest non-negative rotation from `from` to this axis. Written out rather than
        // with rem_euclid, which is a std-only inherent method on f32.
        let raw = axis - from;
        let delta = raw - 360. * (raw / 360.).floor();
        if delta <= span {
            let (s, c) = (axis.to_radians().sin(), axis.to_radians().cos());
            include(cx + outer * c, cy + outer * s);
        }
    }

    LogicalRect::new(
        euclid::point2(min_x as Coord, min_y as Coord),
        euclid::size2((max_x - min_x) as Coord, (max_y - min_y) as Coord),
    )
    .inflate(ARC_BAND_SLACK, ARC_BAND_SLACK)
}

#[cfg(test)]
mod arc_dirty_tests {
    use super::{annular_sector_bounds, ARC_BAND_SLACK};
    use crate::lengths::LogicalSize;

    /// Every pixel the swept difference can touch must lie inside the rect we declare
    /// dirty. Anything outside it keeps last frame's pixels, so a gap here shows up as
    /// arc fragments left behind on screen.
    #[test]
    fn bounds_cover_the_swept_difference() {
        let (cx, cy, inner, outer) = (233.0f32, 233.0f32, 215.0f32, 233.0f32);
        // Starts chosen to straddle every axis, sweeps from a sliver to most of a turn.
        for start in [0.0f32, 45., 90., 135., 140., 180., 270., 315., 350.] {
            for span in [0.5f32, 1., 7., 89., 90., 91., 179., 181., 270., 359.] {
                let r = annular_sector_bounds(cx, cy, inner, outer, start, start + span)
                    .inflate(1., 1.);
                // Sample the sector densely in both angle and radius.
                for i in 0..=400 {
                    let a = (start + span * (i as f32 / 400.)).to_radians();
                    for rad in [inner, (inner + outer) / 2., outer] {
                        let x = cx + rad * a.cos();
                        let y = cy + rad * a.sin();
                        assert!(
                            x >= r.origin.x - 0.01
                                && x <= r.origin.x + r.size.width + 0.01
                                && y >= r.origin.y - 0.01
                                && y <= r.origin.y + r.size.height + 0.01,
                            "start={start} span={span}: point ({x:.1},{y:.1}) outside {r:?}"
                        );
                    }
                }
            }
        }
    }

    /// The renderer does not draw the arc the bounds describe: building the ArcCommand
    /// truncates the centre and both radii into i16, so the figure it paints is displaced and
    /// undersized relative to the exact geometry, and anti-aliasing then reaches half a pixel
    /// past every span end. This models that quantisation and asserts the band still contains
    /// it. The geometry here is deliberately non-integral, taken from the utilization gauge on
    /// a 466px panel - `bounds_cover_the_swept_difference` above uses an integer centre and
    /// integer radii, which makes every truncation error identically zero and is why a 1px
    /// slack survived five test suites while hardware kept leaving bands of arc behind.
    #[test]
    fn bounds_cover_what_the_renderer_actually_paints() {
        // The utilization gauge on a 466px panel, the RSSI arc, and centres whose fractional
        // part is nearly a whole pixel - truncation is worst there, so a bound that holds for
        // 0.999 holds for anything.
        for (cx, cy, radius, stroke) in [
            (201.93333f32, 201.93333f32, 194.16667f32, 15.533334f32),
            (233.0f32, 233.0f32, 227.0f32, 12.0f32),
            (240.999f32, 240.999f32, 228.915f32, 15.83f32),
            (232.999f32, 240.001f32, 220.5f32, 12.75f32),
        ] {
        let half_stroke = stroke / 2.;
        let (inner, outer) = (radius - half_stroke, radius + half_stroke);

        // What the ArcCommand ends up holding: i16 truncation toward zero throughout.
        let (cxq, cyq) = (cx.trunc(), cy.trunc());
        let (innerq, outerq) = ((radius - half_stroke).max(0.).trunc(), outer.trunc());
        let capq = half_stroke.trunc();
        // Horizontal anti-aliasing writes one column past each span end.
        const AA: f32 = 0.5;

        for start in [0.0f32, 45., 90., 133.7, 180., 224.3, 270., 315., 350.] {
            for span in [0.5f32, 1., 7., 89., 90., 91., 179., 181., 270., 359.] {
                let band = annular_sector_bounds(cx, cy, inner, outer, start, start + span)
                    .inflate(ARC_BAND_SLACK, ARC_BAND_SLACK);
                let contains = |x: f32, y: f32, what: &str| {
                    assert!(
                        x >= band.origin.x - 0.01
                            && x <= band.origin.x + band.size.width + 0.01
                            && y >= band.origin.y - 0.01
                            && y <= band.origin.y + band.size.height + 0.01,
                        "start={start} span={span}: {what} ({x:.3},{y:.3}) outside {band:?}"
                    );
                };

                for i in 0..=400 {
                    let a = (start + span * (i as f32 / 400.)).to_radians();
                    // The ring as drawn, about the truncated centre and radii.
                    for rad in [innerq, (innerq + outerq) / 2., outerq] {
                        contains(cxq + rad * a.cos() - AA, cyq + rad * a.sin(), "ring");
                        contains(cxq + rad * a.cos() + AA, cyq + rad * a.sin(), "ring");
                    }
                }

                // Round caps: discs on the truncated stroke centre line, truncated radius.
                for a in [start.to_radians(), (start + span).to_radians()] {
                    let (ccx, ccy) =
                        ((cx + radius * a.cos()).trunc(), (cy + radius * a.sin()).trunc());
                    for k in 0..=64 {
                        let t = (k as f32 / 64.) * core::f32::consts::TAU;
                        contains(
                            ccx + (capq + AA) * t.cos(),
                            ccy + (capq + AA) * t.sin(),
                            "cap",
                        );
                    }
                }
            }
        }
        }
    }

    /// The region is scaled and rounded before the renderer sees it, so containment has to
    /// survive that. Rounding the exact extremes drops the outermost row or column, and
    /// where the arc runs parallel to that edge - by the horizontal axes - one lost column
    /// removes a wide band of it. This is what the float-tolerance check above missed.
    #[test]
    fn bounds_survive_rounding_to_the_pixel_grid() {
        // A dial the size the firmware draws.
        let (cx, cy, inner, outer) = (233.0f32, 233.0f32, 221.0f32, 233.0f32);
        for start in [0.0f32, 90., 170., 176., 180., 184., 266., 350., 356., 4.] {
            for span in [1.0f32, 2., 5., 30., 90., 179.] {
                let r = annular_sector_bounds(cx, cy, inner, outer, start, start + span);
                // As the renderer does: round the edges to whole pixels.
                let (x0, y0) = (r.origin.x.round(), r.origin.y.round());
                let (x1, y1) =
                    ((r.origin.x + r.size.width).round(), (r.origin.y + r.size.height).round());
                for i in 0..=400 {
                    let a = (start + span * (i as f32 / 400.)).to_radians();
                    for rad in [inner, (inner + outer) / 2., outer] {
                        let (x, y) = (cx + rad * a.cos(), cy + rad * a.sin());
                        assert!(
                            x >= x0 && x <= x1 && y >= y0 && y <= y1,
                            "start={start} span={span}: ({x:.2},{y:.2}) outside rounded \
                             [{x0}..{x1}, {y0}..{y1}]"
                        );
                    }
                }
            }
        }
    }

    /// The band a small movement sweeps must stay a small fraction of the element, which is
    /// the whole reason for narrowing at all. The grid this replaced could not manage it: one
    /// cell of a 3x3 grid is 11% of the element before a band is even considered.
    #[test]
    fn a_small_change_sweeps_a_small_band() {
        let size = LogicalSize::new(466., 466.);
        let full = size.width * size.height;
        for start in [0.0f32, 45., 140., 200., 300., 359.] {
            let band = annular_sector_bounds(233., 233., 221., 233., start, start + 1.)
                .inflate(ARC_BAND_SLACK, ARC_BAND_SLACK);
            let frac = (band.size.width * band.size.height) / full;
            // Expressed relative to the slack so the bound stays meaningful if it changes:
            // a 1 degree sweep is a few pixels of arc, so the band is essentially the slack
            // squared plus that, against the whole element.
            let limit = ((4. * ARC_BAND_SLACK + 24.) * (4. * ARC_BAND_SLACK + 24.)) / full;
            assert!(
                frac <= limit,
                "start={start}: band is {:.2}% of the element, limit {:.2}%",
                frac * 100.,
                limit * 100.
            );
        }
    }

    /// The point of the exercise: a small sweep change must produce a rect far smaller
    /// than the element, or nothing has been gained over invalidating the whole thing.
    #[test]
    fn small_delta_is_a_small_rect() {
        let full = 466.0f32 * 466.0;
        for start in [0.0f32, 140., 300.] {
            let r = annular_sector_bounds(233., 233., 215., 233., start, start + 1.);
            let area = r.size.width * r.size.height;
            assert!(area < full * 0.05, "start={start}: {area} px is not a small delta");
        }
    }
}

impl ItemConsts for Path {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<Path, CachedRenderingData> =
        Path::FIELD_OFFSETS.cached_rendering_data().as_unpinned_projection();
}

struct FittedPathInner(RefCell<Option<FittedPath>>);

/// Opaque box holding the FittedPath, allocated lazily on first use
#[repr(C)]
pub struct FittedPathBox(core::cell::Cell<*mut FittedPathInner>);

impl Default for FittedPathBox {
    fn default() -> Self {
        FittedPathBox(core::cell::Cell::new(core::ptr::null_mut()))
    }
}
impl FittedPathBox {
    fn get_or_init(&self) -> &FittedPathInner {
        if self.0.get().is_null() {
            self.0.set(Box::leak(Box::new(FittedPathInner(Default::default()))));
        }
        // Safety: the pointer is guaranteed non-null above, and was created from a Box::leak
        unsafe { &*self.0.get() }
    }
}
impl Drop for FittedPathBox {
    fn drop(&mut self) {
        let ptr = self.0.get();
        if !ptr.is_null() {
            // Safety: ptr was constructed from a Box::leak in get_or_init
            drop(unsafe { Box::from_raw(ptr) });
        }
    }
}
impl core::ops::Deref for FittedPathBox {
    type Target = RefCell<Option<FittedPath>>;
    fn deref(&self) -> &Self::Target {
        &self.get_or_init().0
    }
}

/// # Safety
/// This must be called using a non-null pointer pointing to a chunk of memory big enough to
/// hold a FittedPathBox
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slint_path_fitted_cache_init(cache: *mut FittedPathBox) {
    unsafe { core::ptr::write(cache, FittedPathBox::default()) };
}

/// # Safety
/// This must be called using a non-null pointer pointing to an initialized FittedPathBox
#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slint_path_fitted_cache_free(cache: *mut FittedPathBox) {
    unsafe {
        core::ptr::drop_in_place(cache);
    }
}

#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slint_path_point_at(
    self_component: &vtable::VRc<crate::item_tree::ItemTreeVTable>,
    self_index: u32,
    t: f32,
) -> crate::lengths::LogicalPoint {
    let self_rc = ItemRc::new(self_component.clone(), self_index);
    self_rc.downcast::<Path>().unwrap().as_pin_ref().point_at(&self_rc, t)
}

#[cfg(feature = "ffi")]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn slint_path_angle_at(
    self_component: &vtable::VRc<crate::item_tree::ItemTreeVTable>,
    self_index: u32,
    t: f32,
) -> f32 {
    let self_rc = ItemRc::new(self_component.clone(), self_index);
    self_rc.downcast::<Path>().unwrap().as_pin_ref().angle_at(&self_rc, t)
}

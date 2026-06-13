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
use crate::graphics::{Brush, PathData, PathDataIterator};
use crate::input::{
    FocusEvent, FocusEventResult, InputEventFilterResult, InputEventResult, InternalKeyEvent,
    KeyEventResult, MouseEvent,
};
use crate::item_rendering::{CachedRenderingData, RenderArc};

use crate::items::ImageFit;
use crate::layout::{LayoutInfo, Orientation};
use crate::lengths::{
    LogicalBorderRadius, LogicalLength, LogicalRect, LogicalSize, LogicalVector, RectLengths,
};
#[cfg(feature = "rtti")]
use crate::rtti::*;
use crate::window::WindowAdapter;
use crate::{Coord, Property};
use alloc::rc::Rc;
use const_field_offset::FieldOffsets;
use core::pin::Pin;
use euclid::num::Zero;
use i_slint_core_macros::*;

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
    pub viewbox_x: Property<f32>,
    pub viewbox_y: Property<f32>,
    pub viewbox_width: Property<f32>,
    pub viewbox_height: Property<f32>,
    pub fit: Property<ImageFit>,
    pub clip: Property<bool>,
    pub anti_alias: Property<bool>,
    pub cached_rendering_data: CachedRenderingData,
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
        _: &mut super::MouseCursor,
    ) -> InputEventFilterResult {
        InputEventFilterResult::ForwardAndIgnore
    }

    fn input_event(
        self: Pin<&Self>,
        _: &MouseEvent,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
        _: &mut super::MouseCursor,
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
            (*backend).combine_clip(
                size.into(),
                LogicalBorderRadius::zero(),
                LogicalLength::zero(),
            );
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
}

impl ItemConsts for Path {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<Path, CachedRenderingData> =
        Path::FIELD_OFFSETS.cached_rendering_data().as_unpinned_projection();
}

/// The implementation of the `ArcSegment` element
#[repr(C)]
#[derive(FieldOffsets, Default, SlintElement)]
#[pin]
pub struct ArcSegment {
    pub stroke: Property<Brush>,
    pub stroke_width: Property<LogicalLength>,
    pub start_angle: Property<f32>, // angles are represented as f32 in degrees
    pub end_angle: Property<f32>,
    pub stroke_line_cap: Property<LineCap>,
    pub cached_rendering_data: CachedRenderingData,
}

impl Item for ArcSegment {
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
        _: &mut super::MouseCursor,
    ) -> InputEventFilterResult {
        InputEventFilterResult::ForwardAndIgnore
    }

    fn input_event(
        self: Pin<&Self>,
        _: &MouseEvent,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
        _: &mut super::MouseCursor,
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
        (*backend).draw_arc(self, self_rc, size);
        RenderingResult::ContinueRenderingChildren
    }

    fn bounding_rect(
        self: core::pin::Pin<&Self>,
        _window_adapter: &Rc<dyn WindowAdapter>,
        _self_rc: &ItemRc,
        geometry: LogicalRect,
    ) -> LogicalRect {
        let start_deg = self.start_angle();
        let end_deg = self.end_angle();
        let min_a = num_traits::Float::min(start_deg, end_deg);
        let max_a = num_traits::Float::max(start_deg, end_deg);
        arc_bounding_rect_for_angles(geometry, self.stroke_width().get() / 2.0, min_a, max_a)
    }

    fn clips_children(self: core::pin::Pin<&Self>) -> bool {
        false
    }
}

/// Compute the tight bounding rect for an arbitrary angle range within `geometry`.
///
/// This is the building block for [`ArcSegment::bounding_rect`] and for the partial
/// renderer's delta-dirty optimisation: instead of dirtying the full arc bbox on every
/// angle update, only the wedge swept between the old and new endpoint is dirtied.
///
/// `stroke_half_width` – half the stroke width (used for the pixel-level expansion).
/// `a_min_deg` / `a_max_deg` – inclusive angular range in degrees (0° = 3 o'clock, CW).
pub fn arc_bounding_rect_for_angles(
    geometry: LogicalRect,
    stroke_half_width: f32,
    a_min_deg: f32,
    a_max_deg: f32,
) -> LogicalRect {
    use num_traits::Float;
    if a_max_deg <= a_min_deg {
        return LogicalRect::default();
    }
    if a_max_deg - a_min_deg >= 360.0 {
        return geometry;
    }

    let cx = geometry.origin.x + geometry.size.width / 2.0;
    let cy = geometry.origin.y + geometry.size.height / 2.0;
    let rx = Float::max(geometry.size.width - stroke_half_width * 2.0, 0.0) / 2.0;
    let ry = Float::max(geometry.size.height - stroke_half_width * 2.0, 0.0) / 2.0;

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    let mut add_point = |angle_deg: f32| {
        let rad = angle_deg * 0.017_453_292_5;
        let px = cx + rx * Float::cos(rad);
        let py = cy + ry * Float::sin(rad);
        if px < min_x { min_x = px; }
        if px > max_x { max_x = px; }
        if py < min_y { min_y = py; }
        if py > max_y { max_y = py; }
    };

    add_point(a_min_deg);
    add_point(a_max_deg);

    let first_q = Float::floor(a_min_deg / 90.0) as i32;
    let last_q  = Float::ceil(a_max_deg / 90.0) as i32;
    for q in first_q..=last_q {
        let q_angle = (q as f32) * 90.0;
        if q_angle >= a_min_deg && q_angle <= a_max_deg {
            add_point(q_angle);
        }
    }

    let expansion = stroke_half_width + 1.0;
    min_x -= expansion;
    max_x += expansion;
    min_y -= expansion;
    max_y += expansion;

    let orig_min_x = geometry.origin.x - stroke_half_width;
    let orig_max_x = geometry.origin.x + geometry.size.width + stroke_half_width;
    let orig_min_y = geometry.origin.y - stroke_half_width;
    let orig_max_y = geometry.origin.y + geometry.size.height + stroke_half_width;

    min_x = Float::min(Float::max(min_x, orig_min_x), orig_max_x);
    max_x = Float::min(Float::max(max_x, orig_min_x), orig_max_x);
    min_y = Float::min(Float::max(min_y, orig_min_y), orig_max_y);
    max_y = Float::min(Float::max(max_y, orig_min_y), orig_max_y);

    crate::lengths::LogicalRect::new(
        crate::lengths::LogicalPoint::new(min_x, min_y),
        crate::lengths::LogicalSize::new(max_x - min_x, max_y - min_y),
    )
}

impl RenderArc for ArcSegment {
    fn stroke(self: Pin<&Self>) -> Brush {
        self.stroke()
    }

    fn stroke_width(self: Pin<&Self>) -> LogicalLength {
        self.stroke_width()
    }

    fn start_angle(self: Pin<&Self>) -> f32 {
        self.start_angle()
    }

    fn end_angle(self: Pin<&Self>) -> f32 {
        self.end_angle()
    }

    fn stroke_line_cap(self: Pin<&Self>) -> LineCap {
        self.stroke_line_cap()
    }
}

impl ItemConsts for ArcSegment {
    const cached_rendering_data_offset: const_field_offset::FieldOffset<ArcSegment, CachedRenderingData> =
        ArcSegment::FIELD_OFFSETS.cached_rendering_data().as_unpinned_projection();
}


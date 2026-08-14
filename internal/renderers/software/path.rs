// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

//! Path rendering support for the software renderer using zeno

use super::PhysicalRect;
use super::draw_functions::{PremultipliedRgbaColor, TargetPixel};
use alloc::vec;
use alloc::vec::Vec;
use zeno::{Cap, Fill, Join, Mask, Stroke, Style};

pub use zeno::Command;

/// Convert Slint's PathDataIterator to zeno's Command format
pub fn convert_path_data_to_zeno(
    path_data: i_slint_core::graphics::PathDataIterator,
    rotation: crate::RotationInfo,
    scale_factor: i_slint_core::lengths::ScaleFactor,
    offset: euclid::Vector2D<f32, i_slint_core::lengths::PhysicalPx>,
) -> Vec<Command> {
    use crate::Transform as _;
    use i_slint_core::lengths::LogicalPoint;
    use lyon_path::Event;
    let mut commands = Vec::new();

    let convert_point = |p| {
        let p = (LogicalPoint::from_untyped(p) * scale_factor + offset).transformed(rotation);
        zeno::Point::new(p.x, p.y)
    };

    for event in path_data.iter() {
        match event {
            Event::Begin { at } => {
                commands.push(Command::MoveTo(convert_point(at)));
            }
            Event::Line { to, .. } => {
                commands.push(Command::LineTo(convert_point(to)));
            }
            Event::Quadratic { ctrl, to, .. } => {
                commands.push(Command::QuadTo(convert_point(ctrl), convert_point(to)));
            }
            Event::Cubic { ctrl1, ctrl2, to, .. } => {
                commands.push(Command::CurveTo(
                    convert_point(ctrl1),
                    convert_point(ctrl2),
                    convert_point(to),
                ));
            }
            Event::End { close, .. } => {
                if close {
                    commands.push(Command::Close);
                }
            }
        }
    }

    commands
}

/// Build the zeno stroke style for the given Slint stroke properties.
pub fn stroke_style(
    stroke_width: f32,
    stroke_line_cap: i_slint_core::items::LineCap,
    stroke_line_join: i_slint_core::items::LineJoin,
    stroke_miter_limit: f32,
) -> Style<'static> {
    let mut stroke = Stroke::new(stroke_width);
    stroke
        .cap(match stroke_line_cap {
            i_slint_core::items::LineCap::Round => Cap::Round,
            i_slint_core::items::LineCap::Square => Cap::Square,
            i_slint_core::items::LineCap::Butt | _ => Cap::Butt,
        })
        .join(match stroke_line_join {
            i_slint_core::items::LineJoin::Round => Join::Round,
            i_slint_core::items::LineJoin::Bevel => Join::Bevel,
            i_slint_core::items::LineJoin::Miter | _ => Join::Miter,
        })
        .miter_limit(stroke_miter_limit);
    Style::Stroke(stroke)
}

/// Rasterize a path into an 8-bit coverage mask covering `path_geometry`.
///
/// Returns the mask and its dimensions, or None if the geometry is empty. The line-by-line
/// renderer needs the coverage separately from the blending, because it composites one
/// scanline at a time and cannot call into a full-frame buffer.
pub fn rasterize_mask(
    commands: &[Command],
    path_geometry: &PhysicalRect,
    style: Style,
) -> Option<(Vec<u8>, usize, usize)> {
    let path_width = path_geometry.size.width as usize;
    let path_height = path_geometry.size.height as usize;

    if path_width == 0 || path_height == 0 {
        return None;
    }

    let mut mask_buffer = vec![0u8; path_width * path_height];

    // Rasterize through a Scratch rather than Mask::new. Without one, zeno's rasterizer
    // builds an AdaptiveStorage local, whose inline [Cell; 1024] + [i32; 512] is ~18KB of
    // stack - more than an MCU render task typically has in total. Scratch redirects that
    // storage to the heap.
    let mut scratch = zeno::Scratch::new();
    Mask::with_scratch(commands, &mut scratch)
        .size(path_width as u32, path_height as u32)
        .style(style)
        .render_into(&mut mask_buffer, None);

    Some((mask_buffer, path_width, path_height))
}

/// Common rendering logic for both filled and stroked paths
fn render_path_with_style<T: TargetPixel>(
    commands: &[Command],
    path_geometry: &PhysicalRect,
    clip_geometry: &PhysicalRect,
    color: PremultipliedRgbaColor,
    style: zeno::Style,
    buffer: &mut impl crate::target_pixel_buffer::TargetPixelBuffer<TargetPixel = T>,
) {
    let Some((mask_buffer, path_width, path_height)) =
        rasterize_mask(commands, path_geometry, style)
    else {
        return;
    };

    // Calculate the intersection region - only apply within clipped area
    // clip_geometry is relative to screen, path_geometry is also relative to screen
    let clip_x_start = clip_geometry.origin.x.max(0) as usize;
    let clip_y_start = clip_geometry.origin.y.max(0) as usize;
    let clip_x_end = (clip_geometry.max_x().max(0) as usize).min(buffer.line_slice(0).len());
    let clip_y_end = (clip_geometry.max_y().max(0) as usize).min(buffer.num_lines());

    let path_x_start = path_geometry.origin.x as isize;
    let path_y_start = path_geometry.origin.y as isize;

    // Apply the mask only within the clipped region
    for screen_y in clip_y_start..clip_y_end {
        let line = buffer.line_slice(screen_y);

        // Calculate the y coordinate in the mask buffer
        let mask_y = screen_y as isize - path_y_start;
        if mask_y < 0 || mask_y >= path_height as isize {
            continue;
        }

        // Iterate the writable portion of the line directly to avoid indexing by loop variable
        let line_slice = &mut line[clip_x_start..clip_x_end];
        for (i, pixel) in line_slice.iter_mut().enumerate() {
            let screen_x = clip_x_start + i;

            // Calculate the x coordinate in the mask buffer
            let mask_x = screen_x as isize - path_x_start;
            if mask_x < 0 || mask_x >= path_width as isize {
                continue;
            }

            let mask_idx = (mask_y as usize) * path_width + (mask_x as usize);
            let coverage = mask_buffer[mask_idx];

            if coverage > 0 {
                // Scale all color components by coverage to maintain premultiplication
                let coverage_factor = coverage as u16;
                let alpha_color = PremultipliedRgbaColor {
                    red: ((color.red as u16 * coverage_factor) / 255) as u8,
                    green: ((color.green as u16 * coverage_factor) / 255) as u8,
                    blue: ((color.blue as u16 * coverage_factor) / 255) as u8,
                    alpha: ((color.alpha as u16 * coverage_factor) / 255) as u8,
                };
                T::blend(pixel, alpha_color);
            }
        }
    }
}

/// Render a filled path
///
/// * `commands` - The path commands to render
/// * `path_geometry` - The full bounding box of the path in screen coordinates
/// * `clip_geometry` - The clipped region where the path should be rendered (intersection of path and clip)
/// * `color` - The color to render the path
/// * `buffer` - The target pixel buffer
pub fn render_filled_path<T: TargetPixel>(
    commands: &[Command],
    path_geometry: &PhysicalRect,
    clip_geometry: &PhysicalRect,
    color: PremultipliedRgbaColor,
    buffer: &mut impl crate::target_pixel_buffer::TargetPixelBuffer<TargetPixel = T>,
) {
    render_path_with_style(
        commands,
        path_geometry,
        clip_geometry,
        color,
        zeno::Style::Fill(Fill::NonZero),
        buffer,
    );
}

/// Render a stroked path
///
/// * `commands` - The path commands to render
/// * `path_geometry` - The full bounding box of the path in screen coordinates
/// * `clip_geometry` - The clipped region where the path should be rendered (intersection of path and clip)
/// * `color` - The color to render the path
/// * `stroke_width` - The width of the stroke
/// * `buffer` - The target pixel buffer
pub fn render_stroked_path<T: TargetPixel>(
    commands: &[Command],
    path_geometry: &PhysicalRect,
    clip_geometry: &PhysicalRect,
    color: PremultipliedRgbaColor,
    stroke_width: f32,
    stroke_line_cap: i_slint_core::items::LineCap,
    stroke_line_join: i_slint_core::items::LineJoin,
    stroke_miter_limit: f32,
    buffer: &mut impl crate::target_pixel_buffer::TargetPixelBuffer<TargetPixel = T>,
) {
    let style = stroke_style(stroke_width, stroke_line_cap, stroke_line_join, stroke_miter_limit);
    render_path_with_style(commands, path_geometry, clip_geometry, color, style, buffer);
}

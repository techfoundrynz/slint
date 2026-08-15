// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

// cSpell: ignore premultiply
#![allow(clippy::identity_op)] // We use x + 0 a lot here for symmetry

//! This is the module for the functions that are drawing the pixels
//! on the line buffer

use super::{Fixed, PhysicalLength, PhysicalRect};
use derive_more::{Add, Mul, Sub};
use i_slint_core::Color;
use i_slint_core::graphics::{Rgb8Pixel, TexturePixelFormat};
use i_slint_core::lengths::{PointLengths, SizeLengths};
use integer_sqrt::IntegerSquareRoot;
#[allow(unused_imports)]
use num_traits::Float;

/// Draw one line of the texture in the line buffer
///
pub(super) fn draw_texture_line(
    span: &PhysicalRect,
    line: PhysicalLength,
    texture: &super::SceneTexture,
    line_buffer: &mut [impl TargetPixel],
    extra_clip_begin: i16,
    extra_clip_end: i16,
) {
    let super::SceneTexture {
        data,
        format,
        pixel_stride,
        extra: super::SceneTextureExtra { colorize, alpha, rotation, dx, dy, off_x, off_y },
    } = *texture;

    let source_size = texture.source_size().cast::<i32>();
    let len = line_buffer.len();
    let y = line - span.origin.y_length();
    let y = if rotation.mirror_width() { span.size.height - y.get() - 1 } else { y.get() } as i32;

    let off_y = Fixed::<i32, 8>::from_fixed(off_y);
    let dx = Fixed::<i32, 8>::from_fixed(dx);
    let dy = Fixed::<i32, 8>::from_fixed(dy);
    let off_x = Fixed::<i32, 8>::from_fixed(off_x);

    if !rotation.is_transpose() {
        let mut delta = dx;
        let row = off_y + dy * y;
        // The position where to start in the image array for a this row
        let row_offset = (row.truncate() % source_size.height) as usize * pixel_stride as usize;
        let mut tile_start = 0;

        // the size of the tile in physical pixels in the target
        let tile_len = (Fixed::from_integer(source_size.width) / delta) as usize;
        // the amount of missing image pixel on one tile
        let mut remainder = Fixed::from_integer(source_size.width) % delta;
        // The position in image pixel where to get the image
        let mut pos;
        // the end index in the target buffer
        let mut end;
        // the accumulated error in image pixels
        let mut acc_err;
        if rotation.mirror_height() {
            let o = (off_x + (delta * (extra_clip_end as i32 + len as i32 - 1)))
                % Fixed::from_integer(source_size.width);
            pos = o;
            tile_start = source_size.width;
            end = (o / delta) as usize + 1;
            acc_err = -delta + o % delta;
            delta = -delta;
            remainder = -remainder;
        } else {
            let o =
                (off_x + delta * extra_clip_begin as i32) % Fixed::from_integer(source_size.width);
            pos = o;
            end = ((Fixed::from_integer(source_size.width) - o) / delta) as usize;
            acc_err = (Fixed::from_integer(source_size.width) - o) % delta;
            if acc_err != Fixed::default() {
                acc_err = delta - acc_err;
                end += 1;
            }
        }
        end = end.min(len);
        let mut begin = 0;
        let row_fract = row.fract();
        while begin < len {
            fetch_blend_pixel(
                &mut line_buffer[begin..end],
                format,
                data,
                alpha,
                colorize,
                (pixel_stride as usize, dy),
                #[inline(always)]
                |bpp| {
                    let p = ((row_offset + pos.truncate() as usize) * bpp, pos.fract(), row_fract);
                    pos += delta;
                    p
                },
            );
            begin = end;
            end += tile_len;
            pos = acc_err + Fixed::from_integer(tile_start);
            if remainder != Fixed::from_integer(0) {
                acc_err -= remainder;
                let wrap = if rotation.mirror_height() {
                    acc_err >= Fixed::from_integer(0)
                } else {
                    acc_err < Fixed::from_integer(0)
                };
                if wrap {
                    acc_err += delta;
                    end += 1;
                }
            };
            end = end.min(len);
        }
    } else {
        let bpp = format.bpp();
        let col = off_x + dx * y;
        let col_fract = col.fract();
        let col = (col.truncate() % source_size.width) as usize * bpp;
        let stride = pixel_stride as usize * bpp;
        let mut row_delta = dy;
        let tile_len = (Fixed::from_integer(source_size.height) / row_delta) as usize;
        let mut remainder = Fixed::from_integer(source_size.height) % row_delta;
        let mut end;
        let mut row_init = Fixed::default();
        let mut row;
        let mut acc_err;
        if rotation.mirror_height() {
            row_init = Fixed::from_integer(source_size.height);
            row = (off_y + (row_delta * (extra_clip_end as i32 + len as i32 - 1)))
                % Fixed::from_integer(source_size.height);
            end = (row / row_delta) as usize + 1;
            acc_err = -row_delta + row % row_delta;
            row_delta = -row_delta;
            remainder = -remainder;
        } else {
            row = (off_y + row_delta * extra_clip_begin as i32)
                % Fixed::from_integer(source_size.height);
            end = ((Fixed::from_integer(source_size.height) - row) / row_delta) as usize;
            acc_err = (Fixed::from_integer(source_size.height) - row) % row_delta;
            if acc_err != Fixed::default() {
                acc_err = row_delta - acc_err;
                end += 1;
            }
        };
        end = end.min(len);
        let mut begin = 0;
        while begin < len {
            fetch_blend_pixel(
                &mut line_buffer[begin..end],
                format,
                data,
                alpha,
                colorize,
                (stride, dy),
                #[inline(always)]
                |_| {
                    let pos = (row.truncate() as usize * stride + col, col_fract, row.fract());
                    row += row_delta;
                    pos
                },
            );
            begin = end;
            end += tile_len;
            row = row_init;
            row += acc_err;
            if remainder != Fixed::from_integer(0) {
                acc_err -= remainder;
                let wrap = if rotation.mirror_height() {
                    acc_err >= Fixed::from_integer(0)
                } else {
                    acc_err < Fixed::from_integer(0)
                };
                if wrap {
                    acc_err += row_delta;
                    end += 1;
                }
            };
            end = end.min(len);
        }
    };

    fn fetch_blend_pixel(
        line_buffer: &mut [impl TargetPixel],
        format: TexturePixelFormat,
        data: &[u8],
        alpha: u8,
        color: Color,
        (stride, delta): (usize, Fixed<i32, 8>),
        mut pos: impl FnMut(usize) -> (usize, u8, u8),
    ) {
        match format {
            TexturePixelFormat::Rgb => {
                for pix in line_buffer {
                    let pos = pos(3).0;
                    let p: &[u8] = &data[pos..pos + 3];
                    if alpha == 0xff {
                        *pix = TargetPixel::from_rgb(p[0], p[1], p[2]);
                    } else {
                        pix.blend(PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                            alpha, p[0], p[1], p[2],
                        )))
                    }
                }
            }
            TexturePixelFormat::Rgba => {
                if color.alpha() == 0 {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let alpha = ((data[pos + 3] as u16 * alpha as u16) / 255) as u8;
                        let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                            alpha,
                            data[pos + 0],
                            data[pos + 1],
                            data[pos + 2],
                        ));
                        pix.blend(c);
                    }
                } else {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let alpha = ((data[pos + 3] as u16 * alpha as u16) / 255) as u8;
                        let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                            alpha,
                            color.red(),
                            color.green(),
                            color.blue(),
                        ));
                        pix.blend(c);
                    }
                }
            }
            TexturePixelFormat::RgbaPremultiplied => {
                if color.alpha() > 0 {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                            ((data[pos + 3] as u16 * alpha as u16) / 255) as u8,
                            color.red(),
                            color.green(),
                            color.blue(),
                        ));
                        pix.blend(c);
                    }
                } else if alpha == 0xff {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let c = PremultipliedRgbaColor {
                            alpha: data[pos + 3],
                            red: data[pos + 0],
                            green: data[pos + 1],
                            blue: data[pos + 2],
                        };
                        pix.blend(c);
                    }
                } else {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let c = PremultipliedRgbaColor {
                            alpha: (data[pos + 3] as u16 * alpha as u16 / 255) as u8,
                            red: (data[pos + 0] as u16 * alpha as u16 / 255) as u8,
                            green: (data[pos + 1] as u16 * alpha as u16 / 255) as u8,
                            blue: (data[pos + 2] as u16 * alpha as u16 / 255) as u8,
                        };
                        pix.blend(c);
                    }
                }
            }
            TexturePixelFormat::AlphaMap => {
                // Hottest loop on any screen with text: the colour channels are
                // loop-invariant, and at full alpha the coverage scaling collapses to a read.
                let (cr, cg, cb) = (color.red(), color.green(), color.blue());
                if alpha == 0xff {
                    for pix in line_buffer {
                        let pos = pos(1).0;
                        let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                            data[pos],
                            cr,
                            cg,
                            cb,
                        ));
                        pix.blend(c);
                    }
                } else {
                    let a = alpha as u16;
                    for pix in line_buffer {
                        let pos = pos(1).0;
                        let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                            ((data[pos] as u16 * a) / 255) as u8,
                            cr,
                            cg,
                            cb,
                        ));
                        pix.blend(c);
                    }
                }
            }
            TexturePixelFormat::SignedDistanceField => {
                const RANGE: i32 = 6;
                let factor = (362 * 256 / delta.0) * RANGE; // 362 ≃ 255 * sqrt(2)
                for pix in line_buffer {
                    let (pos, col_f, row_f) = pos(1);
                    let (col_f, row_f) = (col_f as i32, row_f as i32);
                    let mut dist = ((data[pos] as i8 as i32) * (256 - col_f)
                        + (data[pos + 1] as i8 as i32) * col_f)
                        * (256 - row_f);
                    if pos + stride + 1 < data.len() {
                        dist += ((data[pos + stride] as i8 as i32) * (256 - col_f)
                            + (data[pos + stride + 1] as i8 as i32) * col_f)
                            * row_f
                    } else {
                        debug_assert_eq!(row_f, 0);
                    }
                    let a = ((((dist >> 8) * factor) >> 16) + 128).clamp(0, 255) * alpha as i32;
                    let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                        (a / 255) as u8,
                        color.red(),
                        color.green(),
                        color.blue(),
                    ));
                    pix.blend(c);
                }
            }
        };
    }
}

/// Draw one line of a stroked circular arc into the line buffer.
///
/// Coverage comes from the circle equation rather than from a rasterized mask, so the only
/// pixels touched are the ones the ring passes through. Per row this is two square roots
/// for the radial edges plus O(1) work for the angular ends; the interior of each span is
/// a flat `blend_slice`.
///
/// The row is built in three stages:
///   1. radial - `x = sqrt(r^2 - y^2)` for the outer and inner edge gives the row's one or
///      two runs (two when the row passes through the hole)
///   2. angular - each boundary ray is a half plane whose edge crosses this row at a single
///      x, so the wedge reduces to an interval; a reflex sweep is the union of two
///   3. the intersection of the two, emitted as solid runs with partial pixels at the ends
pub(super) fn draw_arc_line(
    span: &PhysicalRect,
    line: PhysicalLength,
    arc: &super::ArcCommand,
    line_buffer: &mut [impl TargetPixel],
    extra_left_clip: i16,
) {
    let width = line_buffer.len() as i32;
    if width <= 0 || arc.color.alpha == 0 {
        return;
    }

    // Centre of this pixel row relative to the circle centre.
    let dy = (line.get() - span.origin.y_length().get() - arc.center_y.get()) as f32 + 0.5;
    let outer = arc.outer_radius.get() as f32;
    let inner = arc.inner_radius.get().max(0) as f32;
    let dy_abs = if dy < 0. { -dy } else { dy };
    if dy_abs >= outer {
        return;
    }

    let sqrt = |v: f32| -> f32 {
        if v <= 0. { 0. } else { Float::sqrt(v) }
    };

    // Half width of the ring at this row. The outer edge always exists here; the inner
    // edge only when the row passes through the hole.
    let half_outer = sqrt(outer * outer - dy * dy);
    let has_hole = dy_abs < inner;
    let half_inner = if has_hole { sqrt(inner * inner - dy * dy) } else { 0. };

    // Circle centre in line-buffer coordinates. center_x is relative to the span origin,
    // and the buffer starts extra_left_clip pixels into the span.
    let cx = (arc.center_x.get() - extra_left_clip) as f32;

    // The row's runs before angular clipping. Without a hole the ring is one run.
    let mut runs: [(f32, f32); 2] = [(0., 0.); 2];
    let run_count = if has_hole {
        runs[0] = (cx - half_outer, cx - half_inner);
        runs[1] = (cx + half_inner, cx + half_outer);
        2
    } else {
        runs[0] = (cx - half_outer, cx + half_outer);
        1
    };

    // Angular clipping. For a boundary ray with unit direction d the wedge side is
    // cross(d, p) >= 0, which for a fixed row is linear in x and so clips the row to a
    // half line. A reflex sweep is the union of the two half planes, which can leave a
    // gap in the middle of the row.
    let half_plane = |dir: (f32, f32)| -> (f32, f32) {
        let (dx_f, dy_f) = dir;
        // cross(d, p) = d.x * dy - d.y * dx >= 0  =>  dx <= (d.x * dy) / d.y  when d.y > 0
        if dy_f > 0. {
            (f32::NEG_INFINITY, cx + (dx_f * dy) / dy_f)
        } else if dy_f < 0. {
            (cx + (dx_f * dy) / dy_f, f32::INFINITY)
        } else if dx_f * dy >= 0. {
            // A horizontal ray has no x to solve for, so the row is in or out as a whole.
            // Rows within half a pixel of the centre are handled per pixel instead, above.
            (f32::NEG_INFINITY, f32::INFINITY)
        } else {
            (0., 0.)
        }
    };

    // The row a boundary ray passes through, done per pixel.
    //
    // Clipping a row to an x interval per ray cannot describe this row. A ray on the
    // horizontal has no x to solve for - its half plane boundary *is* the row - and a half
    // turn arc has both rays on it at once, where the intersection of two half planes cannot
    // express "both tips". Testing each pixel against the wedge directly has neither problem.
    // It costs one row per ray, so at most two per arc per frame.
    if !arc.full_circle && dy_abs <= 0.5 {
        let inside = |dx: f32| {
            // cross(start, p) >= 0 is at or after the start; cross(p, end) >= 0 is at or
            // before the end.
            let after_start = arc.start_dir.0 * dy - arc.start_dir.1 * dx >= 0.;
            let before_end = dx * arc.end_dir.1 - dy * arc.end_dir.0 >= 0.;
            if arc.reflex { after_start || before_end } else { after_start && before_end }
        };
        let cap_covers = |dx: f32| {
            if !arc.round_caps {
                return false;
            }
            let h = arc.cap_radius.get() as f32;
            [arc.start_cap, arc.end_cap].iter().any(|cap| {
                let cdx = dx - (cap.0.get() - arc.center_x.get()) as f32;
                let cdy = dy - (cap.1.get() - arc.center_y.get()) as f32;
                cdx * cdx + cdy * cdy <= h * h
            })
        };
        // Sweep the ring runs *and* the cap discs: a cap can cover a pixel outside the ring's
        // radial run on this row, so the run clip gates only the ring test.
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for run in runs.iter().take(run_count) {
            lo = lo.min(run.0);
            hi = hi.max(run.1);
        }
        if arc.round_caps {
            let h = arc.cap_radius.get() as f32;
            for cap in [arc.start_cap, arc.end_cap] {
                let ccx = (cap.0.get() - extra_left_clip) as f32;
                let cdy = dy - (cap.1.get() - arc.center_y.get()) as f32;
                let half_chord = h * h - cdy * cdy;
                if half_chord > 0. {
                    let half_chord = Float::sqrt(half_chord);
                    lo = lo.min(ccx - half_chord);
                    hi = hi.max(ccx + half_chord);
                }
            }
        }
        if hi > lo {
            let from = lo.max(0.).floor() as i32;
            let to = (hi.min(width as f32).ceil() as i32).min(width);
            for x in from..to {
                let px = x as f32 + 0.5;
                let dx = px - cx;
                let in_ring = runs
                    .iter()
                    .take(run_count)
                    .any(|run| px >= run.0 && px <= run.1);
                if (in_ring && inside(dx)) || cap_covers(dx) {
                    line_buffer[x as usize].blend(arc.color);
                }
            }
        }
        return;
    }

    // Up to four from the ring (two runs, each possibly split by a reflex wedge) plus one
    // per round cap.
    let mut spans: [(f32, f32); 6] = [(0., 0.); 6];
    let mut span_count = 0usize;
    {
        let mut push = |lo: f32, hi: f32| {
            if hi > lo && span_count < 6 {
                spans[span_count] = (lo, hi);
                span_count += 1;
            }
        };
        let a = if arc.full_circle { (f32::NEG_INFINITY, f32::INFINITY) } else { half_plane(arc.start_dir) };
        let b = if arc.full_circle {
            (f32::NEG_INFINITY, f32::INFINITY)
        } else {
            half_plane((-arc.end_dir.0, -arc.end_dir.1))
        };
        for run in runs.iter().take(run_count) {
            if arc.full_circle || !arc.reflex {
                push(run.0.max(a.0).max(b.0), run.1.min(a.1).min(b.1));
            } else {
                // Union of the two half planes, merged when they overlap so no pixel is
                // blended twice.
                let mut p0 = (run.0.max(a.0), run.1.min(a.1));
                let mut p1 = (run.0.max(b.0), run.1.min(b.1));
                if p0.0 > p1.0 {
                    core::mem::swap(&mut p0, &mut p1);
                }
                if p0.1 > p0.0 && p1.1 > p1.0 && p0.1 >= p1.0 {
                    push(p0.0, if p0.1 > p1.1 { p0.1 } else { p1.1 });
                } else {
                    push(p0.0, p0.1);
                    push(p1.0, p1.1);
                }
            }
        }

        // Round caps: a disc at each end of the sweep, which on this row is just another
        // span from the circle equation. A full circle has no ends.
        if arc.round_caps && !arc.full_circle {
            let h = arc.cap_radius.get() as f32;
            for cap in [arc.start_cap, arc.end_cap] {
                let ccx = (cap.0.get() - extra_left_clip) as f32;
                let dyc = dy - (cap.1.get() - arc.center_y.get()) as f32;
                if dyc > -h && dyc < h {
                    let half = sqrt(h * h - dyc * dyc);
                    push(ccx - half, ccx + half);
                }
            }
        }
    }

    // The spans can now overlap - a cap sits on top of the ring it terminates - so sort
    // and merge them. Blending the same pixel twice would darken the overlap, and with a
    // translucent stroke the seam would be plainly visible.
    for i in 1..span_count {
        let v = spans[i];
        let mut j = i;
        while j > 0 && spans[j - 1].0 > v.0 {
            spans[j] = spans[j - 1];
            j -= 1;
        }
        spans[j] = v;
    }
    let mut merged: [(f32, f32); 6] = [(0., 0.); 6];
    let mut merged_count = 0usize;
    for k in 0..span_count {
        if merged_count > 0 && spans[k].0 <= merged[merged_count - 1].1 {
            if spans[k].1 > merged[merged_count - 1].1 {
                merged[merged_count - 1].1 = spans[k].1;
            }
        } else {
            merged[merged_count] = spans[k];
            merged_count += 1;
        }
    }

    // Emit. Both ends of every span carry fractional coverage, whether that end came from
    // the circle or from a boundary ray; everything between is opaque.
    for &(x0, x1) in merged.iter().take(merged_count) {
        let x0 = x0.max(0.);
        let x1 = x1.min(width as f32);
        if x1 <= x0 {
            continue;
        }
        let first = x0.floor() as i32;
        let last = ((x1.ceil() as i32) - 1).min(width - 1);
        if first > last || first < 0 {
            continue;
        }
        if first == last {
            blend_coverage(&mut line_buffer[first as usize], arc.color, x1 - x0);
            continue;
        }
        blend_coverage(&mut line_buffer[first as usize], arc.color, (first + 1) as f32 - x0);
        let solid_start = (first + 1) as usize;
        let solid_end = last as usize;
        if solid_start < solid_end {
            TargetPixel::blend_slice(&mut line_buffer[solid_start..solid_end], arc.color);
        }
        blend_coverage(&mut line_buffer[last as usize], arc.color, x1 - last as f32);
    }
}

#[inline]
fn blend_coverage(pixel: &mut impl TargetPixel, color: PremultipliedRgbaColor, coverage: f32) {
    let cov = (coverage.clamp(0., 1.) * 255.) as u32;
    if cov == 0 {
        return;
    }
    pixel.blend(PremultipliedRgbaColor {
        alpha: ((color.alpha as u32 * cov) / 255) as u8,
        red: ((color.red as u32 * cov) / 255) as u8,
        green: ((color.green as u32 * cov) / 255) as u8,
        blue: ((color.blue as u32 * cov) / 255) as u8,
    });
}

/// draw one line of the rounded rectangle in the line buffer
#[allow(clippy::unnecessary_cast)] // Coord
pub(super) fn draw_rounded_rectangle_line(
    span: &PhysicalRect,
    line: PhysicalLength,
    rr: &super::RoundedRectangle,
    line_buffer: &mut [impl TargetPixel],
    extra_left_clip: i16,
    extra_right_clip: i16,
) {
    /// This is an integer shifted by 4 bits.
    /// Note: this is not a "fixed point" because multiplication and sqrt operation operate to
    /// the shifted integer
    #[derive(Clone, Copy, PartialEq, Ord, PartialOrd, Eq, Add, Sub, Mul)]
    struct Shifted(u32);
    impl Shifted {
        const ONE: Self = Shifted(1 << 4);
        #[track_caller]
        #[inline]
        pub fn new(value: impl TryInto<u32> + core::fmt::Debug + Copy) -> Self {
            Self(value.try_into().unwrap_or_else(|_| panic!("Overflow {value:?}")) << 4)
        }
        #[inline(always)]
        pub fn floor(self) -> u32 {
            self.0 >> 4
        }
        #[inline(always)]
        pub fn ceil(self) -> u32 {
            (self.0 + Self::ONE.0 - 1) >> 4
        }
        #[inline(always)]
        pub fn saturating_sub(self, other: Self) -> Self {
            Self(self.0.saturating_sub(other.0))
        }
        #[inline(always)]
        pub fn sqrt(self) -> Self {
            Self(self.0.integer_sqrt())
        }
    }
    impl core::ops::Mul for Shifted {
        type Output = Shifted;
        #[inline(always)]
        fn mul(self, rhs: Self) -> Self::Output {
            Self(self.0 * rhs.0)
        }
    }
    let width = line_buffer.len();
    let y1 = (line - span.origin.y_length()) + rr.top_clip;
    let y2 = (span.origin.y_length() + span.size.height_length() - line) + rr.bottom_clip
        - PhysicalLength::new(1);
    let y = y1.min(y2);
    debug_assert!(y.get() >= 0,);
    let border = Shifted::new(rr.width.get());
    const ONE: Shifted = Shifted::ONE;
    const ZERO: Shifted = Shifted(0);
    let anti_alias = |x1: Shifted, x2: Shifted, process_pixel: &mut dyn FnMut(usize, u32)| {
        // x1 and x2 are the coordinate on the top and bottom of the intersection of the pixel
        // line and the curve.
        // `process_pixel` be called for the coordinate in the array and a coverage between 0..255
        // This algorithm just go linearly which is not perfect, but good enough.
        for x in x1.floor()..x2.ceil() {
            // the coverage is basically how much of the pixel should be used
            let cov = ((ONE + Shifted::new(x) - x1).0 << 8) / (ONE + x2 - x1).0;
            process_pixel(x as usize, cov);
        }
    };
    let rev = |x: Shifted| {
        (Shifted::new(width) + Shifted::new(rr.right_clip.get() + extra_right_clip))
            .saturating_sub(x)
    };
    let calculate_xxxx = |r: i16, y: i16| {
        let r = Shifted::new(r);
        // `y` is how far away from the center of the circle the current line is.
        let y = r - Shifted::new(y);
        // Circle equation: x = √(r² - y²)
        // Coordinate from the left edge: x' = r - x
        let x2 = r - (r * r).saturating_sub(y * y).sqrt();
        let x1 = r - (r * r).saturating_sub((y - ONE) * (y - ONE)).sqrt();
        let r2 = r.saturating_sub(border);
        let x4 = r - (r2 * r2).saturating_sub(y * y).sqrt();
        let x3 = r - (r2 * r2).saturating_sub((y - ONE) * (y - ONE)).sqrt();
        (x1, x2, x3, x4)
    };

    let (x1, x2, x3, x4, x5, x6, x7, x8) = if let Some(r) = rr.radius.as_uniform() {
        let (x1, x2, x3, x4) =
            if y.get() < r { calculate_xxxx(r, y.get()) } else { (ZERO, ZERO, border, border) };
        (x1, x2, x3, x4, rev(x4), rev(x3), rev(x2), rev(x1))
    } else {
        let (x1, x2, x3, x4) = if y1 < PhysicalLength::new(rr.radius.top_left) {
            calculate_xxxx(rr.radius.top_left, y.get())
        } else if y2 < PhysicalLength::new(rr.radius.bottom_left) {
            calculate_xxxx(rr.radius.bottom_left, y.get())
        } else {
            (ZERO, ZERO, border, border)
        };
        let (x5, x6, x7, x8) = if y1 < PhysicalLength::new(rr.radius.top_right) {
            let x = calculate_xxxx(rr.radius.top_right, y.get());
            (x.3, x.2, x.1, x.0)
        } else if y2 < PhysicalLength::new(rr.radius.bottom_right) {
            let x = calculate_xxxx(rr.radius.bottom_right, y.get());
            (x.3, x.2, x.1, x.0)
        } else {
            (border, border, ZERO, ZERO)
        };
        (x1, x2, x3, x4, rev(x5), rev(x6), rev(x7), rev(x8))
    };
    anti_alias(
        x1.saturating_sub(Shifted::new(rr.left_clip.get() + extra_left_clip)),
        x2.saturating_sub(Shifted::new(rr.left_clip.get() + extra_left_clip)),
        &mut |x, cov| {
            if x >= width {
                return;
            }
            let c = if border == ZERO { rr.inner_color } else { rr.border_color };
            let col = PremultipliedRgbaColor {
                alpha: (((c.alpha as u32) * cov as u32) / 255) as u8,
                red: (((c.red as u32) * cov as u32) / 255) as u8,
                green: (((c.green as u32) * cov as u32) / 255) as u8,
                blue: (((c.blue as u32) * cov as u32) / 255) as u8,
            };
            line_buffer[x].blend(col);
        },
    );
    if y < rr.width {
        // up or down border (x2 .. x7)
        let l = x2
            .ceil()
            .saturating_sub((rr.left_clip.get() + extra_left_clip) as u32)
            .min(width as u32) as usize;
        let r = x7.floor().min(width as u32) as usize;
        if l < r {
            TargetPixel::blend_slice(&mut line_buffer[l..r], rr.border_color)
        }
    } else {
        if border > ZERO {
            // 3. draw the border (between x2 and x3)
            if ONE + x2 <= x3 {
                TargetPixel::blend_slice(
                    &mut line_buffer[x2
                        .ceil()
                        .saturating_sub((rr.left_clip.get() + extra_left_clip) as u32)
                        .min(width as u32) as usize
                        ..x3.floor()
                            .saturating_sub((rr.left_clip.get() + extra_left_clip) as u32)
                            .min(width as u32) as usize],
                    rr.border_color,
                )
            }
            // 4. anti-aliasing for the contents (x3 .. x4)
            anti_alias(
                x3.saturating_sub(Shifted::new(rr.left_clip.get() + extra_left_clip)),
                x4.saturating_sub(Shifted::new(rr.left_clip.get() + extra_left_clip)),
                &mut |x, cov| {
                    if x >= width {
                        return;
                    }
                    let col = interpolate_color(cov, rr.border_color, rr.inner_color);
                    line_buffer[x].blend(col);
                },
            );
        }
        if rr.inner_color.alpha > 0 {
            // 5. inside (x4 .. x5)
            let begin = x4
                .ceil()
                .saturating_sub((rr.left_clip.get() + extra_left_clip) as u32)
                .min(width as u32);
            let end = x5.floor().min(width as u32);
            if begin < end {
                TargetPixel::blend_slice(
                    &mut line_buffer[begin as usize..end as usize],
                    rr.inner_color,
                )
            }
        }
        if border > ZERO {
            // 6. border anti-aliasing: x5..x6
            anti_alias(x5, x6, &mut |x, cov| {
                if x >= width {
                    return;
                }
                let col = interpolate_color(cov, rr.inner_color, rr.border_color);
                line_buffer[x].blend(col)
            });
            // 7. border x6 .. x7
            if ONE + x6 <= x7 {
                TargetPixel::blend_slice(
                    &mut line_buffer[x6.ceil().min(width as u32) as usize
                        ..x7.floor().min(width as u32) as usize],
                    rr.border_color,
                )
            }
        }
    }
    anti_alias(x7, x8, &mut |x, cov| {
        if x >= width {
            return;
        }
        let c = if border == ZERO { rr.inner_color } else { rr.border_color };
        let col = PremultipliedRgbaColor {
            alpha: (((c.alpha as u32) * (255 - cov) as u32) / 255) as u8,
            red: (((c.red as u32) * (255 - cov) as u32) / 255) as u8,
            green: (((c.green as u32) * (255 - cov) as u32) / 255) as u8,
            blue: (((c.blue as u32) * (255 - cov) as u32) / 255) as u8,
        };
        line_buffer[x].blend(col);
    });
}

// a is between 0 and 255. When 0, we get color1, when 255 we get color2
fn interpolate_color(
    a: u32,
    color1: PremultipliedRgbaColor,
    color2: PremultipliedRgbaColor,
) -> PremultipliedRgbaColor {
    let b = 255 - a;

    let al1 = color1.alpha as u32;
    let al2 = color2.alpha as u32;

    let a_ = a * al2;
    let b_ = b * al1;
    let m = a_ + b_;

    if m == 0 {
        return PremultipliedRgbaColor::default();
    }

    PremultipliedRgbaColor {
        alpha: (m / 255) as u8,
        red: ((b * color1.red as u32 + a * color2.red as u32) / 255) as u8,
        green: ((b * color1.green as u32 + a * color2.green as u32) / 255) as u8,
        blue: ((b * color1.blue as u32 + a * color2.blue as u32) / 255) as u8,
    }
}

pub(super) fn draw_linear_gradient(
    rect: &PhysicalRect,
    line: PhysicalLength,
    g: &super::LinearGradientCommand,
    mut buffer: &mut [impl TargetPixel],
    extra_left_clip: i16,
) {
    let fill_col1 = g.flags & 0b010 != 0;
    let fill_col2 = g.flags & 0b100 != 0;
    let invert_slope = g.flags & 0b1 != 0;

    let y = (line.get() - rect.min_y() + g.top_clip.get()) as i32;
    let size_y = (rect.height() + g.top_clip.get() + g.bottom_clip.get()) as i32;
    let start = g.start as i32;

    let (mut color1, mut color2) = (g.color1, g.color2);

    if g.start == 0 {
        let p = if invert_slope {
            (255 - start) * y / size_y
        } else {
            start + (255 - start) * y / size_y
        };
        if (fill_col1 || p >= 0) && (fill_col2 || p < 255) {
            let col = interpolate_color(p.clamp(0, 255) as u32, color1, color2);
            TargetPixel::blend_slice(buffer, col);
        }
        return;
    }

    let size_x = (rect.width() + g.left_clip.get() + g.right_clip.get()) as i32;

    let mut x = if invert_slope {
        (y * size_x * (255 - start)) / (size_y * start)
    } else {
        (size_y - y) * size_x * (255 - start) / (size_y * start)
    } + g.left_clip.get() as i32
        + extra_left_clip as i32;

    let len = ((255 * size_x) / start) as usize;

    if x < 0 {
        let l = (-x as usize).min(buffer.len());
        if invert_slope {
            if fill_col1 {
                TargetPixel::blend_slice(&mut buffer[..l], g.color1);
            }
        } else if fill_col2 {
            TargetPixel::blend_slice(&mut buffer[..l], g.color2);
        }
        buffer = &mut buffer[l..];
        x = 0;
    }

    if buffer.len() + x as usize > len {
        let l = len.saturating_sub(x as usize);
        if invert_slope {
            if fill_col2 {
                TargetPixel::blend_slice(&mut buffer[l..], g.color2);
            }
        } else if fill_col1 {
            TargetPixel::blend_slice(&mut buffer[l..], g.color1);
        }
        buffer = &mut buffer[..l];
    }

    if buffer.is_empty() {
        return;
    }

    if !invert_slope {
        core::mem::swap(&mut color1, &mut color2);
    }

    let dr = (((color2.red as i32 - color1.red as i32) * start) << 15) / (255 * size_x);
    let dg = (((color2.green as i32 - color1.green as i32) * start) << 15) / (255 * size_x);
    let db = (((color2.blue as i32 - color1.blue as i32) * start) << 15) / (255 * size_x);
    let da = (((color2.alpha as i32 - color1.alpha as i32) * start) << 15) / (255 * size_x);

    let mut r = ((color1.red as u32) << 15).wrapping_add((x * dr) as _);
    let mut g = ((color1.green as u32) << 15).wrapping_add((x * dg) as _);
    let mut b = ((color1.blue as u32) << 15).wrapping_add((x * db) as _);
    let mut a = ((color1.alpha as u32) << 15).wrapping_add((x * da) as _);

    if color1.alpha == 255 && color2.alpha == 255 {
        buffer.fill_with(|| {
            let pix = TargetPixel::from_rgb((r >> 15) as u8, (g >> 15) as u8, (b >> 15) as u8);
            r = r.wrapping_add(dr as _);
            g = g.wrapping_add(dg as _);
            b = b.wrapping_add(db as _);
            pix
        })
    } else {
        for pix in buffer {
            pix.blend(PremultipliedRgbaColor {
                red: (r >> 15) as u8,
                green: (g >> 15) as u8,
                blue: (b >> 15) as u8,
                alpha: (a >> 15) as u8,
            });
            r = r.wrapping_add(dr as _);
            g = g.wrapping_add(dg as _);
            b = b.wrapping_add(db as _);
            a = a.wrapping_add(da as _);
        }
    }
}

/// Draw a radial gradient on a line
pub(super) fn draw_radial_gradient(
    rect: &PhysicalRect,
    line: PhysicalLength,
    g: &super::RadialGradientCommand,
    buffer: &mut [impl TargetPixel],
    extra_left_clip: i16,
    _extra_right_clip: i16,
) {
    if g.stops.is_empty() {
        return;
    }

    let center_x = rect.min_x() as f32 + g.center_x;
    let center_y = rect.min_y() as f32 + g.center_y;

    debug_assert!(
        g.radius >= 0.0,
        "radius must be resolved before constructing RadialGradientCommand"
    );
    let max_radius = g.radius.max(f32::EPSILON);

    let start_x = rect.min_x() + extra_left_clip;
    let dy = line.get() as f32 - center_y;
    let dy_squared = dy * dy;

    for (i, pixel) in buffer.iter_mut().enumerate() {
        let x = start_x + i as i16;
        let dx = x as f32 - center_x;
        let distance = (dx * dx + dy_squared).sqrt();
        let position = (distance / max_radius).clamp(0.0, 1.0);

        // Find the two gradient stops to interpolate between
        let mut color = g.stops.first().map(|s| s.color).unwrap_or_default();

        for window in g.stops.windows(2) {
            let stop1 = &window[0];
            let stop2 = &window[1];

            if position >= stop1.position && position <= stop2.position {
                // Interpolate between the two stops
                let t = if stop2.position == stop1.position {
                    0.0
                } else {
                    (position - stop1.position) / (stop2.position - stop1.position)
                };

                let c1 = stop1.color.to_argb_u8();
                let c2 = stop2.color.to_argb_u8();

                let alpha = ((1.0 - t) * c1.alpha as f32 + t * c2.alpha as f32) as u8;
                let red = ((1.0 - t) * c1.red as f32 + t * c2.red as f32) as u8;
                let green = ((1.0 - t) * c1.green as f32 + t * c2.green as f32) as u8;
                let blue = ((1.0 - t) * c1.blue as f32 + t * c2.blue as f32) as u8;

                color = Color::from_argb_u8(alpha, red, green, blue);
                break;
            } else if position > stop2.position {
                color = stop2.color;
            }
        }

        pixel.blend(super::PremultipliedRgbaColor::from(color));
    }
}

/// Draw a conic gradient on a line
pub(super) fn draw_conic_gradient(
    rect: &PhysicalRect,
    line: PhysicalLength,
    g: &super::ConicGradientCommand,
    buffer: &mut [impl TargetPixel],
    extra_left_clip: i16,
    _extra_right_clip: i16,
) {
    if g.stops.is_empty() {
        return;
    }

    let center_x = rect.min_x() as f32 + g.center_x;
    let center_y = rect.min_y() as f32 + g.center_y;

    let start_x = rect.min_x() + extra_left_clip;
    let y = line.get() as f32;

    for (i, pixel) in buffer.iter_mut().enumerate() {
        let x = (start_x + i as i16) as f32;

        // Calculate angle from center to current pixel
        let dx = x - center_x;
        let dy = y - center_y;

        // atan2 returns angle in radians from -π to π
        // For 0deg at north (12 o'clock), we need to rotate by -90 degrees
        let mut angle = dy.atan2(dx) + core::f32::consts::FRAC_PI_2;

        // Normalize angle to [0, 2π]
        while angle < 0.0 {
            angle += 2.0 * core::f32::consts::PI;
        }
        while angle >= 2.0 * core::f32::consts::PI {
            angle -= 2.0 * core::f32::consts::PI;
        }

        // Convert to position in [0, 1]
        let position = angle / (2.0 * core::f32::consts::PI);

        // Find the two gradient stops to interpolate between
        let mut color = g.stops.first().map(|s| s.color).unwrap_or_default();

        for window in g.stops.windows(2) {
            let stop1 = &window[0];
            let stop2 = &window[1];

            if position >= stop1.position && position <= stop2.position {
                // Interpolate between the two stops
                let t = if stop2.position == stop1.position {
                    0.0
                } else {
                    (position - stop1.position) / (stop2.position - stop1.position)
                };

                let c1 = stop1.color.to_argb_u8();
                let c2 = stop2.color.to_argb_u8();

                let alpha = ((1.0 - t) * c1.alpha as f32 + t * c2.alpha as f32) as u8;
                let red = ((1.0 - t) * c1.red as f32 + t * c2.red as f32) as u8;
                let green = ((1.0 - t) * c1.green as f32 + t * c2.green as f32) as u8;
                let blue = ((1.0 - t) * c1.blue as f32 + t * c2.blue as f32) as u8;

                color = Color::from_argb_u8(alpha, red, green, blue);
                break;
            } else if position > stop2.position {
                color = stop2.color;
            }
        }

        pixel.blend(super::PremultipliedRgbaColor::from(color));
    }
}

/// A color whose component have been pre-multiplied by alpha
///
/// The renderer operates faster on pre-multiplied color since it
/// caches the multiplication of its component
///
/// PremultipliedRgbaColor can be constructed from a [`Color`] with
/// the [`From`] trait. This conversion will pre-multiply the color
/// components
#[allow(missing_docs)]
#[derive(Clone, Copy, Debug, Default, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct PremultipliedRgbaColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

/// Convert a non-premultiplied color to a premultiplied one
impl From<Color> for PremultipliedRgbaColor {
    fn from(col: Color) -> Self {
        Self::premultiply(col)
    }
}

impl PremultipliedRgbaColor {
    /// Convert a non premultiplied color to a premultiplied one
    fn premultiply(col: Color) -> Self {
        let a = col.alpha() as u16;
        Self {
            alpha: col.alpha(),
            red: (col.red() as u16 * a / 255) as u8,
            green: (col.green() as u16 * a / 255) as u8,
            blue: (col.blue() as u16 * a / 255) as u8,
        }
    }
}

/// Trait for the pixels in the buffer
#[cfg(test)]
mod arc_line_tests {
    use super::*;
    use alloc::vec;
    use alloc::vec::Vec;
    use i_slint_core::graphics::Rgb8Pixel;

    // A 466px dial with a 12px stroke, which is what the firmware actually draws. At this
    // size most rows pass through the ring's hole and split into a left and a right run;
    // a 22px test arc barely exercises that at all.
    const C: i16 = 233;
    const OUTER: i16 = 233;
    const INNER: i16 = 221;
    const W: usize = 466;

    fn arc(start_deg: f32, sweep_deg: f32) -> super::super::ArcCommand {
        let unit = |d: f32| {
            let r = d.to_radians();
            (r.cos(), r.sin())
        };
        super::super::ArcCommand {
            center_x: PhysicalLength::new(C),
            center_y: PhysicalLength::new(C),
            outer_radius: PhysicalLength::new(OUTER),
            inner_radius: PhysicalLength::new(INNER),
            // #ff8800, the gauge indicator colour. White quantises exactly in 565 and so
            // cannot show a rounding difference between the two fill paths.
            color: PremultipliedRgbaColor { alpha: 255, red: 255, green: 136, blue: 0 },
            start_dir: unit(start_deg),
            end_dir: unit(start_deg + sweep_deg),
            reflex: sweep_deg.abs() > 180.,
            full_circle: sweep_deg.abs() >= 360.,
            round_caps: false,
            cap_radius: PhysicalLength::new((OUTER - INNER) / 2),
            start_cap: (PhysicalLength::new(0), PhysicalLength::new(0)),
            end_cap: (PhysicalLength::new(0), PhysicalLength::new(0)),
        }
    }

    /// Painted x positions on one row of a full-width buffer.
    fn painted(a: &super::super::ArcCommand, row: i16) -> Vec<usize> {
        let span = PhysicalRect::new(euclid::point2(0, 0), euclid::size2(W as i16, W as i16));
        let mut buf = vec![Rgb8Pixel { r: 0, g: 0, b: 0 }; W];
        draw_arc_line(&span, PhysicalLength::new(row), a, &mut buf, 0);
        buf.iter().enumerate().filter(|(_, p)| p.r > 40).map(|(i, _)| i).collect()
    }

    /// Painted x positions on one row when the buffer covers only [from, to) of the span,
    /// which is what a narrowed dirty region hands over.
    fn painted_clipped(
        a: &super::super::ArcCommand,
        row: i16,
        from: usize,
        to: usize,
    ) -> Vec<usize> {
        let span = PhysicalRect::new(euclid::point2(0, 0), euclid::size2(W as i16, W as i16));
        let mut buf = vec![Rgb8Pixel { r: 0, g: 0, b: 0 }; to - from];
        draw_arc_line(&span, PhysicalLength::new(row), a, &mut buf, from as i16);
        buf.iter().enumerate().filter(|(_, p)| p.r > 40).map(|(i, _)| i + from).collect()
    }

    /// Drawing clipped to a narrow window must paint exactly what the full-width pass paints
    /// inside that window.
    ///
    /// This is the difference between a narrowed dirty region and a full-element one, and it
    /// is invisible on a full repaint: any pixel the clipped pass drops is simply left as the
    /// previous frame had it, which reads as a gap in the arc that repairs itself later.
    #[test]
    fn clipped_drawing_matches_full_width() {
        for (start, sweep) in [
            (140., 60.),
            (140., 200.),
            (330., 60.),
            (45., 90.),
            (0., 30.),
            (170., 20.),
        ] {
            let a = arc(start, sweep);
            for row in [C - OUTER + 3, C - 120, C - 1, C, C + 1, C + 120, C + OUTER - 3] {
                let full = painted(&a, row);
                // Windows deliberately cutting through the arc, including odd edges.
                for &(from, to) in &[
                    (0usize, 120usize),
                    (100, 240),
                    (101, 241),
                    (200, 300),
                    (233, 400),
                    (300, 466),
                    (111, 355),
                ] {
                    let clipped = painted_clipped(&a, row, from, to);
                    let expect: Vec<usize> =
                        full.iter().copied().filter(|x| *x >= from && *x < to).collect();
                    assert_eq!(
                        clipped, expect,
                        "start={start} sweep={sweep} row={row} window={from}..{to}"
                    );
                }
            }
        }
    }

    /// Like `arc`, but with the round caps the firmware's gauges actually use. The cap
    /// centres sit on the stroke's centre line at each end of the sweep.
    fn arc_round(start_deg: f32, sweep_deg: f32) -> super::super::ArcCommand {
        let mut a = arc(start_deg, sweep_deg);
        a.round_caps = true;
        let mid = ((OUTER + INNER) / 2) as f32;
        let at = |d: f32| {
            let r = d.to_radians();
            (
                PhysicalLength::new((C as f32 + mid * r.cos()) as i16),
                PhysicalLength::new((C as f32 + mid * r.sin()) as i16),
            )
        };
        a.start_cap = at(start_deg);
        a.end_cap = at(start_deg + sweep_deg);
        a
    }

    /// Rgb565, not Rgb8: the interior of a span is filled by `blend_slice` while its end
    /// pixels go through `blend_coverage`, and those two only round to different values once
    /// the result is quantised to 5/6/5. An Rgb8 buffer hides the whole class of defect.
    fn row_values(a: &super::super::ArcCommand, row: i16, from: usize, to: usize) -> Vec<u16> {
        let span = PhysicalRect::new(euclid::point2(0, 0), euclid::size2(W as i16, W as i16));
        let mut buf = vec![Rgb565Pixel(0); to - from];
        draw_arc_line(&span, PhysicalLength::new(row), a, &mut buf, from as i16);
        buf.iter().map(|p| p.0).collect()
    }

    /// Clipping must not change the *value* of any pixel, only which ones are offered.
    ///
    /// `clipped_drawing_matches_full_width` compares sets of painted indices with an
    /// `r > 40` threshold, so it cannot see a pixel that is painted in both passes but with
    /// different coverage. That is exactly the defect: the antialiased pixel at the edge of
    /// a clipped window gets coverage measured against the window instead of against the
    /// arc, and the one-step colour difference is left on the panel because nothing repaints
    /// that pixel afterwards.
    #[test]
    fn clipped_drawing_matches_full_width_including_coverage() {
        let mut failures = Vec::new();
        for (start, sweep) in
            [(140., 60.), (140., 200.), (330., 60.), (45., 90.), (0., 30.), (170., 20.), (90., 20.)]
        {
            for a in [arc(start, sweep), arc_round(start, sweep)] {
                for row in [C - OUTER + 3, C - 120, C - 1, C, C + 1, C + 120, C + OUTER - 3] {
                    let full = row_values(&a, row, 0, W);
                    for from in [0usize, 100, 101, 200, 233, 300, 111] {
                        for len in [1usize, 2, 7, 44, 120, 166] {
                            let to = (from + len).min(W);
                            if to <= from {
                                continue;
                            }
                            let clipped = row_values(&a, row, from, to);
                            for (i, px) in clipped.iter().enumerate() {
                                if *px != full[from + i] {
                                    failures.push(alloc::format!(
                                        "start={start} sweep={sweep} caps={} row={row}                                          window={from}..{to} x={} clipped=0x{:04x} full=0x{:04x}",
                                        a.round_caps,
                                        from + i,
                                        px,
                                        full[from + i]
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} clipped pixels differ in value from the full-width pass; first 10:
{}",
            failures.len(),
            failures.iter().take(10).cloned().collect::<Vec<_>>().join("
")
        );
    }

    fn contiguous_groups(xs: &[usize]) -> Vec<(usize, usize)> {
        let mut out: Vec<(usize, usize)> = vec![];
        for &x in xs {
            match out.last_mut() {
                Some(g) if x == g.1 + 1 => g.1 = x,
                _ => out.push((x, x)),
            }
        }
        out
    }

    /// The row through the centre crosses the hole, so a full ring must paint two runs
    /// there and nothing between them.
    #[test]
    fn centre_row_of_a_full_ring_has_two_runs() {
        let groups = contiguous_groups(&painted(&arc(0., 360.), C));
        assert_eq!(groups.len(), 2, "expected a left and a right run, got {groups:?}");
        assert!(groups[0].0 <= 1 && groups[0].1 >= 10, "left run {:?}", groups[0]);
        assert!(groups[1].1 >= W - 2 && groups[1].0 <= W - 11, "right run {:?}", groups[1]);
    }

    /// 150..210 degrees is the 8 to 10 o'clock sector: the left run only, on every row
    /// that passes through the hole.
    #[test]
    fn left_sector_paints_only_the_left_run() {
        let a = arc(150., 60.);
        for row in [C - 100, C - 20, C, C + 20, C + 100] {
            let groups = contiguous_groups(&painted(&a, row));
            assert!(!groups.is_empty(), "row {row}: nothing painted");
            for g in &groups {
                assert!(g.1 < C as usize, "row {row}: painted right of centre at {g:?}");
            }
        }
    }

    /// 330..30 degrees is 2 to 4 o'clock: the right run only.
    #[test]
    fn right_sector_paints_only_the_right_run() {
        let a = arc(330., 60.);
        for row in [C - 100, C - 20, C, C + 20, C + 100] {
            let groups = contiguous_groups(&painted(&a, row));
            assert!(!groups.is_empty(), "row {row}: nothing painted");
            for g in &groups {
                assert!(g.0 > C as usize, "row {row}: painted left of centre at {g:?}");
            }
        }
    }

    /// The row a boundary ray passes through must still be painted. An arc ending on the
    /// horizontal has its outermost pixels on that row, and losing it shows as a nick at 3
    /// or 9 o'clock. Both tips of a half turn arc live on it at once.
    #[test]
    fn the_row_a_horizontal_ray_passes_through_is_painted() {
        // Sweeps that put a ray exactly on the horizontal, from either side.
        for (start, sweep, expect_left, expect_right) in [
            (0., 90., false, true),    // starts at 3 o'clock, sweeps down
            (270., 90., false, true),  // ends at 3 o'clock
            (180., 90., true, false),  // starts at 9 o'clock
            (90., 90., true, false),   // ends at 9 o'clock
            (180., 180., true, true),  // both tips on the row
            (0., 180., true, true),
        ] {
            let a = arc(start, sweep);
            // The exact horizontal falls between the rows at dy = -0.5 and +0.5, so the tip
            // legitimately sits on one or the other depending on which way the arc sweeps.
            // What must not happen is it being missing from both.
            let mut groups = contiguous_groups(&painted(&a, C - 1));
            groups.extend(contiguous_groups(&painted(&a, C)));
            let has_left = groups.iter().any(|g| g.1 < C as usize);
            let has_right = groups.iter().any(|g| g.0 > C as usize);
            assert!(
                has_left == expect_left && has_right == expect_right,
                "start={start} sweep={sweep}: left={has_left} right={has_right},                  wanted left={expect_left} right={expect_right} (groups {groups:?})"
            );
        }
    }

    /// Nothing may be painted outside the sector. The tests above only check that rows are
    /// not skipped, which cannot see over-painting - and a reflex sweep takes the union of
    /// two half planes, where painting outside the wedge is the natural way to be wrong.
    /// A dial spanning 140..400 degrees leaves a gap around 6 o'clock, so anything drawn
    /// there is arc where there is meant to be none.
    #[test]
    fn nothing_is_painted_outside_the_sector() {
        for (start, sweep) in [(140., 260.), (140., 200.), (140., 190.), (150., 60.), (0., 359.)] {
            let a = arc(start, sweep);
            let mut bad = vec![];
            for row in (C - OUTER + 1)..(C + OUTER - 1) {
                for x in painted(&a, row) {
                    let dx = x as f32 + 0.5 - C as f32;
                    let dy = row as f32 + 0.5 - C as f32;
                    let r = (dx * dx + dy * dy).sqrt();
                    // Ignore the anti-aliased fringe just outside the ring's radii.
                    if r < INNER as f32 - 1.5 || r > OUTER as f32 + 1.5 {
                        continue;
                    }
                    let ang = dy.atan2(dx).to_degrees();
                    // Rotation from the sweep's start, allowing a couple of degrees for
                    // the caps and anti-aliasing at each end.
                    let rel = (ang - start).rem_euclid(360.);
                    if rel > sweep + 3. && rel < 360. - 3. {
                        bad.push((x, row, rel as i32));
                    }
                }
            }
            assert!(
                bad.is_empty(),
                "start={start} sweep={sweep}: {} px painted outside the sector, e.g. {:?}",
                bad.len(),
                &bad[..bad.len().min(6)]
            );
        }
    }

    /// Every row the ring covers must be painted somewhere, for sectors sitting on each
    /// horizontal axis and for a reflex sweep spanning both.
    #[test]
    fn no_row_of_a_sector_is_skipped() {
        for (start, sweep) in [(150., 60.), (330., 60.), (140., 260.), (140., 200.)] {
            let a = arc(start, sweep);
            let mut blank = vec![];
            for row in (C - OUTER + 1)..(C + OUTER - 1) {
                // Rows the sector genuinely does not reach are not a failure; only look at
                // rows where some angle in the sweep has that y.
                let reaches = (0..=(sweep as i32)).any(|k| {
                    let ang = (start + k as f32).to_radians();
                    let y = C as f32 + (INNER as f32 + 6.) * ang.sin();
                    (y.round() as i16 - row).abs() <= 1
                });
                if reaches && painted(&a, row).is_empty() {
                    blank.push(row);
                }
            }
            assert!(blank.is_empty(), "start={start} sweep={sweep}: unpainted rows {blank:?}");
        }
    }
}

pub trait TargetPixel: Sized + Copy {
    /// Blend a single pixel with a color
    fn blend(&mut self, color: PremultipliedRgbaColor);
    /// Blend a color to all the pixel in the slice.
    fn blend_slice(slice: &mut [Self], color: PremultipliedRgbaColor) {
        if color.alpha == u8::MAX {
            slice.fill(Self::from_rgb(color.red, color.green, color.blue))
        } else {
            for x in slice {
                Self::blend(x, color);
            }
        }
    }
    /// Create a pixel from the red, gree, blue component in the range 0..=255
    fn from_rgb(red: u8, green: u8, blue: u8) -> Self;

    /// Pixel which will be filled as the background in case the slint view has transparency
    fn background() -> Self {
        Self::from_rgb(0, 0, 0)
    }
}

impl TargetPixel for Rgb8Pixel {
    fn blend(&mut self, color: PremultipliedRgbaColor) {
        let a = (u8::MAX - color.alpha) as u16;
        self.r = (self.r as u16 * a / 255) as u8 + color.red;
        self.g = (self.g as u16 * a / 255) as u8 + color.green;
        self.b = (self.b as u16 * a / 255) as u8 + color.blue;
    }

    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self::new(r, g, b)
    }
}

impl TargetPixel for PremultipliedRgbaColor {
    fn blend(&mut self, color: PremultipliedRgbaColor) {
        let a = (u8::MAX - color.alpha) as u16;
        self.red = (self.red as u16 * a / 255) as u8 + color.red;
        self.green = (self.green as u16 * a / 255) as u8 + color.green;
        self.blue = (self.blue as u16 * a / 255) as u8 + color.blue;
        self.alpha = (self.alpha as u16 + color.alpha as u16
            - (self.alpha as u16 * color.alpha as u16) / 255) as u8;
    }

    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self { red: r, green: g, blue: b, alpha: 255 }
    }

    fn background() -> Self {
        Self { red: 0, green: 0, blue: 0, alpha: 0 }
    }
}

/// A 16bit pixel that has 5 red bits, 6 green bits and  5 blue bits
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Rgb565Pixel(pub u16);

impl Rgb565Pixel {
    const R_MASK: u16 = 0b1111_1000_0000_0000;
    const G_MASK: u16 = 0b0000_0111_1110_0000;
    const B_MASK: u16 = 0b0000_0000_0001_1111;

    /// Return the red component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    fn red(self) -> u8 {
        ((self.0 & Self::R_MASK) >> 8) as u8
    }
    /// Return the green component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    fn green(self) -> u8 {
        ((self.0 & Self::G_MASK) >> 3) as u8
    }
    /// Return the blue component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    fn blue(self) -> u8 {
        ((self.0 & Self::B_MASK) << 3) as u8
    }
}

impl TargetPixel for Rgb565Pixel {
    /// Opaque fills are most of the pixels in a frame, and the default `slice.fill` lowers to
    /// a non-unrolled 16-bit store loop on Xtensa. Pairing the pixels into 32-bit stores
    /// halves the store count for the aligned middle.
    fn blend_slice(slice: &mut [Self], color: PremultipliedRgbaColor) {
        if color.alpha == u8::MAX {
            let p = Self::from_rgb(color.red, color.green, color.blue);
            let (head, mid, tail) = bytemuck::pod_align_to_mut::<Self, u32>(slice);
            head.fill(p);
            mid.fill((p.0 as u32) | ((p.0 as u32) << 16));
            tail.fill(p);
        } else {
            for x in slice {
                Self::blend(x, color);
            }
        }
    }

    fn blend(&mut self, color: PremultipliedRgbaColor) {
        let a = (u8::MAX - color.alpha) as u32;
        // convert to 5 bits
        let a = (a + 4) >> 3;

        // 00000ggg_ggg00000_rrrrr000_000bbbbb
        let expanded = (self.0 & (Self::R_MASK | Self::B_MASK)) as u32
            | (((self.0 & Self::G_MASK) as u32) << 16);

        // gggggggg_000rrrrr_rrr000bb_bbbbbb00
        let c =
            ((color.red as u32) << 13) | ((color.green as u32) << 24) | ((color.blue as u32) << 2);
        // gggggg00_000rrrrr_000000bb_bbb00000
        let c = c & 0b11111100_00011111_00000011_11100000;

        let res = expanded * a + c;

        self.0 = ((res >> 21) as u16 & Self::G_MASK)
            | ((res >> 5) as u16 & (Self::R_MASK | Self::B_MASK));
    }

    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self(((r as u16 & 0b11111000) << 8) | ((g as u16 & 0b11111100) << 3) | (b as u16 >> 3))
    }
}

#[cfg(test)]
mod rgb565_fill_tests {
    use super::*;
    use alloc::vec;

    /// The widened fill writes the aligned middle as 32-bit pairs, so it has to agree with a
    /// plain per-pixel fill at every start offset and length - including the odd head and tail
    /// that fall outside the pairing.
    #[test]
    fn widened_opaque_fill_matches_per_pixel() {
        let opaque = |r, g, b| PremultipliedRgbaColor { red: r, green: g, blue: b, alpha: 255 };
        for color in [opaque(0, 0, 0), opaque(255, 255, 255), opaque(31, 200, 97)] {
            let want = Rgb565Pixel::from_rgb(color.red, color.green, color.blue);
            for len in 0..24usize {
                for off in 0..4usize {
                    let mut buf = vec![Rgb565Pixel(0xdead); off + len];
                    Rgb565Pixel::blend_slice(&mut buf[off..], color);
                    for (i, px) in buf.iter().enumerate() {
                        let expect = if i < off { Rgb565Pixel(0xdead) } else { want };
                        assert_eq!(*px, expect, "color={color:?} len={len} off={off} i={i}");
                    }
                }
            }
        }
    }

    /// Translucent colours must still go through the per-pixel blend.
    #[test]
    fn translucent_fill_still_blends() {
        let c = PremultipliedRgbaColor { red: 40, green: 40, blue: 40, alpha: 128 };
        let mut widened = vec![Rgb565Pixel(0x1234); 9];
        let mut manual = widened.clone();
        Rgb565Pixel::blend_slice(&mut widened, c);
        for px in manual.iter_mut() {
            px.blend(c);
        }
        assert_eq!(widened, manual);
    }
}

impl From<Rgb8Pixel> for Rgb565Pixel {
    fn from(p: Rgb8Pixel) -> Self {
        Self::from_rgb(p.r, p.g, p.b)
    }
}

impl From<Rgb565Pixel> for Rgb8Pixel {
    fn from(p: Rgb565Pixel) -> Self {
        Rgb8Pixel { r: p.red(), g: p.green(), b: p.blue() }
    }
}

// cSpell: ignore RRRRRGGG GGGBBBBB bswap

/// A 16bit RGB565 pixel stored in big-endian byte order.
///
/// The in-memory byte layout is `[RRRRRGGG, GGGBBBBB]` regardless of
/// host endianness — the format expected by most SPI display
/// controllers (ILI9341, ILI9342C, ST7789, etc.) without any
/// post-render byte swapping.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Rgb565BigEndianPixel(pub u16);

impl Rgb565BigEndianPixel {
    /// Return the red component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    pub fn red(self) -> u8 {
        Rgb565Pixel(u16::from_be(self.0)).red()
    }
    /// Return the green component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    pub fn green(self) -> u8 {
        Rgb565Pixel(u16::from_be(self.0)).green()
    }
    /// Return the blue component as a u8.
    ///
    /// The bits are shifted so that the result is between 0 and 255
    pub fn blue(self) -> u8 {
        Rgb565Pixel(u16::from_be(self.0)).blue()
    }
}

impl TargetPixel for Rgb565BigEndianPixel {
    /// Same widening as the native-endian case: the fill value is byte-swapped once up
    /// front, so pairing pixels into 32-bit stores costs nothing extra here.
    fn blend_slice(slice: &mut [Self], color: PremultipliedRgbaColor) {
        if color.alpha == u8::MAX {
            let p = Self::from_rgb(color.red, color.green, color.blue);
            let (head, mid, tail) = bytemuck::pod_align_to_mut::<Self, u32>(slice);
            head.fill(p);
            mid.fill((p.0 as u32) | ((p.0 as u32) << 16));
            tail.fill(p);
        } else {
            for x in slice {
                Self::blend(x, color);
            }
        }
    }

    fn blend(&mut self, color: PremultipliedRgbaColor) {
        // Reuse the canonical native-endian Rgb565Pixel::blend by decoding
        // from BE byte order, blending, and re-encoding. On targets with a
        // byte-swap instruction (ARM REV16, RISC-V Zbb rev8, x86 bswap) each
        // `to_be`/`from_be` is one cycle on a little-endian host and a no-op
        // on a big-endian host. Benchmarking this against a direct-BE
        // bit-reassembly variant on Cortex-M33 showed the swap-around-native
        // form generates tighter code.
        let mut native = Rgb565Pixel(u16::from_be(self.0));
        native.blend(color);
        self.0 = native.0.to_be();
    }

    fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self(Rgb565Pixel::from_rgb(r, g, b).0.to_be())
    }
}

impl From<Rgb8Pixel> for Rgb565BigEndianPixel {
    fn from(p: Rgb8Pixel) -> Self {
        Self(Rgb565Pixel::from(p).0.to_be())
    }
}

impl From<Rgb565BigEndianPixel> for Rgb8Pixel {
    fn from(p: Rgb565BigEndianPixel) -> Self {
        Rgb565Pixel(u16::from_be(p.0)).into()
    }
}

#[test]
fn rgb565() {
    let pix565 = Rgb565Pixel::from_rgb(0xff, 0x25, 0);
    let pix888: Rgb8Pixel = pix565.into();
    assert_eq!(pix565, pix888.into());

    let pix565 = Rgb565Pixel::from_rgb(0x56, 0x42, 0xe3);
    let pix888: Rgb8Pixel = pix565.into();
    assert_eq!(pix565, pix888.into());
}

#[test]
fn rgb565_be() {
    // BE should be byte-swapped LE for any color
    for &(r, g, b) in &[(0xff, 0x25, 0u8), (0x56, 0x42, 0xe3), (0, 0xff, 0), (0, 0, 0xff)] {
        let le = Rgb565Pixel::from_rgb(r, g, b);
        let be = Rgb565BigEndianPixel::from_rgb(r, g, b);
        assert_eq!(le.0.swap_bytes(), be.0, "mismatch for ({r}, {g}, {b})");
    }

    // Round-trip through Rgb8Pixel
    let pix_be = Rgb565BigEndianPixel::from_rgb(0xff, 0x25, 0);
    let pix888: Rgb8Pixel = pix_be.into();
    assert_eq!(pix_be, pix888.into());

    let pix_be = Rgb565BigEndianPixel::from_rgb(0x56, 0x42, 0xe3);
    let pix888: Rgb8Pixel = pix_be.into();
    assert_eq!(pix_be, pix888.into());
}

#[test]
fn rgb565_be_blend() {
    // Blending a BE pixel should produce the same visual result as LE
    let color = PremultipliedRgbaColor { red: 127, green: 0, blue: 0, alpha: 127 };

    let mut le = Rgb565Pixel::from_rgb(0, 0, 255);
    le.blend(color);
    let mut be = Rgb565BigEndianPixel::from_rgb(0, 0, 255);
    be.blend(color);
    assert_eq!(le.0.swap_bytes(), be.0);

    // Blend with green over a white background
    let color = PremultipliedRgbaColor { red: 0, green: 200, blue: 0, alpha: 200 };

    let mut le = Rgb565Pixel::from_rgb(255, 255, 255);
    le.blend(color);
    let mut be = Rgb565BigEndianPixel::from_rgb(255, 255, 255);
    be.blend(color);
    assert_eq!(le.0.swap_bytes(), be.0);
}

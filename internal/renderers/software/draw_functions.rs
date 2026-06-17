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
                        let a = ((data[pos + 3] as u16 * alpha as u16) / 255) as u8;
                        if a > 0 {
                            let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                                a,
                                data[pos + 0],
                                data[pos + 1],
                                data[pos + 2],
                            ));
                            if a == 255 {
                                *pix = TargetPixel::from_rgb(c.red, c.green, c.blue);
                            } else {
                                pix.blend(c);
                            }
                        }
                    }
                } else {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let a = ((data[pos + 3] as u16 * alpha as u16) / 255) as u8;
                        if a > 0 {
                            let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                                a,
                                color.red(),
                                color.green(),
                                color.blue(),
                            ));
                            if a == 255 {
                                *pix = TargetPixel::from_rgb(c.red, c.green, c.blue);
                            } else {
                                pix.blend(c);
                            }
                        }
                    }
                }
            }
            TexturePixelFormat::RgbaPremultiplied => {
                if color.alpha() > 0 {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let a = ((data[pos + 3] as u16 * alpha as u16) / 255) as u8;
                        if a > 0 {
                            let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                                a,
                                color.red(),
                                color.green(),
                                color.blue(),
                            ));
                            if a == 255 {
                                *pix = TargetPixel::from_rgb(c.red, c.green, c.blue);
                            } else {
                                pix.blend(c);
                            }
                        }
                    }
                } else if alpha == 0xff {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let a = data[pos + 3];
                        if a > 0 {
                            let c = PremultipliedRgbaColor {
                                alpha: a,
                                red: data[pos + 0],
                                green: data[pos + 1],
                                blue: data[pos + 2],
                            };
                            if a == 255 {
                                *pix = TargetPixel::from_rgb(c.red, c.green, c.blue);
                            } else {
                                pix.blend(c);
                            }
                        }
                    }
                } else {
                    for pix in line_buffer {
                        let pos = pos(4).0;
                        let a = (data[pos + 3] as u16 * alpha as u16 / 255) as u8;
                        if a > 0 {
                            let c = PremultipliedRgbaColor {
                                alpha: a,
                                red: (data[pos + 0] as u16 * alpha as u16 / 255) as u8,
                                green: (data[pos + 1] as u16 * alpha as u16 / 255) as u8,
                                blue: (data[pos + 2] as u16 * alpha as u16 / 255) as u8,
                            };
                            if a == 255 {
                                *pix = TargetPixel::from_rgb(c.red, c.green, c.blue);
                            } else {
                                pix.blend(c);
                            }
                        }
                    }
                }
            }
            TexturePixelFormat::AlphaMap => {
                for pix in line_buffer {
                    let pos = pos(1).0;
                    #[cfg(feature = "disable-aa")]
                    let text_alpha = if data[pos] < 128 { 0 } else { alpha };
                    #[cfg(not(feature = "disable-aa"))]
                    let text_alpha = ((data[pos] as u16 * alpha as u16) / 255) as u8;

                    if text_alpha > 0 {
                        let c = PremultipliedRgbaColor::premultiply(Color::from_argb_u8(
                            text_alpha,
                            color.red(),
                            color.green(),
                            color.blue(),
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
    #[cfg(feature = "disable-aa")]
    let anti_alias = |x1: Shifted, x2: Shifted, process_pixel: &mut dyn FnMut(usize, u32)| {
        let mid = (x1.0 + x2.0) >> 1;
        for x in x1.floor()..x2.ceil() {
            let cov = if (x << 4) < mid { 0 } else { 255 };
            process_pixel(x as usize, cov);
        }
    };
    #[cfg(not(feature = "disable-aa"))]
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
    if a == 0 {
        return color1;
    }
    if a == 255 {
        return color2;
    }
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

#[cfg(feature = "path")]
#[derive(Debug)]
pub struct ArcParameters {
    pub stroke_width: f32,
    pub r_mid: f32,
    pub r_outer: f32,
    pub r_out_bound: f32,
    pub r_in_bound: f32,
    pub r_out_sq: f32,
    pub r_in_sq: f32,
    pub r_outer_sq: f32,
    pub inv_2_r_outer: f32,
    pub r_inner: f32,
    pub r_inner_sq: f32,
    pub inv_2_r_inner: f32,
    pub r_cap: f32,
    pub r_cap_sq: f32,
    pub inv_2_r_cap: f32,
    pub cap_r_in: f32,
    pub cap_r_in_sq: f32,
    pub cap_r_out: f32,
    pub cap_r_out_sq: f32,
    pub x_center: f32,
    pub y_center: f32,
    pub start_angle: f32,
    pub end_angle: f32,
    pub start_cos: f32,
    pub start_sin: f32,
    pub end_cos: f32,
    pub end_sin: f32,
    pub sweep: f32,
    pub is_full_circle: bool,
    pub is_sweep_less_180: bool,
    pub arc_min_y: f32,
    pub arc_max_y: f32,
    pub cap_start_x: f32,
    pub cap_start_y: f32,
    pub cap_end_x: f32,
    pub cap_end_y: f32,
    pub r_out_solid_sq: f32,
    pub r_in_solid_sq: f32,
    pub cap_expansion: f32,
    pub inv_start_sin: f32,
    pub inv_end_sin: f32,
}

#[cfg(feature = "path")]
impl ArcParameters {
    pub fn new(
        span: &PhysicalRect,
        stroke_width: f32,
        start_angle_rad: f32,
        end_angle_rad: f32,
        mut start_cos: f32,
        mut start_sin: f32,
        mut end_cos: f32,
        mut end_sin: f32,
    ) -> Self {
        let w = span.size.width as f32;
        let h = span.size.height as f32;
        let r_mid = (w.min(h) - stroke_width).max(0.0) / 2.0;
        let r_outer = r_mid + stroke_width / 2.0;

        let r_out_bound = r_outer + 0.5;
        let r_in_bound = (r_mid - stroke_width / 2.0 - 0.5).max(0.0);
        let r_out_sq = r_out_bound * r_out_bound;
        let r_in_sq = r_in_bound * r_in_bound;

        let r_outer_sq = r_outer * r_outer;
        let inv_2_r_outer = 1.0 / (2.0 * r_outer);

        let r_inner = (r_mid - stroke_width / 2.0).max(0.0);
        let (r_inner_sq, inv_2_r_inner) = if r_inner > 0.0 {
            (r_inner * r_inner, 1.0 / (2.0 * r_inner))
        } else {
            (-999999.0, 1.0)
        };

        let r_cap = stroke_width / 2.0;
        let r_cap_sq = r_cap * r_cap;
        let inv_2_r_cap = 1.0 / (2.0 * r_cap);
        let cap_r_in = (r_cap - 0.5).max(0.0);
        let cap_r_in_sq = cap_r_in * cap_r_in;
        let cap_r_out = r_cap + 0.5;
        let cap_r_out_sq = cap_r_out * cap_r_out;

        let x_center = span.origin.x as f32 + w / 2.0;
        let y_center = span.origin.y as f32 + h / 2.0;

        // Normalize start/end angles so that sweep is positive
        let mut start_angle = start_angle_rad.to_degrees();
        let mut end_angle = end_angle_rad.to_degrees();
        if end_angle < start_angle {
            core::mem::swap(&mut start_angle, &mut end_angle);
            core::mem::swap(&mut start_cos, &mut end_cos);
            core::mem::swap(&mut start_sin, &mut end_sin);
        }
        let sweep = end_angle - start_angle;
        let is_full_circle = sweep >= 360.0;
        let is_sweep_less_180 = sweep < 180.0;

        let mut min_y_norm = start_sin.min(end_sin);
        let mut max_y_norm = start_sin.max(end_sin);
        if !is_full_circle {
            let start_quad = (start_angle / 90.0).floor() as i32;
            let end_quad = (end_angle / 90.0).floor() as i32;
            for q in start_quad..=end_quad {
                let angle = (q * 90) % 360;
                if angle == 90 || angle == -270 {
                    max_y_norm = 1.0;
                } else if angle == 270 || angle == -90 {
                    min_y_norm = -1.0;
                }
            }
        }
        let expansion = r_cap * 1.5 + 0.5;
        let arc_min_y = min_y_norm * r_mid - expansion;
        let arc_max_y = max_y_norm * r_mid + expansion;

        let cap_start_x = r_mid * start_cos;
        let cap_start_y = r_mid * start_sin;
        let cap_end_x = r_mid * end_cos;
        let cap_end_y = r_mid * end_sin;

        let r_out_solid_sq = (r_outer - 0.5) * (r_outer - 0.5);
        let r_in_solid_sq = (r_inner + 0.5) * (r_inner + 0.5);
        let cap_expansion = r_cap * 1.5 + 0.5;

        let inv_start_sin = if start_sin.abs() > 1e-5 { 1.0 / start_sin } else { 0.0 };
        let inv_end_sin = if end_sin.abs() > 1e-5 { 1.0 / end_sin } else { 0.0 };

        Self {
            stroke_width,
            r_mid,
            r_outer,
            r_out_bound,
            r_in_bound,
            r_out_sq,
            r_in_sq,
            r_outer_sq,
            inv_2_r_outer,
            r_inner,
            r_inner_sq,
            inv_2_r_inner,
            r_cap,
            r_cap_sq,
            inv_2_r_cap,
            cap_r_in,
            cap_r_in_sq,
            cap_r_out,
            cap_r_out_sq,
            x_center,
            y_center,
            start_angle,
            end_angle,
            start_cos,
            start_sin,
            end_cos,
            end_sin,
            sweep,
            is_full_circle,
            is_sweep_less_180,
            arc_min_y,
            arc_max_y,
            cap_start_x,
            cap_start_y,
            cap_end_x,
            cap_end_y,
            r_out_solid_sq,
            r_in_solid_sq,
            cap_expansion,
            inv_start_sin,
            inv_end_sin,
        }
    }
}


#[cfg(feature = "path")]
pub(super) fn draw_arc_line<Pixel: TargetPixel>(
    span: &PhysicalRect,
    line: PhysicalLength,
    arc: &super::SceneArc,
    params: &ArcParameters,
    line_buffer: &mut [Pixel],
    extra_left_clip: i16,
    _extra_right_clip: i16,
) {
    if params.stroke_width <= 0.0 {
        return;
    }

    let is_opaque = arc.stroke_color.alpha == u8::MAX;
    let solid_color_pixel = Pixel::from_rgb(arc.stroke_color.red, arc.stroke_color.green, arc.stroke_color.blue);

    let dy = (line.get() as f32 + 0.5) - params.y_center;
    if dy.abs() >= params.r_out_bound {
        return;
    }

    if params.sweep <= 0.0 {
        return;
    }

    if !params.is_full_circle {
        if dy < params.arc_min_y || dy > params.arc_max_y {
            return;
        }
    }

    let vs_cross_y = dy * params.start_cos;
    let ve_cross_y = dy * params.end_cos;
    let vs_dot_y = dy * params.start_sin;
    let ve_dot_y = dy * params.end_sin;



    let line_start_x = span.origin.x + extra_left_clip;
    
    let dy_sq = dy * dy;

    const DISABLE_AA: bool = cfg!(feature = "disable-aa");
    let r_out_solid_sq = if DISABLE_AA { params.r_out_sq } else { params.r_out_solid_sq };
    let r_in_solid_sq = if DISABLE_AA { params.r_in_sq } else { params.r_in_solid_sq };

    let dy_minus_cap_start_y_sq = (dy - params.cap_start_y) * (dy - params.cap_start_y);
    let dy_minus_cap_end_y_sq = (dy - params.cap_end_y) * (dy - params.cap_end_y);

    let dy_diff_start = (dy - params.cap_start_y).abs();
    let dy_diff_end = (dy - params.cap_end_y).abs();
    let is_near_cap_start_y = dy_diff_start <= params.cap_expansion;
    let is_near_cap_end_y = dy_diff_end <= params.cap_expansion;

    let dx_max_sq = params.r_out_sq - dy_sq;
    if dx_max_sq < 0.0 { return; }
    let dx_max = dx_max_sq.sqrt();

    // Pixel X is covered iff |X + 0.5 - x_center| <= dx_max
    let x_start_phys = (params.x_center - 0.5 - dx_max).ceil() as i16;
    let x_end_phys = (params.x_center - 0.5 + dx_max).floor() as i16 + 1;

    let start_idx = (x_start_phys - line_start_x).max(0) as usize;
    let end_idx = (x_end_phys - line_start_x).min(line_buffer.len() as i16).max(0) as usize;

    let mut ranges = [(0usize, 0usize); 2];
    let mut num_ranges = 0;
    let dx_in_sq = params.r_in_sq - dy_sq;
    let dx_in = if dx_in_sq > 0.0 {
        dx_in_sq.sqrt()
    } else {
        0.0
    };

    if dx_in > 0.0 {
        let skip_x_start = (params.x_center - 0.5 - dx_in).floor() as i16 + 1;
        let skip_x_end = (params.x_center - 0.5 + dx_in).ceil() as i16 - 1;

        if skip_x_start <= skip_x_end {
            let idx_skip_start = (skip_x_start - line_start_x).max(0) as usize;
            let idx_skip_end = (skip_x_end + 1 - line_start_x).max(0) as usize;

            let range1_start = start_idx;
            let range1_end = end_idx.min(idx_skip_start);

            let range2_start = start_idx.max(idx_skip_end).max(range1_end);
            let range2_end = end_idx;

            if range1_start < range1_end {
                ranges[num_ranges] = (range1_start, range1_end);
                num_ranges += 1;
            }
            if range2_start < range2_end {
                ranges[num_ranges] = (range2_start, range2_end);
                num_ranges += 1;
            }
        } else {
            if start_idx < end_idx { 
                ranges[num_ranges] = (start_idx, end_idx);
                num_ranges += 1;
            }
        }
    } else {
        if start_idx < end_idx { 
            ranges[num_ranges] = (start_idx, end_idx);
            num_ranges += 1;
        }
    }

    // Pre-calculate solid ranges for the scanline
    let mut solid_ranges = [None; 2];
    let mut num_solid_ranges = 0;

    let (dx_out_solid, dx_in_solid) = if DISABLE_AA {
        (dx_max, dx_in)
    } else {
        let dx_out_solid = if r_out_solid_sq >= dy_sq {
            (r_out_solid_sq - dy_sq).sqrt()
        } else {
            -1.0
        };
        let dx_in_solid = if r_in_solid_sq > dy_sq {
            (r_in_solid_sq - dy_sq).sqrt()
        } else {
            0.0
        };
        (dx_out_solid, dx_in_solid)
    };

    if dx_out_solid >= 0.0 {
        if dx_in_solid > 0.0 {
            let left_min = (params.x_center - 0.5 - dx_out_solid).ceil() as i16;
            let left_max = (params.x_center - 0.5 - dx_in_solid).floor() as i16;
            if left_min <= left_max {
                solid_ranges[num_solid_ranges] = Some((left_min, left_max));
                num_solid_ranges += 1;
            }
            
            let right_min = (params.x_center - 0.5 + dx_in_solid).ceil() as i16;
            let right_max = (params.x_center - 0.5 + dx_out_solid).floor() as i16;
            if right_min <= right_max {
                solid_ranges[num_solid_ranges] = Some((right_min, right_max));
                num_solid_ranges += 1;
            }
        } else {
            let s_min = (params.x_center - 0.5 - dx_out_solid).ceil() as i16;
            let s_max = (params.x_center - 0.5 + dx_out_solid).floor() as i16;
            if s_min <= s_max {
                solid_ranges[num_solid_ranges] = Some((s_min, s_max));
                num_solid_ranges += 1;
            }
        }
    }

    let is_near_any_cap = is_near_cap_start_y || is_near_cap_end_y;

    for i in 0..num_ranges {
        let (r_start, r_end) = ranges[i];
        
        let dx = (line_start_x + r_start as i16) as f32 + 0.5 - params.x_center;
        let p_cross_vs = dx * params.start_sin - vs_cross_y;
        let p_cross_ve = dx * params.end_sin - ve_cross_y;
        
        let dx_r_start = dx;
        let dx_r_end = dx + (r_end - 1 - r_start) as f32;
        
        let p_cross_vs_r_end = p_cross_vs + (r_end - 1 - r_start) as f32 * params.start_sin;
        let p_cross_ve_r_end = p_cross_ve + (r_end - 1 - r_start) as f32 * params.end_sin;
        
        let is_inside_r_start = if params.is_full_circle {
            true
        } else {
            if params.is_sweep_less_180 {
                p_cross_vs <= 0.0 && p_cross_ve >= 0.0
            } else {
                !(p_cross_ve <= 0.0 && p_cross_vs >= 0.0)
            }
        };
        
        let is_inside_r_end = if params.is_full_circle {
            true
        } else {
            if params.is_sweep_less_180 {
                p_cross_vs_r_end <= 0.0 && p_cross_ve_r_end >= 0.0
            } else {
                !(p_cross_ve_r_end <= 0.0 && p_cross_vs_r_end >= 0.0)
            }
        };
        
        let sign_vs_ok = params.is_full_circle || (p_cross_vs >= 0.0) == (p_cross_vs_r_end >= 0.0);
        let sign_ve_ok = params.is_full_circle || (p_cross_ve >= 0.0) == (p_cross_ve_r_end >= 0.0);
        
        let start_cap_safe = params.is_full_circle || !is_near_cap_start_y
            || dx_r_end < params.cap_start_x - params.cap_expansion
            || dx_r_start > params.cap_start_x + params.cap_expansion;
            
        let end_cap_safe = params.is_full_circle || !is_near_cap_end_y
            || dx_r_end < params.cap_end_x - params.cap_expansion
            || dx_r_start > params.cap_end_x + params.cap_expansion;
            
        let range_completely_outside = !is_inside_r_start && !is_inside_r_end && sign_vs_ok && sign_ve_ok && start_cap_safe && end_cap_safe;

        if range_completely_outside {
            continue;
        }

        if !is_near_any_cap {
            // Find intersection with the inside sweep intervals
            let mut inside_ranges = [None; 2];
            let mut num_inside_ranges = 0;

            if params.is_full_circle {
                inside_ranges[0] = Some((dx_r_start, dx_r_end));
                num_inside_ranges = 1;
            } else {
                let x_vs = if params.start_sin.abs() > 1e-5 {
                    vs_cross_y * params.inv_start_sin
                } else {
                    if vs_cross_y >= 0.0 { f32::NEG_INFINITY } else { f32::INFINITY }
                };
                let x_ve = if params.end_sin.abs() > 1e-5 {
                    ve_cross_y * params.inv_end_sin
                } else {
                    if ve_cross_y <= 0.0 { f32::INFINITY } else { f32::NEG_INFINITY }
                };

                if params.is_sweep_less_180 {
                    let mut min_x = f32::NEG_INFINITY;
                    let mut max_x = f32::INFINITY;
                    if params.start_sin > 0.0 {
                        max_x = max_x.min(x_vs);
                    } else if params.start_sin < 0.0 {
                        min_x = min_x.max(x_vs);
                    } else if vs_cross_y < 0.0 {
                        max_x = f32::NEG_INFINITY;
                    }

                    if params.end_sin > 0.0 {
                        min_x = min_x.max(x_ve);
                    } else if params.end_sin < 0.0 {
                        max_x = max_x.min(x_ve);
                    } else if ve_cross_y > 0.0 {
                        max_x = f32::NEG_INFINITY;
                    }

                    let intersect_min = min_x.max(dx_r_start);
                    let intersect_max = max_x.min(dx_r_end);
                    if intersect_min <= intersect_max {
                        inside_ranges[0] = Some((intersect_min, intersect_max));
                        num_inside_ranges = 1;
                    }
                } else {
                    let mut gap_min = f32::NEG_INFINITY;
                    let mut gap_max = f32::INFINITY;
                    if params.end_sin > 0.0 {
                        gap_max = gap_max.min(x_ve);
                    } else if params.end_sin < 0.0 {
                        gap_min = gap_min.max(x_ve);
                    } else if ve_cross_y > 0.0 {
                        gap_max = f32::NEG_INFINITY;
                    }

                    if params.start_sin > 0.0 {
                        gap_min = gap_min.max(x_vs);
                    } else if params.start_sin < 0.0 {
                        gap_max = gap_max.min(x_vs);
                    } else if vs_cross_y < 0.0 {
                        gap_max = f32::NEG_INFINITY;
                    }

                    if gap_min < gap_max {
                        let left_min = dx_r_start;
                        let left_max = dx_r_end.min(gap_min);
                        if left_min <= left_max {
                            inside_ranges[num_inside_ranges] = Some((left_min, left_max));
                            num_inside_ranges += 1;
                        }

                        let right_min = dx_r_start.max(gap_max);
                        let right_max = dx_r_end;
                        if right_min <= right_max {
                            inside_ranges[num_inside_ranges] = Some((right_min, right_max));
                            num_inside_ranges += 1;
                        }
                    } else {
                        inside_ranges[0] = Some((dx_r_start, dx_r_end));
                        num_inside_ranges = 1;
                    }
                }
            }

            for j in 0..num_inside_ranges {
                if let Some((sub_dx_start, sub_dx_end)) = inside_ranges[j] {
                    let sub_start = (sub_dx_start - dx_r_start + r_start as f32).round() as usize;
                    let sub_end = (sub_dx_end - dx_r_start + r_start as f32).round() as usize + 1;
                    let sub_start = sub_start.clamp(r_start, r_end);
                    let sub_end = sub_end.clamp(r_start, r_end);

                    if sub_start < sub_end {
                        let (s_start_sub, s_end_sub) = if DISABLE_AA {
                            (sub_start, sub_end)
                        } else {
                            let mut s_start_sub = sub_start;
                            let mut s_end_sub = sub_start;
                            for sr in &solid_ranges[0..num_solid_ranges] {
                                if let Some((s_phys_min, s_phys_max)) = *sr {
                                    let intersect_min = s_phys_min.max(line_start_x + sub_start as i16);
                                    let intersect_max = s_phys_max.min(line_start_x + sub_end as i16 - 1);
                                    
                                    if intersect_min <= intersect_max {
                                        s_start_sub = (intersect_min - line_start_x) as usize;
                                        s_end_sub = (intersect_max + 1 - line_start_x) as usize;
                                        break;
                                    }
                                }
                            }
                            (s_start_sub, s_end_sub)
                        };

                        // 1. Draw leading non-solid/AA edge
                        let mut sub_dx = dx_r_start + (sub_start - r_start) as f32;
                        for idx in sub_start..s_start_sub {
                            let dist_sq = sub_dx * sub_dx + dy_sq;
                            let d_out = (dist_sq - params.r_outer_sq) * params.inv_2_r_outer;
                            let d_in = (params.r_inner_sq - dist_sq) * params.inv_2_r_inner;
                            let dist_to_shape = d_out.max(d_in);
                            let dist_to_shape = dist_to_shape.max(-0.5);
                            if dist_to_shape < 0.5 {
                                let alpha_i32 = (128.0 - dist_to_shape * 256.0) as i32;
                                let alpha_u8 = alpha_i32.clamp(0, 256) as u32;
                                if alpha_u8 > 0 {
                                    let c = PremultipliedRgbaColor {
                                        alpha: ((arc.stroke_color.alpha as u32 * alpha_u8) >> 8) as u8,
                                        red: ((arc.stroke_color.red as u32 * alpha_u8) >> 8) as u8,
                                        green: ((arc.stroke_color.green as u32 * alpha_u8) >> 8) as u8,
                                        blue: ((arc.stroke_color.blue as u32 * alpha_u8) >> 8) as u8,
                                    };
                                    line_buffer[idx].blend(c);
                                }
                            }
                            sub_dx += 1.0;
                        }

                        // 2. Draw solid middle using bulk fill/blend
                        if s_start_sub < s_end_sub {
                            if is_opaque {
                                line_buffer[s_start_sub..s_end_sub].fill(solid_color_pixel);
                            } else {
                                Pixel::blend_slice(&mut line_buffer[s_start_sub..s_end_sub], arc.stroke_color);
                            }
                        }

                        // 3. Draw trailing non-solid/AA edge
                        let mut sub_dx = dx_r_start + (s_end_sub - r_start) as f32;
                        for idx in s_end_sub..sub_end {
                            let dist_sq = sub_dx * sub_dx + dy_sq;
                            let d_out = (dist_sq - params.r_outer_sq) * params.inv_2_r_outer;
                            let d_in = (params.r_inner_sq - dist_sq) * params.inv_2_r_inner;
                            let dist_to_shape = d_out.max(d_in);
                            let dist_to_shape = dist_to_shape.max(-0.5);
                            if dist_to_shape < 0.5 {
                                let alpha_i32 = (128.0 - dist_to_shape * 256.0) as i32;
                                let alpha_u8 = alpha_i32.clamp(0, 256) as u32;
                                if alpha_u8 > 0 {
                                    let c = PremultipliedRgbaColor {
                                        alpha: ((arc.stroke_color.alpha as u32 * alpha_u8) >> 8) as u8,
                                        red: ((arc.stroke_color.red as u32 * alpha_u8) >> 8) as u8,
                                        green: ((arc.stroke_color.green as u32 * alpha_u8) >> 8) as u8,
                                        blue: ((arc.stroke_color.blue as u32 * alpha_u8) >> 8) as u8,
                                    };
                                    line_buffer[idx].blend(c);
                                }
                            }
                            sub_dx += 1.0;
                        }
                    }
                }
            }
        } else {
            // Fallback path: range overlaps cap. Simple loop without division or sorting.
            let mut local_dx = dx;
            let mut local_p_cross_vs = p_cross_vs;
            let mut local_p_cross_ve = p_cross_ve;

            for idx in r_start..r_end {
                let dist_sq = local_dx * local_dx + dy_sq;
                let is_inside = if params.is_full_circle {
                    true
                } else {
                    if params.is_sweep_less_180 {
                        local_p_cross_vs <= 0.0 && local_p_cross_ve >= 0.0
                    } else {
                        !(local_p_cross_ve <= 0.0 && local_p_cross_vs >= 0.0)
                    }
                };
                
                if !is_inside {
                    let mut near_cap = is_near_cap_start_y && (local_dx - params.cap_start_x).abs() <= params.cap_expansion;
                    if !near_cap {
                        near_cap = is_near_cap_end_y && (local_dx - params.cap_end_x).abs() <= params.cap_expansion;
                    }
                    
                    if !near_cap {
                        local_dx += 1.0;
                        local_p_cross_vs += params.start_sin;
                        local_p_cross_ve += params.end_sin;
                        continue;
                    }
                }

                let is_solid = is_inside && dist_sq <= r_out_solid_sq && dist_sq >= r_in_solid_sq;
                if is_solid {
                    if is_opaque {
                        line_buffer[idx] = solid_color_pixel;
                    } else {
                        line_buffer[idx].blend(arc.stroke_color);
                    }
                    local_dx += 1.0;
                    local_p_cross_vs += params.start_sin;
                    local_p_cross_ve += params.end_sin;
                    continue;
                }

                if DISABLE_AA {
                    if !is_inside {
                        // Check if we are inside the cap
                        let in_cap = match arc.stroke_line_cap {
                            i_slint_core::items::LineCap::Round => {
                                let dist_sq_start = (local_dx - params.cap_start_x) * (local_dx - params.cap_start_x) + dy_minus_cap_start_y_sq;
                                let dist_sq_end = (local_dx - params.cap_end_x) * (local_dx - params.cap_end_x) + dy_minus_cap_end_y_sq;
                                dist_sq_start <= params.r_cap_sq || dist_sq_end <= params.r_cap_sq
                            }
                            i_slint_core::items::LineCap::Square => {
                                let p_dot_vs = local_dx * params.start_cos + vs_dot_y;
                                let p_dot_ve = local_dx * params.end_cos + ve_dot_y;

                                let dist_to_ray_unsigned = if p_dot_vs > p_dot_ve {
                                    local_p_cross_vs.abs()
                                } else {
                                    local_p_cross_ve.abs()
                                };
                                let dist_to_ray = dist_to_ray_unsigned - params.stroke_width / 2.0;

                                let p_dot = if p_dot_vs > p_dot_ve { p_dot_vs } else { p_dot_ve };
                                let dist_along_ray = p_dot;

                                let d = dist_to_ray.max(dist_along_ray);
                                d <= 0.0
                            }
                            _ => false,
                        };
                        if in_cap {
                            if is_opaque {
                                line_buffer[idx] = solid_color_pixel;
                            } else {
                                line_buffer[idx].blend(arc.stroke_color);
                            }
                        }
                    }
                } else {
                    let dist_to_shape = if is_inside {
                        let d_out = (dist_sq - params.r_outer_sq) * params.inv_2_r_outer;
                        let d_in = (params.r_inner_sq - dist_sq) * params.inv_2_r_inner;
                        d_out.max(d_in)
                    } else {
                        match arc.stroke_line_cap {
                            i_slint_core::items::LineCap::Round => {
                                let dist_sq_start = (local_dx - params.cap_start_x) * (local_dx - params.cap_start_x) + dy_minus_cap_start_y_sq;
                                let dist_sq_end = (local_dx - params.cap_end_x) * (local_dx - params.cap_end_x) + dy_minus_cap_end_y_sq;
                                
                                let dist_to_start_cap = if dist_sq_start > params.cap_r_out_sq {
                                    1.0
                                } else if dist_sq_start <= params.cap_r_in_sq {
                                    -1.0
                                } else {
                                    (dist_sq_start - params.r_cap_sq) * params.inv_2_r_cap
                                };
                                let dist_to_end_cap = if dist_sq_end > params.cap_r_out_sq {
                                    1.0
                                } else if dist_sq_end <= params.cap_r_in_sq {
                                    -1.0
                                } else {
                                    (dist_sq_end - params.r_cap_sq) * params.inv_2_r_cap
                                };
                                dist_to_start_cap.min(dist_to_end_cap)
                            }
                            i_slint_core::items::LineCap::Square => {
                                let p_dot_vs = local_dx * params.start_cos + vs_dot_y;
                                let p_dot_ve = local_dx * params.end_cos + ve_dot_y;

                                let dist_to_ray_unsigned = if p_dot_vs > p_dot_ve {
                                    local_p_cross_vs.abs()
                                } else {
                                    local_p_cross_ve.abs()
                                };
                                let dist_to_ray = dist_to_ray_unsigned - params.stroke_width / 2.0;

                                let p_dot = if p_dot_vs > p_dot_ve { p_dot_vs } else { p_dot_ve };
                                let dist_along_ray = p_dot;

                                dist_to_ray.max(dist_along_ray)
                            }
                            _ => 1.0,
                        }
                    };

                    let dist_to_shape = dist_to_shape.max(-0.5);
                    if dist_to_shape < 0.5 {
                        let alpha_i32 = (128.0 - dist_to_shape * 256.0) as i32;
                        let alpha_u8 = alpha_i32.clamp(0, 256) as u32;
                        if alpha_u8 > 0 {
                            let c = PremultipliedRgbaColor {
                                alpha: ((arc.stroke_color.alpha as u32 * alpha_u8) >> 8) as u8,
                                red: ((arc.stroke_color.red as u32 * alpha_u8) >> 8) as u8,
                                green: ((arc.stroke_color.green as u32 * alpha_u8) >> 8) as u8,
                                blue: ((arc.stroke_color.blue as u32 * alpha_u8) >> 8) as u8,
                            };
                            line_buffer[idx].blend(c);
                        }
                    }
                }

                local_dx += 1.0;
                local_p_cross_vs += params.start_sin;
                local_p_cross_ve += params.end_sin;
            }
        }
    }
}


// early return test

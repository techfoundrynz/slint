// Copyright © SixtyFPS GmbH <info@slint.dev>
// SPDX-License-Identifier: GPL-3.0-only OR LicenseRef-Slint-Royalty-free-2.0 OR LicenseRef-Slint-Software-3.0

use super::{PhysicalLength, PhysicalSize, fonts};
use alloc::rc::Rc;
use alloc::vec::Vec;
use fonts::GlyphRenderer;
use i_slint_core::SharedString;
use i_slint_core::graphics::FontRequest;
use i_slint_core::items::{TextHorizontalAlignment, TextOverflow, TextVerticalAlignment, TextWrap};
use i_slint_core::lengths::PhysicalPx;
use i_slint_core::textlayout::{AbstractFont, TextParagraphLayout};

const MAX_BYTES: usize = 8 * 1024;
const MAX_MASK_BYTES: usize = 64 * 1024;
const MAX_MASK_ENTRY_BYTES: usize = 16 * 1024;
const MAX_ENTRIES: usize = 24;
const MAX_TEXT_BYTES: usize = 128;

#[derive(PartialEq)]
pub(crate) struct TextKey {
    pub text: SharedString,
    pub request: FontRequest,
    pub bitmap: usize,
    pub glyphs: usize,
    pub pixel_size: i16,
    pub scale: u32,
    pub size: PhysicalSize,
    pub horizontal_alignment: TextHorizontalAlignment,
    pub vertical_alignment: TextVerticalAlignment,
    pub wrap: TextWrap,
    pub overflow: TextOverflow,
    pub max_lines: Option<usize>,
}

pub(crate) struct PreparedGlyph {
    pub glyph: fonts::RenderableGlyph,
    pub rect: euclid::Rect<f32, PhysicalPx>,
}

pub(crate) struct PreparedText {
    key: TextKey,
    pub glyphs: Vec<PreparedGlyph>,
    bytes: usize,
    #[cfg(feature = "testing")]
    mask_count: usize,
    mask_bytes: usize,
}

#[derive(Default)]
pub(crate) struct TextCache {
    entries: Vec<Rc<PreparedText>>,
    #[cfg(feature = "testing")]
    pub(crate) disabled: bool,
    #[cfg(feature = "testing")]
    pub(crate) hits: usize,
    bytes: usize,
    mask_bytes: usize,
}

impl TextCache {
    #[cfg(feature = "testing")]
    pub fn stats(&self) -> (usize, usize) {
        (self.entries.len(), self.bytes)
    }

    #[cfg(feature = "testing")]
    pub fn mask_stats(&self) -> (usize, usize) {
        (self.entries.iter().filter(|entry| entry.mask_count > 0).count(), self.mask_bytes)
    }

    pub fn clear(&mut self) {
        #[cfg(feature = "testing")]
        let disabled = self.disabled;
        *self = Self::default();
        #[cfg(feature = "testing")]
        {
            self.disabled = disabled;
        }
    }

    pub fn get_or_insert<Font>(
        &mut self,
        key: TextKey,
        paragraph: &TextParagraphLayout<'_, Font>,
        font: &Font,
        context: &i_slint_core::SlintContext,
    ) -> Option<Rc<PreparedText>>
    where
        Font: AbstractFont
            + i_slint_core::textlayout::TextShaper<Length = PhysicalLength>
            + GlyphRenderer,
    {
        if !cfg!(feature = "embedded-caches") {
            return None;
        }
        #[cfg(feature = "testing")]
        if self.disabled {
            return None;
        }
        if key.text.len() > MAX_TEXT_BYTES {
            return None;
        }
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            #[cfg(feature = "testing")]
            {
                self.hits += 1;
            }
            let entry = self.entries.remove(index);
            self.entries.push(entry.clone());
            return Some(entry);
        }
        // One extra glyph accommodates the ellipsis without growing the allocation.
        let capacity = key.text.chars().count() + 1;
        let bytes = core::mem::size_of::<PreparedText>()
            + capacity * core::mem::size_of::<PreparedGlyph>()
            + key.text.len()
            + key.request.family.as_ref().map_or(0, |s| s.len());
        if bytes > MAX_BYTES {
            return None;
        }
        while self.entries.len() >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES {
            let old = self.entries.remove(0);
            self.bytes -= old.bytes;
            self.mask_bytes -= old.mask_bytes;
        }
        self.entries.try_reserve(1).ok()?;
        let mut prepared = Vec::new();
        prepared.try_reserve_exact(capacity).ok()?;
        let result = paragraph.layout_lines::<()>(
            |glyphs, line_x, line_y, _, _| {
                let baseline = line_y + paragraph.layout.half_leading() + font.ascent();
                for positioned in glyphs {
                    let Some(glyph) = font.render_glyph(positioned.glyph_id, context) else {
                        continue;
                    };
                    if prepared.len() == capacity {
                        return core::ops::ControlFlow::Break(());
                    }
                    let gl_x = PhysicalLength::new((-glyph.x).truncate() as i16);
                    let gl_y = PhysicalLength::new(glyph.y.truncate() as i16);
                    let rect = super::PhysicalRect::new(
                        super::PhysicalPoint::from_lengths(
                            line_x + positioned.x - gl_x,
                            baseline - gl_y - glyph.height,
                        ),
                        glyph.size(),
                    )
                    .cast();
                    prepared.push(PreparedGlyph { glyph, rect });
                }
                core::ops::ControlFlow::Continue(())
            },
            None,
        );
        if result.is_err() {
            return None;
        }
        let can_mask =
            font.scale_delta() == super::Fixed::from_integer(1) && mask_bounds(&prepared).is_some();
        if can_mask {
            while self.mask_bytes + MAX_MASK_ENTRY_BYTES > MAX_MASK_BYTES {
                let old = self.entries.remove(0);
                self.bytes -= old.bytes;
                self.mask_bytes -= old.mask_bytes;
            }
        }
        let masks = if can_mask { build_mask_groups(&prepared) } else { None };
        let (prepared, _mask_count, mask_bytes) = masks.unwrap_or((prepared, 0, 0));
        let entry = Rc::new(PreparedText {
            key,
            glyphs: prepared,
            bytes,
            #[cfg(feature = "testing")]
            mask_count: _mask_count,
            mask_bytes,
        });
        self.mask_bytes += mask_bytes;
        self.bytes += bytes;
        self.entries.push(entry.clone());
        Some(entry)
    }
}

fn try_zeroed_bytes(len: usize) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).ok()?;
    bytes.resize(len, 0);
    Some(bytes)
}

fn mask_bounds(glyphs: &[PreparedGlyph]) -> Option<super::PhysicalRect> {
    if glyphs.len() < 3 {
        return None;
    }
    let mut bounds: Option<super::PhysicalRect> = None;
    for glyph in glyphs {
        if glyph.glyph.sdf || glyph.rect != glyph.rect.round() {
            return None;
        }
        let rect = glyph.rect.cast::<i16>();
        bounds = Some(bounds.map_or(rect, |b| b.union(&rect)));
    }
    let bounds = bounds?;
    if bounds.is_empty() {
        return None;
    }
    let bytes = (bounds.width() as usize).checked_mul(bounds.height() as usize)?;
    if !(1024..=MAX_MASK_ENTRY_BYTES).contains(&bytes) {
        return None;
    }
    Some(bounds)
}

fn build_mask(glyphs: &[PreparedGlyph], bounds: super::PhysicalRect) -> Option<PreparedGlyph> {
    let width = bounds.width() as usize;
    let height = bounds.height() as usize;
    let mut pixels = try_zeroed_bytes(width.checked_mul(height)?)?;
    for prepared in glyphs {
        let glyph = &prepared.glyph;
        let fonts::GlyphAlphaMap::Static(data) = &glyph.alpha_map else {
            return None;
        };
        let x = prepared.rect.origin.x as i16 - bounds.min_x();
        let y = prepared.rect.origin.y as i16 - bounds.min_y();
        for row in 0..glyph.height.get() as usize {
            for col in 0..glyph.width.get() as usize {
                let coverage = *data.get(row * glyph.pixel_stride as usize + col)?;
                if coverage == 0 {
                    continue;
                }
                let target = (y as usize + row) * width + x as usize + col;
                if pixels[target] != 0 {
                    return None;
                }
                pixels[target] = coverage;
            }
        }
    }
    Some(PreparedGlyph {
        rect: bounds.cast(),
        glyph: fonts::RenderableGlyph {
            x: super::Fixed::default(),
            y: super::Fixed::default(),
            width: PhysicalLength::new(width as i16),
            height: PhysicalLength::new(height as i16),
            pixel_stride: width as u16,
            sdf: false,
            alpha_map: fonts::GlyphAlphaMap::Cached(Rc::new(pixels)),
        },
    })
}

fn build_mask_groups(glyphs: &[PreparedGlyph]) -> Option<(Vec<PreparedGlyph>, usize, usize)> {
    let mut result = Vec::new();
    result.try_reserve_exact(glyphs.len()).ok()?;
    let mut begin = 0;
    let mut bytes = 0;
    let mut mask_count = 0;
    while begin < glyphs.len() {
        let remaining = &glyphs[begin..];
        let end = match mask_bounds(remaining) {
            Some(bounds) => disjoint_prefix(remaining, bounds)?,
            None => remaining.len(),
        };
        let group = &remaining[..end];
        let mask = mask_bounds(group).and_then(|bounds| build_mask(group, bounds));
        if let Some(mask) = mask {
            bytes += mask.glyph.width.get() as usize * mask.glyph.height.get() as usize;
            if bytes > MAX_MASK_ENTRY_BYTES {
                return None;
            }
            result.push(mask);
            mask_count += 1;
        } else {
            result.extend(
                group.iter().map(|g| PreparedGlyph { glyph: g.glyph.clone(), rect: g.rect }),
            );
        }
        begin += end;
    }
    (mask_count > 0).then_some((result, mask_count, bytes))
}

fn disjoint_prefix(glyphs: &[PreparedGlyph], bounds: super::PhysicalRect) -> Option<usize> {
    let width = bounds.width() as usize;
    let mut pixels = try_zeroed_bytes(width.checked_mul(bounds.height() as usize)?)?;
    for (index, prepared) in glyphs.iter().enumerate() {
        let glyph = &prepared.glyph;
        let fonts::GlyphAlphaMap::Static(data) = &glyph.alpha_map else {
            return None;
        };
        let x = prepared.rect.origin.x as i16 - bounds.min_x();
        let y = prepared.rect.origin.y as i16 - bounds.min_y();
        for row in 0..glyph.height.get() as usize {
            for col in 0..glyph.width.get() as usize {
                let coverage = *data.get(row * glyph.pixel_stride as usize + col)?;
                let target = (y as usize + row) * width + x as usize + col;
                if coverage > 0 && pixels[target] > 0 {
                    return Some(index);
                }
            }
        }
        for row in 0..glyph.height.get() as usize {
            for col in 0..glyph.width.get() as usize {
                let target = (y as usize + row) * width + x as usize + col;
                pixels[target] |= *data.get(row * glyph.pixel_stride as usize + col)?;
            }
        }
    }
    Some(glyphs.len())
}

#[cfg(test)]
mod mask_tests {
    use super::*;

    fn glyph(x: f32) -> PreparedGlyph {
        PreparedGlyph {
            rect: euclid::rect(x, 0., 20., 20.),
            glyph: fonts::RenderableGlyph {
                x: super::super::Fixed::default(),
                y: super::super::Fixed::default(),
                width: PhysicalLength::new(20),
                height: PhysicalLength::new(20),
                pixel_stride: 20,
                sdf: false,
                alpha_map: fonts::GlyphAlphaMap::Static(&[255; 400]),
            },
        }
    }

    #[test]
    fn masks_reject_sdf_and_dynamic_glyphs() {
        let mut sdf = glyph(0.);
        sdf.glyph.sdf = true;
        assert!(mask_bounds(&[sdf, glyph(20.)]).is_none());
        let mut dynamic = glyph(0.);
        dynamic.glyph.alpha_map = fonts::GlyphAlphaMap::Shared(Rc::from(&[255u8; 400][..]));
        assert!(mask_bounds(&[dynamic, glyph(20.)]).is_none());
    }

    #[test]
    fn allocation_failure_returns_to_glyph_path() {
        assert!(try_zeroed_bytes(usize::MAX).is_none());
    }

    #[test]
    fn masks_preserve_disjoint_coverage_and_reject_overlap() {
        let disjoint = [glyph(0.), glyph(20.), glyph(40.)];
        let mask = build_mask(&disjoint, mask_bounds(&disjoint).unwrap()).unwrap();
        let fonts::GlyphAlphaMap::Cached(pixels) = mask.glyph.alpha_map else { panic!() };
        assert_eq!(pixels.len(), 1200);
        assert!(pixels.iter().all(|&p| p == 255));
        let overlap = [glyph(0.), glyph(10.), glyph(40.)];
        assert!(build_mask(&overlap, mask_bounds(&overlap).unwrap()).is_none());
    }

    #[test]
    fn masks_reject_fractional_geometry_and_excessive_bounds() {
        assert!(mask_bounds(&[glyph(0.5), glyph(20.), glyph(40.)]).is_none());
        assert!(mask_bounds(&[glyph(0.), glyph(20.), glyph(2000.)]).is_none());
        assert!(mask_bounds(&[glyph(0.)]).is_none());
    }
    #[test]
    fn overlapping_labels_keep_ordered_mask_groups() {
        let glyphs = [glyph(0.), glyph(20.), glyph(40.), glyph(50.), glyph(70.), glyph(90.)];
        let (groups, count, bytes) = build_mask_groups(&glyphs).unwrap();
        assert_eq!(count, 2);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].rect.origin.x, 0.);
        assert_eq!(groups[1].rect.origin.x, 50.);
        assert_eq!(bytes, 2400);
    }
}

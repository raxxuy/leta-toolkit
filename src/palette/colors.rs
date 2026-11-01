use std::path::Path;

use ::image::{DynamicImage, GenericImageView};
use anyhow::Context;

const SAMPLE_SIDE: u32 = 64;
const BIN_COUNT: usize = 4096;
const DEFAULT_HUE: f64 = 220.0;
const DEFAULT_SATURATION: f64 = 0.55;
const MIN_SATURATION: f64 = 0.15;
const MIN_BRIGHTNESS: u8 = 24;
const MIN_HUE_SEPARATION: f64 = 35.0;
const ACCENT_LIGHTNESS: f64 = 0.70;
const HUE_RADIUS: i32 = 18;
const MIN_RELATIVE_SUPPORT: f64 = 0.08;

#[derive(Clone, Copy, Debug)]
struct Rgb(u8, u8, u8);

#[derive(Clone, Copy)]
struct Tone {
    hue: f64,
    saturation: f64,
    lightness: f64,
}

impl Rgb {
    fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    fn max_channel(self) -> u8 {
        self.0.max(self.1).max(self.2)
    }

    fn luminance(self) -> f64 {
        fn linear(channel: u8) -> f64 {
            let c = f64::from(channel) / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }
        0.2126 * linear(self.0) + 0.7152 * linear(self.1) + 0.0722 * linear(self.2)
    }

    fn tone(self) -> Tone {
        let r = f64::from(self.0) / 255.0;
        let g = f64::from(self.1) / 255.0;
        let b = f64::from(self.2) / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let delta = max - min;
        if delta == 0.0 {
            return Tone {
                hue: 0.0,
                saturation: 0.0,
                lightness: max,
            };
        }
        let lightness = (max + min) / 2.0;
        let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
        let sector = if max == r {
            ((g - b) / delta).rem_euclid(6.0)
        } else if max == g {
            (b - r) / delta + 2.0
        } else {
            (r - g) / delta + 4.0
        };
        Tone {
            hue: sector * 60.0,
            saturation,
            lightness,
        }
    }
}

fn hsl(hue: f64, saturation: f64, lightness: f64) -> Rgb {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let sector = hue.rem_euclid(360.0) / 60.0;
    let x = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match sector as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let offset = lightness - chroma / 2.0;
    let channel = |c: f64| ((c + offset) * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb(channel(r), channel(g), channel(b))
}

fn contrast(a: Rgb, b: Rgb) -> f64 {
    let a = a.luminance();
    let b = b.luminance();
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn readable_on(color: Rgb) -> Rgb {
    let black = Rgb(0, 0, 0);
    let white = Rgb(255, 255, 255);
    if contrast(color, black) >= contrast(color, white) {
        black
    } else {
        white
    }
}

fn hue_distance(a: f64, b: f64) -> f64 {
    let difference = (a - b).abs();
    difference.min(360.0 - difference)
}

/// Accumulates alpha-weighted RGB sums, so partially transparent pixels
/// contribute proportionally without assuming a compositing background.
#[derive(Clone, Copy, Default)]
struct Bin {
    weight: u64,
    red: u64,
    green: u64,
    blue: u64,
}

impl Bin {
    // Quantizes each channel to four bits so similar colors share a bin.
    fn index([red, green, blue, _]: [u8; 4]) -> usize {
        (usize::from(red >> 4) << 8) | (usize::from(green >> 4) << 4) | usize::from(blue >> 4)
    }

    fn add(&mut self, [red, green, blue, alpha]: [u8; 4]) {
        let weight = u64::from(alpha);
        self.weight += weight;
        self.red += u64::from(red) * weight;
        self.green += u64::from(green) * weight;
        self.blue += u64::from(blue) * weight;
    }

    // Weighted mean color; only valid when `weight > 0`.
    fn average(&self) -> Rgb {
        Rgb(
            (self.red / self.weight) as u8,
            (self.green / self.weight) as u8,
            (self.blue / self.weight) as u8,
        )
    }
}

#[derive(Clone, Copy)]
struct Candidate {
    tone: Tone,
    score: f64,
}

// Center of the `index`-th of `samples` equal cells spanning `size` pixels.
fn sample_position(index: u32, samples: u32, size: u32) -> u32 {
    // u64 arithmetic avoids overflow on large images.
    ((u64::from(index) * 2 + 1) * u64::from(size) / (2 * u64::from(samples))) as u32
}

fn bin_pixels(image: &DynamicImage) -> Vec<Bin> {
    let (width, height) = image.dimensions();
    let columns = width.min(SAMPLE_SIDE);
    let rows = height.min(SAMPLE_SIDE);
    let mut bins = vec![Bin::default(); BIN_COUNT];

    for y in 0..rows {
        for x in 0..columns {
            let pixel = image
                .get_pixel(
                    sample_position(x, columns, width),
                    sample_position(y, rows, height),
                )
                .0;
            bins[Bin::index(pixel)].add(pixel);
        }
    }
    bins
}

fn image_candidates(image: &DynamicImage) -> Vec<Candidate> {
    // Accumulate before scoring: splitting a color across RGB bins must not 
    // increase or decrease its influence. RGB chroma discounts pale neutrals.
    let mut hues = [0.0; 360];
    let mut populations = [0.0; 360];
    let mut saturations = [0.0; 360];
    let mut lightnesses = [0.0; 360];
    for bin in bin_pixels(image).iter().filter(|bin| bin.weight > 0) {
        let color = bin.average();
        let tone = color.tone();
        if tone.saturation < MIN_SATURATION || color.max_channel() < MIN_BRIGHTNESS {
            continue;
        }
        let chroma = f64::from(color.max_channel() - color.0.min(color.1).min(color.2)) / 255.0;
        let weight = bin.weight as f64 * chroma;
        let index = (tone.hue.round() as usize) % 360;
        hues[index] += weight;
        // Rank colorful families, but use their population to retain typical lightness.
        let population = bin.weight as f64;
        populations[index] += population;
        saturations[index] += population * tone.saturation;
        lightnesses[index] += population * tone.lightness;
    }

    (0..360)
        .filter_map(|center| {
            let mut score = 0.0;
            let mut hue_offset = 0.0;
            let mut population = 0.0;
            let mut saturation = 0.0;
            let mut lightness = 0.0;
            for offset in -HUE_RADIUS..=HUE_RADIUS {
                let index = (center + offset).rem_euclid(360) as usize;
                let weight = hues[index];
                score += weight;
                hue_offset += weight * f64::from(offset);
                population += populations[index];
                saturation += saturations[index];
                lightness += lightnesses[index];
            }
            (score > 0.0).then(|| Candidate {
                tone: Tone {
                    hue: (f64::from(center) + hue_offset / score).rem_euclid(360.0),
                    saturation: saturation / population,
                    lightness: lightness / population,
                },
                score,
            })
        })
        .collect()
}

fn choose_tone(candidates: &[Candidate], excluded_hues: &[f64]) -> Option<Tone> {
    let minimum = candidates.iter().map(|c| c.score).fold(0.0, f64::max) * MIN_RELATIVE_SUPPORT;
    candidates
        .iter()
        .filter(|candidate| candidate.score >= minimum)
        .filter(|candidate| {
            excluded_hues
                .iter()
                .all(|&hue| hue_distance(candidate.tone.hue, hue) >= MIN_HUE_SEPARATION)
        })
        .max_by(|a, b| a.score.total_cmp(&b.score))
        .map(|candidate| candidate.tone)
}

// Prefer distinct supported hues; reuse a related tone when none has enough support.
fn select_tones(candidates: &[Candidate]) -> [Tone; 3] {
    let primary = choose_tone(candidates, &[]).unwrap_or(Tone {
        hue: DEFAULT_HUE,
        saturation: DEFAULT_SATURATION,
        lightness: ACCENT_LIGHTNESS,
    });
    let secondary = choose_tone(candidates, &[primary.hue]).unwrap_or(primary);
    let accent = choose_tone(candidates, &[primary.hue, secondary.hue]).unwrap_or(primary);
    [primary, secondary, accent]
}

fn accent_color(tone: Tone, background: Rgb) -> Rgb {
    let saturation = tone.saturation.clamp(0.25, 0.8);
    let mut lightness = tone.lightness.clamp(0.55, 0.78);
    let mut color = hsl(tone.hue, saturation, lightness);
    // Accents remain visible against the dark background as well as their text.
    while contrast(color, background) < 3.0 && lightness < 1.0 {
        lightness = (lightness + 0.01).min(1.0);
        color = hsl(tone.hue, saturation, lightness);
    }
    color
}

fn palette_from_image(image: &DynamicImage) -> Vec<(String, String)> {
    let [primary_tone, secondary_tone, accent_tone] = select_tones(&image_candidates(image));

    let background = hsl(primary_tone.hue, 0.18, 0.07);
    let surface = hsl(primary_tone.hue, 0.16, 0.13);
    let primary = accent_color(primary_tone, background);
    let secondary = accent_color(secondary_tone, background);
    let accent = accent_color(accent_tone, background);
    let error = hsl(0.0, 0.72, ACCENT_LIGHTNESS);

    [
        ("background", "foreground", background),
        ("surface", "on_surface", surface),
        ("primary", "on_primary", primary),
        ("secondary", "on_secondary", secondary),
        ("accent", "on_accent", accent),
        ("error", "on_error", error),
    ]
    .into_iter()
    .flat_map(|(name, on_name, color)| [(name, color), (on_name, readable_on(color))])
    .map(|(name, color)| (name.to_owned(), color.hex()))
    .collect()
}

/// Extract a deterministic dark palette from an image file.
///
/// Transparent pixels are weighted by opacity; an entirely transparent or
/// achromatic image uses a blue fallback hue. Text roles are chosen to meet
/// at least WCAG AA contrast against their corresponding colors.
pub fn extract(path: &Path) -> anyhow::Result<Vec<(String, String)>> {
    let image =
        ::image::open(path).with_context(|| format!("failed to open image {}", path.display()))?;
    Ok(palette_from_image(&image))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::image::{DynamicImage, Rgba, RgbaImage};

    fn parse(hex: &str) -> Rgb {
        assert_eq!(hex.len(), 7);
        assert!(hex.starts_with('#'));
        Rgb(
            u8::from_str_radix(&hex[1..3], 16).unwrap(),
            u8::from_str_radix(&hex[3..5], 16).unwrap(),
            u8::from_str_radix(&hex[5..7], 16).unwrap(),
        )
    }

    #[test]
    fn palettes_are_deterministic_dark_and_readable() {
        for pixel in [
            Rgba([255, 0, 0, 255]),
            Rgba([32, 32, 32, 255]),
            Rgba([0, 255, 0, 0]),
            Rgba([10, 20, 240, 128]),
        ] {
            let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(7, 5, pixel));
            let palette = palette_from_image(&image);
            assert_eq!(palette, palette_from_image(&image));
            let names: Vec<_> = palette.iter().map(|(name, _)| name.as_str()).collect();
            assert_eq!(
                names,
                [
                    "background",
                    "foreground",
                    "surface",
                    "on_surface",
                    "primary",
                    "on_primary",
                    "secondary",
                    "on_secondary",
                    "accent",
                    "on_accent",
                    "error",
                    "on_error"
                ]
            );
            assert!(parse(&palette[0].1).luminance() < 0.05);
            assert!(parse(&palette[2].1).luminance() < 0.05);
            for pair in palette.chunks_exact(2) {
                assert!(
                    contrast(parse(&pair[0].1), parse(&pair[1].1)) >= 4.5,
                    "{pair:?}"
                );
            }
        }
    }

    #[test]
    fn transparency_and_empty_images_use_fallback() {
        let transparent_red =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 0])));
        let transparent_blue =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 255, 0])));
        let empty = DynamicImage::ImageRgba8(RgbaImage::new(0, 0));
        assert_eq!(
            palette_from_image(&transparent_red),
            palette_from_image(&transparent_blue)
        );
        assert_eq!(
            palette_from_image(&transparent_red),
            palette_from_image(&empty)
        );
    }

    #[test]
    fn image_hue_changes_palette() {
        let red = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([255, 0, 0, 255])));
        let blue = DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 255, 255])));
        assert_ne!(
            palette_from_image(&red)[4].1,
            palette_from_image(&blue)[4].1
        );
    }

    fn tones(image: RgbaImage) -> [Tone; 3] {
        select_tones(&image_candidates(&DynamicImage::ImageRgba8(image)))
    }

    #[test]
    fn fragmented_and_flat_colors_have_comparable_influence() {
        let make = |fragmented| {
            RgbaImage::from_fn(64, 64, |x, y| {
                if x < 32 {
                    if fragmented {
                        Rgba([
                            30 + (y % 4 * 8) as u8,
                            80 + (x % 4 * 8) as u8,
                            180 + (y % 4 * 8) as u8,
                            255,
                        ])
                    } else {
                        Rgba([42, 92, 192, 255])
                    }
                } else {
                    Rgba([220, 180, 130, 255])
                }
            })
        };
        let flat = tones(make(false));
        let fragmented = tones(make(true));
        assert!(hue_distance(flat[0].hue, fragmented[0].hue) < 10.0);
        assert!(hue_distance(fragmented[0].hue, 220.0) < 20.0);
        assert!(hue_distance(fragmented[1].hue, 33.0) < 15.0);
    }

    #[test]
    fn textured_blue_competes_with_flat_cream() {
        let image = RgbaImage::from_fn(64, 64, |x, y| {
            if x < 30 {
                Rgba([
                    35 + (y % 5 * 5) as u8,
                    65 + (x % 5 * 7) as u8,
                    150 + (y % 7 * 7) as u8,
                    255,
                ])
            } else {
                Rgba([235, 220, 195, 255])
            }
        });
        let selected = tones(image);
        assert!(
            hue_distance(selected[0].hue, 220.0) < 25.0,
            "{}",
            selected[0].hue
        );
        assert!(hue_distance(selected[1].hue, 38.0) < 15.0);
    }

    #[test]
    fn small_or_transparent_noise_does_not_become_a_distinct_accent() {
        for alpha in [255, 10] {
            let image = RgbaImage::from_fn(64, 64, |x, y| {
                if x < 2 && y < 2 {
                    Rgba([255, 0, 0, alpha])
                } else {
                    Rgba([45, 90, 180, 255])
                }
            });
            let selected = tones(image);
            for tone in selected {
                assert!(hue_distance(tone.hue, 220.0) < 10.0);
            }
        }
        let image = RgbaImage::from_fn(64, 64, |x, _| {
            if x < 32 {
                Rgba([255, 0, 0, 10])
            } else {
                Rgba([45, 90, 180, 255])
            }
        });
        assert!(hue_distance(tones(image)[0].hue, 220.0) < 10.0);
    }

    #[test]
    fn grayscale_uses_related_fallback_and_accents_remain_readable() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_fn(64, 64, |x, _| {
            let gray = (x * 4) as u8;
            Rgba([gray, gray, gray, 255])
        }));
        let selected = select_tones(&image_candidates(&image));
        assert!(selected.iter().all(|tone| tone.hue == DEFAULT_HUE));
        let palette = palette_from_image(&image);
        let background = parse(&palette[0].1);
        for index in [4, 6, 8] {
            assert!(contrast(background, parse(&palette[index].1)) >= 3.0);
            assert!(contrast(parse(&palette[index].1), parse(&palette[index + 1].1)) >= 4.5);
        }
    }

    #[test]
    fn typical_lightness_is_not_overwhelmed_by_a_saturated_detail() {
        let image = RgbaImage::from_fn(64, 64, |x, _| {
            if x < 48 {
                Rgba([255, 190, 200, 255])
            } else {
                Rgba([210, 0, 35, 255])
            }
        });
        let selected = tones(image);
        assert!(selected[0].lightness > 0.75);
    }

    #[test]
    fn source_lightness_and_saturation_affect_accents() {
        let background = Rgb(18, 18, 18);
        let dull = Tone {
            hue: 45.0,
            saturation: 0.3,
            lightness: 0.58,
        };
        let bright = Tone {
            hue: 45.0,
            saturation: 0.7,
            lightness: 0.75,
        };
        let a = accent_color(dull, background).tone();
        let b = accent_color(bright, background).tone();
        assert!(a.lightness < b.lightness);
        assert!(a.saturation < b.saturation);
    }

    #[test]
    fn missing_file_returns_error() {
        assert!(extract(Path::new("/this/path/does/not/exist/leta-palette.png")).is_err());
    }
}

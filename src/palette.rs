//! Generate a dark editor palette from representative colors in a wallpaper.
use crate::theme::{Appearance, Palette, Rgb};
use image::{DynamicImage, GenericImageView};

const HUES: usize = 24;
const NEUTRAL: usize = HUES;

#[derive(Clone, Copy, Default)]
struct ColorGroup {
    weight: u32,
    channels: [u64; 3],
}

impl ColorGroup {
    fn add(&mut self, rgb: Rgb, alpha: u8) {
        self.weight += u32::from(alpha);
        for (sum, channel) in self.channels.iter_mut().zip(rgb) {
            *sum += u64::from(channel) * u64::from(alpha);
        }
    }

    fn mean(&self) -> Rgb {
        let weight = u64::from(self.weight);
        self.channels.map(|sum| ((sum + weight / 2) / weight) as u8)
    }

    fn score(&self) -> f32 {
        let [r, g, b] = self.mean();
        let chroma = r.max(g).max(b) - r.min(g).min(b);
        self.weight as f32 * (0.35 + f32::from(chroma) / 255.0)
    }
}

fn hue_group([r, g, b]: Rgb) -> usize {
    let high = r.max(g).max(b);
    let chroma = high - r.min(g).min(b);
    if chroma < 16 {
        return NEUTRAL;
    }
    let [r, g, b] = [r, g, b].map(f32::from);
    let delta = f32::from(chroma);
    let sector = if r == f32::from(high) {
        (g - b) / delta
    } else if g == f32::from(high) {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    ((sector * HUES as f32 / 6.0).round() as i32).rem_euclid(HUES as i32) as usize
}

pub(crate) fn mix(from: Rgb, to: Rgb, amount: f32) -> Rgb {
    std::array::from_fn(|i| {
        (f32::from(from[i]) * (1.0 - amount) + f32::from(to[i]) * amount).round() as u8
    })
}

fn luminance(rgb: Rgb) -> f32 {
    let [r, g, b] = rgb.map(|v| {
        let s = f32::from(v) / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    });
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

pub fn readable(rgb: Rgb, surface: Rgb) -> Rgb {
    let bg = luminance(surface) + 0.05;
    let end = if bg * bg > 0.05 * 1.05 {
        [0; 3]
    } else {
        [255; 3]
    };
    (0..=32)
        .map(|step| mix(rgb, end, step as f32 / 32.0))
        .find(|candidate| {
            let fg = luminance(*candidate) + 0.05;
            fg.max(bg) / fg.min(bg) >= 4.5
        })
        .unwrap_or(end)
}

impl Palette {
    pub fn variant(&self, mode: Appearance) -> Self {
        let (base, raised, ink, quiet) = match mode {
            Appearance::Light => (253, 236, 28, 83),
            Appearance::Dark => (16, 36, 246, 167),
        };
        let surface = mix([raised; 3], self.accent, 0.13);
        Self {
            background: mix([base; 3], self.accent, 0.06),
            surface,
            text: readable(mix([ink; 3], self.accent, 0.12), surface),
            muted: readable(mix([quiet; 3], self.accent, 0.20), surface),
            accent: readable(self.accent, surface),
            secondary: readable(self.secondary, surface),
        }
    }
}

pub fn extract(image: &DynamicImage) -> Option<Palette> {
    let (width, height) = image.dimensions();
    let (cols, rows) = (width.min(128), height.min(128));
    let mut groups = [ColorGroup::default(); HUES + 1];
    // Sample the center of each grid cell in the oriented original. Alpha is coverage,
    // so RGB values hidden in a transparent PNG cannot dominate the result.
    let center = |cell: u32, cells: u32, size: u32| {
        ((u64::from(cell * 2 + 1) * u64::from(size)) / u64::from(cells * 2)) as u32
    };
    for row in 0..rows {
        for col in 0..cols {
            let [r, g, b, a] = image
                .get_pixel(center(col, cols, width), center(row, rows, height))
                .0;
            groups[hue_group([r, g, b])].add([r, g, b], a);
        }
    }
    let total: u32 = groups.iter().map(|g| g.weight).sum();
    if total == 0 {
        return None;
    }

    // A hue must occupy at least 2% of the visible sample to stand ahead of a neutral field.
    let mut hues: Vec<_> = (0..HUES)
        .filter(|&i| groups[i].weight * 50 >= total)
        .collect();
    hues.sort_by(|&a, &b| groups[b].score().total_cmp(&groups[a].score()));
    let first = hues.first().copied().unwrap_or(NEUTRAL);
    let primary = groups[first].mean();
    let surface = mix([36; 3], primary, 0.14);
    let accent = readable(primary, surface);
    let alternate = hues.iter().copied().find(|&i| {
        let distance = i.abs_diff(first);
        distance.min(HUES - distance) >= 3
    });
    let secondary = match alternate {
        Some(i) => groups[i].mean(),
        None => {
            let neutral = if luminance(accent) > 0.55 { 154 } else { 240 };
            mix(accent, [neutral; 3], 0.55)
        }
    };
    Some(Palette {
        background: mix([16; 3], primary, 0.08),
        surface,
        text: mix([246; 3], primary, 0.07),
        muted: readable(mix([167; 3], primary, 0.18), surface),
        accent,
        secondary: readable(secondary, surface),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgba};

    fn sample(width: u32, pixel: impl Fn(u32) -> [u8; 4]) -> DynamicImage {
        ImageBuffer::from_fn(width, 1, |x, _| Rgba(pixel(x))).into()
    }

    #[test]
    fn finds_a_repeated_hue_across_shades_and_a_distinct_secondary() {
        let img = sample(100, |x| match x {
            0..55 => [232, 232, 232, 255],
            55..70 => [238, 144, 94, 255],
            70..85 => [242, 148, 98, 255],
            85..98 => [96, 174, 218, 255],
            _ => [0, 255, 0, 0],
        });
        let p = extract(&img).unwrap();
        assert_eq!(p.accent, [240, 146, 96]);
        assert_eq!(p.secondary, [96, 174, 218]);
        assert_eq!(extract(&img), Some(p));
    }

    #[test]
    fn transparency_and_isolated_specks_do_not_select_the_accent() {
        let img = sample(100, |x| match x {
            0..90 => [255, 0, 0, 0],
            90..95 => [255, 0, 0, 1],
            _ => [54, 188, 158, 255],
        });
        assert_eq!(extract(&img).unwrap().accent, [54, 188, 158]);
        assert!(extract(&sample(1, |_| [255, 0, 255, 0])).is_none());
        let neutral = extract(&sample(100, |x| {
            if x == 0 {
                [255, 0, 0, 255]
            } else {
                [183, 183, 183, 255]
            }
        }))
        .unwrap();
        assert_eq!(neutral.accent, [183; 3]);
    }

    #[test]
    fn single_pixel_and_dark_photos_produce_readable_palette_roles() {
        for rgb in [[0; 3], [127; 3], [255; 3], [3, 6, 95], [246, 227, 13]] {
            let p = extract(&sample(1, |_| [rgb[0], rgb[1], rgb[2], 255])).unwrap();
            assert!(luminance(p.background) < luminance(p.surface));
            for ink in [p.text, p.muted, p.accent, p.secondary] {
                assert!((luminance(ink) + 0.05) / (luminance(p.surface) + 0.05) >= 4.5);
                if rgb[0] == rgb[1] && rgb[1] == rgb[2] {
                    assert_eq!(ink, [ink[0]; 3]);
                }
            }
        }
    }
    #[test]
    fn imported_transparency_and_generated_theme_survive_save_and_reload() -> anyhow::Result<()> {
        use crate::{
            theme::{Theme, Wallpaper},
            wallpaper,
        };
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("Coastal light.png");
        let image = sample(10, |x| {
            if x < 8 {
                [240, 0, 0, 0]
            } else {
                [90, 190, 223, 255]
            }
        });
        image.save(&path)?;
        let source = Wallpaper::Local(path.clone());
        let palette = extract(&wallpaper::decode(&source)?).unwrap();
        let mut theme = Theme::from_wallpaper(&path, palette.clone());
        theme.validate()?;
        assert_eq!(theme.name, "Coastal light");
        assert!(theme.id.starts_with("custom-"));
        assert!(theme.code_theme.is_empty() && theme.vim_scheme.is_empty());
        theme.wallpaper = Wallpaper::Local(wallpaper::import(dir.path(), &source)?);
        let restored: Theme = serde_json::from_slice(&serde_json::to_vec(&theme)?)?;
        assert_eq!(
            Some(palette),
            extract(&wallpaper::decode(&restored.wallpaper)?)
        );
        assert_eq!(
            wallpaper::load(&restored.wallpaper)?.get_pixel(0, 0).0,
            [0; 3]
        );
        Ok(())
    }
}

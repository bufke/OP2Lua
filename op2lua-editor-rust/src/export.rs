//! The map image with the mission drawn on it, as the editor shows it: regions, dozed borders, walls, beacons,
//! markers and units, with their labels.

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};
use eframe::egui::Color32;
use op2_formats::render::{self, Image, MapTilesets};
use op2_formats::Map;

use crate::mission::Mission;
use op2_formats::placement::ObjectKind;
use crate::units;

/// What the exported image shows over the map.
#[derive(Clone, Copy)]
pub struct Layers {
    /// Units and the dozed ground around structures.
    pub units: bool,
    /// Beacons, walls and tubes, and markers.
    pub objects: bool,
}

/// The map at `px` pixels per tile, with `mission` (if any) drawn over it.
pub fn map_image(map: &Map, tilesets: &MapTilesets, px: u32, mission: Option<&Mission>, layers: Layers) -> Image {
    let mut img = render::compose(map, tilesets, px);
    if let Some(m) = mission {
        let mut c = Canvas { w: img.width as i32, h: img.height as i32, rgba: &mut img.rgba, px: px as i32, font: font() };
        c.mission(m, layers);
    }
    img
}

fn font() -> FontRef<'static> {
    FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("egui's default font")
}

fn rgba(c: Color32, a: u8) -> [u8; 4] {
    [c.r(), c.g(), c.b(), a]
}

struct Canvas<'a> {
    w: i32,
    h: i32,
    rgba: &'a mut Vec<u8>,
    /// Pixels per tile.
    px: i32,
    font: FontRef<'static>,
}

impl Canvas<'_> {
    fn blend(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h || c[3] == 0 {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        let a = c[3] as u32;
        for (d, &s) in self.rgba[i..i + 3].iter_mut().zip(&c[..3]) {
            *d = ((s as u32 * a + *d as u32 * (255 - a)) / 255) as u8;
        }
        self.rgba[i + 3] = 255;
    }

    fn fill(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]) {
        for y in y0.max(0)..y1.min(self.h) {
            for x in x0.max(0)..x1.min(self.w) {
                self.blend(x, y, c);
            }
        }
    }

    fn stroke(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, width: i32, c: [u8; 4]) {
        self.fill(x0, y0, x1, y0 + width, c);
        self.fill(x0, y1 - width, x1, y1, c);
        self.fill(x0, y0 + width, x0 + width, y1 - width, c);
        self.fill(x1 - width, y0 + width, x1, y1 - width, c);
    }

    fn line(&mut self, (x0, y0): (i32, i32), (x1, y1): (i32, i32), c: [u8; 4]) {
        let n = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
        for k in 0..=n {
            self.blend(x0 + (x1 - x0) * k / n, y0 + (y1 - y0) * k / n, c);
        }
    }

    /// The pixel rectangle (exclusive end) of status-bar tiles x0..=x1, y0..=y1.
    fn tiles(&self, x0: i32, y0: i32, x1: i32, y1: i32) -> (i32, i32, i32, i32) {
        let p = self.px;
        ((x0.min(x1) - 1) * p, (y0.min(y1) - 1) * p, x0.max(x1) * p, y0.max(y1) * p)
    }

    fn text_width(&self, size: f32, s: &str) -> f32 {
        let f = self.font.as_scaled(PxScale::from(size));
        s.chars().map(|ch| f.h_advance(f.glyph_id(ch))).sum()
    }

    /// Draws `s` with its top-left at (x, y).
    fn text(&mut self, x: f32, y: f32, size: f32, s: &str, c: [u8; 4]) {
        let scale = PxScale::from(size);
        let ascent = self.font.as_scaled(scale).ascent();
        let mut caret = x;
        for ch in s.chars() {
            let f = self.font.as_scaled(scale);
            let id = f.glyph_id(ch);
            let advance = f.h_advance(id);
            let glyph = id.with_scale_and_position(scale, ab_glyph::point(caret, y + ascent));
            if let Some(outline) = self.font.outline_glyph(glyph) {
                let b = outline.px_bounds();
                let mut cover = Vec::new();
                outline.draw(|gx, gy, v| cover.push((gx as i32, gy as i32, v)));
                for (gx, gy, v) in cover {
                    let a = (v.clamp(0.0, 1.0) * c[3] as f32) as u8;
                    self.blend(b.min.x as i32 + gx, b.min.y as i32 + gy, [c[0], c[1], c[2], a]);
                }
            }
            caret += advance;
        }
    }

    fn label_size(&self) -> f32 {
        (self.px as f32 * 0.5).clamp(9.0, 18.0)
    }

    fn mission(&mut self, m: &Mission, layers: Layers) {
        let p = self.px;
        let labels = p >= 12;
        let line_w = (p / 8).max(1);

        for (i, r) in m.regions.iter().enumerate() {
            let col = units::REGION_COLORS[i % units::REGION_COLORS.len()];
            let (x0, y0, x1, y1) = self.tiles(r.x1, r.y1, r.x2, r.y2);
            self.fill(x0, y0, x1, y1, rgba(col, 70));
            self.stroke(x0, y0, x1, y1, line_w, rgba(col, 255));
            if r.point {
                self.line((x0, y0), (x1 - 1, y1 - 1), rgba(col, 255));
                self.line((x1 - 1, y0), (x0, y1 - 1), rgba(col, 255));
            }
            if labels {
                let size = self.label_size();
                let w = self.text_width(size, &r.name);
                let h = size * 1.25;
                self.fill(x0, y0 - h as i32, x0 + w as i32 + 6, y0, [0, 0, 0, 190]);
                self.text(x0 as f32 + 3.0, y0 as f32 - h, size, &r.name, rgba(col, 255));
            }
        }

        if layers.units {
            let mut dozed = std::collections::BTreeSet::new();
            for u in m.units.units.iter().filter(|u| u.has_at && units::has_dozed_border(&u.unit_type)) {
                let (x0, y0, x1, y1) = units::footprint_tiles(&u.unit_type, u.x, u.y);
                for x in x0 - 1..=x1 + 1 {
                    for y in y0 - 1..=y1 + 1 {
                        dozed.insert((x, y));
                    }
                }
            }
            for (x, y) in dozed {
                let (a, b, c, d) = self.tiles(x, y, x, y);
                self.fill(a, b, c, d, [72, 66, 60, 150]);
            }
        }

        if layers.units {
            for u in m.units.units.iter().filter(|u| u.has_at) {
                for (x, y) in units::dropped_tubes(&u.unit_type, u.x, u.y).into_iter().flatten() {
                    let (a, b, c, d) = self.tiles(x, y, x, y);
                    self.fill(a, b, c, d, [205, 170, 110, 170]);
                }
            }
        }

        if layers.objects {
            for o in m.objects.iter().filter(|o| o.kind == ObjectKind::Wall) {
                let col = match o.obj_type.as_str() {
                    "Tube" => [205, 170, 110, 170],
                    "LavaWall" => [230, 90, 30, 190],
                    "MicrobeWall" => [110, 160, 50, 190],
                    _ => [120, 120, 120, 190],
                };
                let (a, b, c, d) = self.tiles(o.x, o.y, o.x, o.y);
                self.fill(a, b, c, d, col);
            }
            for o in m.objects.iter().filter(|o| o.kind != ObjectKind::Wall) {
                let (x0, y0, _, _) = self.tiles(o.x, o.y, o.x, o.y);
                let (cx, cy) = (x0 + p / 2, y0 + p / 2);
                let h = ((p as f32) * 0.45).max(2.0) as i32;
                if o.kind == ObjectKind::Beacon {
                    let (label, col) = match (o.obj_type.as_str(), o.ore.as_str()) {
                        ("MagmaVent", _) => ("V", [255, 69, 0, 210]),
                        ("Fumarole", _) => ("F", [147, 112, 219, 210]),
                        (_, "Common") => ("C", [255, 215, 0, 210]),
                        (_, "Rare") => ("R", [0, 191, 255, 210]),
                        _ => ("?", [245, 245, 245, 210]),
                    };
                    for dy in -h..=h {
                        let half = h - dy.abs();
                        for dx in -half..=half {
                            let edge = dx.abs() == half;
                            self.blend(cx + dx, cy + dy, if edge { [0, 0, 0, 255] } else { col });
                        }
                    }
                    if labels {
                        let size = (p as f32 * 0.35).max(8.0);
                        let w = self.text_width(size, label);
                        self.text(cx as f32 - w / 2.0, cy as f32 - size * 0.6, size, label, [0, 0, 0, 255]);
                    }
                } else {
                    let width = (p as f32 / 12.0).max(1.0);
                    for dy in -h - 2..=h + 2 {
                        for dx in -h - 2..=h + 2 {
                            let d = ((dx * dx + dy * dy) as f32).sqrt();
                            if (d - h as f32).abs() <= width / 2.0 + 0.5 {
                                self.blend(cx + dx, cy + dy, [255, 0, 255, 255]);
                            }
                        }
                    }
                    if labels && let Some(name) = &o.name {
                        let size = self.label_size();
                        self.text((x0 + p) as f32, y0 as f32, size, name, [255, 0, 255, 255]);
                    }
                }
            }
        }

        if layers.units {
            for u in m.units.units.iter().filter(|u| u.has_at) {
                let (tx0, ty0, tx1, ty1) = units::footprint_tiles(&u.unit_type, u.x, u.y);
                let (x0, y0, x1, y1) = self.tiles(tx0, ty0, tx1, ty1);
                self.fill(x0, y0, x1, y1, rgba(units::player_color(u.player), 200));
                self.stroke(x0, y0, x1, y1, 1, [0, 0, 0, 255]);
                if labels {
                    let label = units::abbreviation(&u.unit_type);
                    let size = (p as f32 * 0.4).clamp(9.0, 16.0);
                    let w = self.text_width(size, &label);
                    let (lx, ly) = ((x0 + x1) as f32 / 2.0 - w / 2.0, (y0 + y1) as f32 / 2.0 - size * 0.6);
                    self.text(lx + 1.0, ly + 1.0, size, &label, [0, 0, 0, 255]);
                    self.text(lx, ly, size, &label, [255, 255, 255, 255]);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// eden01 with a region and a unit on it: the region's border and the unit's box are drawn, labels included.
    /// Needs the game: OP2_DIR, else skipped.
    #[test]
    fn draws_the_mission_over_the_map() {
        let Some(dir) = std::env::var_os("OP2_DIR") else { return };
        let files = op2_formats::GameFiles::open(std::path::PathBuf::from(dir)).unwrap();
        let map = files.read_map("eden01.map").unwrap();
        let tilesets = MapTilesets::load(&files, &map);
        let text = "return {
  units = { { type = \"CommandCenter\", player = 2, at = { 10, 10 } }, { type = \"Lynx\", player = 1, at = { 22, 22 }, weapon = \"Laser\" } },
  beacons = { { at = { 30, 12 }, ore = \"Rare\" } },
  markers = { home = { at = { 14, 30 } } },
  regions = { base = { 20, 20, 25, 24 }, spot = { 30, 30 } },
}
";
        let m = Mission::from_text(std::path::Path::new("t.placement.lua"), text);
        let layers = Layers { units: true, objects: true };
        let plain = render::compose(&map, &tilesets, 32);
        let drawn = map_image(&map, &tilesets, 32, Some(&m), layers);
        let px = |img: &Image, x: u32, y: u32| {
            let i = ((y * img.width + x) * 4) as usize;
            [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
        };
        // OP2LUAEDITOR_EXPORT_PNG names a file to save the picture to, for a look.
        if let Some(out) = std::env::var_os("OP2LUAEDITOR_EXPORT_PNG") {
            image::save_buffer(out, &drawn.rgba, drawn.width, drawn.height, image::ColorType::Rgba8).unwrap();
        }
        // The region's top-left border pixel is its colour (cyan, the first region's).
        assert_eq!(px(&drawn, 19 * 32, 19 * 32), [0, 255, 255]);
        // Inside the CC's box (3 x 2 around 10, 10) the map is tinted, and the label row above the region changed.
        assert_ne!(px(&drawn, 9 * 32 + 4, 9 * 32 + 4), px(&plain, 9 * 32 + 4, 9 * 32 + 4));
        let row = 19 * 32 - 6;
        assert!((19 * 32..19 * 32 + 40).any(|x| px(&drawn, x, row) != px(&plain, x, row)));
        // Far from both, nothing changed.
        assert_eq!(px(&drawn, 50 * 32, 50 * 32), px(&plain, 50 * 32, 50 * 32));
    }
}

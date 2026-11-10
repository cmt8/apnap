//! Card images and mana symbols for the UI.
//!
//! Scryfall's files are downloaded on demand into the user's cache directory by a few
//! worker threads (never on the UI thread, never into the repository), decoded there, and
//! handed back as textures. Until an image is ready — or when a card has none (the invented
//! test cards, tokens) or the app is offline — the UI draws the card from its definition.
//!
//! Only this module and `card_db` touch the network or the cache.

use card_db::images::{self, ImageKind};
use card_db::scryfall::{self, ScryfallData};
use eframe::egui::{self, ColorImage, TextureHandle};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

/// Width of Scryfall's `normal` image; anything shown wider uses `large`.
const NORMAL_WIDTH: u32 = 488;
/// A texture nothing has drawn for this many frames is dropped.
const UNUSED_FRAMES: u64 = 1_500;
const WORKERS: usize = 3;

/// A texture is made for the exact number of screen pixels it is drawn at (scaled once, with
/// a good filter, on a worker), so cards and symbols are as sharp as the screen allows.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Key {
    /// Card name, width in pixels.
    Card(String, u32),
    /// Symbol code, side in pixels.
    Symbol(String, u32),
}

enum State {
    Loading,
    Ready(TextureHandle),
    Failed,
}

struct Slot {
    state: State,
    /// The frame it was last asked for.
    used: u64,
}

enum Job {
    Image {
        key: Key,
        path: PathBuf,
        /// `Some` = download it if it isn't cached.
        url: Option<String>,
        ctx: egui::Context,
    },
    Data {
        names: Vec<String>,
        ctx: egui::Context,
    },
}

enum Done {
    Image {
        key: Key,
        result: Result<ColorImage, String>,
    },
    Data(Result<ScryfallData, String>),
}

pub struct Art {
    /// The cache directory images are stored under.
    root: PathBuf,
    data: ScryfallData,
    /// May files be downloaded? (Otherwise only what is already cached is shown.)
    pub online: bool,
    jobs: Option<mpsc::Sender<Job>>,
    done: Option<mpsc::Receiver<Done>>,
    slots: BTreeMap<Key, Slot>,
    frame: u64,
    data_requested: bool,
    /// Files being downloaded or decoded right now.
    pub pending: usize,
    /// The most recent problem, for the status line.
    pub last_error: Option<String>,
}

impl std::fmt::Debug for Art {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Art({} cards known, {} textures, online: {})",
            self.data.cards.len(),
            self.slots.len(),
            self.online
        )
    }
}

impl Art {
    /// No images at all: every card is drawn from its definition. Starts no threads.
    pub fn disabled() -> Art {
        Art {
            root: PathBuf::new(),
            data: ScryfallData::default(),
            online: false,
            jobs: None,
            done: None,
            slots: BTreeMap::new(),
            frame: 0,
            data_requested: true,
            pending: 0,
            last_error: None,
        }
    }

    /// Images from the user's cache; missing ones are downloaded if `online`.
    pub fn new(online: bool) -> Art {
        let (data, err) = match scryfall::load_cache() {
            Ok(d) => (d, None),
            Err(e) => (ScryfallData::default(), Some(e.to_string())),
        };
        let mut art = Art::with_cache(scryfall::cache_dir(), data, online);
        art.last_error = err;
        art
    }

    /// Images under `root`, described by `data`.
    pub fn with_cache(root: PathBuf, data: ScryfallData, online: bool) -> Art {
        let (job_tx, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done_rx) = mpsc::channel::<Done>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for _ in 0..WORKERS {
            let job_rx = Arc::clone(&job_rx);
            let done_tx = done_tx.clone();
            std::thread::spawn(move || worker(&job_rx, &done_tx));
        }
        Art {
            root,
            data,
            online,
            jobs: Some(job_tx),
            done: Some(done_rx),
            slots: BTreeMap::new(),
            frame: 0,
            data_requested: false,
            pending: 0,
            last_error: None,
        }
    }

    pub fn enabled(&self) -> bool {
        self.jobs.is_some()
    }

    /// How many textures are held (for tests and the debug line).
    pub fn textures(&self) -> usize {
        self.slots
            .values()
            .filter(|s| matches!(s.state, State::Ready(_)))
            .count()
    }

    /// Make sure the printed data (image addresses, artists) for `names` is cached,
    /// downloading it once in the background if allowed.
    pub fn want_data(&mut self, ctx: &egui::Context, names: impl FnOnce() -> Vec<String>) {
        if self.data_requested || !self.online {
            return;
        }
        self.data_requested = true;
        let names = names();
        if scryfall::is_complete(&self.data, &names) {
            return;
        }
        if let Some(tx) = &self.jobs {
            self.pending += 1;
            let _ = tx.send(Job::Data {
                names,
                ctx: ctx.clone(),
            });
        }
    }

    /// Take finished work from the workers and drop textures nothing draws any more. Call
    /// once per frame.
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.frame += 1;
        let frame = self.frame;
        self.slots
            .retain(|_, s| matches!(s.state, State::Loading) || s.used + UNUSED_FRAMES >= frame);
        let Some(rx) = &self.done else {
            return;
        };
        let finished: Vec<Done> = rx.try_iter().collect();
        for d in finished {
            self.pending = self.pending.saturating_sub(1);
            match d {
                Done::Image { key, result } => {
                    let state = match result {
                        Ok(img) => {
                            let name = format!("{key:?}");
                            State::Ready(ctx.load_texture(name, img, egui::TextureOptions::LINEAR))
                        }
                        Err(e) => {
                            self.last_error = Some(e);
                            State::Failed
                        }
                    };
                    self.slots.insert(key, Slot { state, used: frame });
                }
                Done::Data(Ok(data)) => {
                    self.data = data;
                    // Cards that had no image address may have one now.
                    self.slots.retain(|_, s| !matches!(s.state, State::Failed));
                }
                Done::Data(Err(e)) => self.last_error = Some(e),
            }
        }
    }

    fn request(&mut self, ctx: &egui::Context, key: Key) -> Option<TextureHandle> {
        if let Some(slot) = self.slots.get_mut(&key) {
            slot.used = self.frame;
            return match &slot.state {
                State::Ready(t) => Some(t.clone()),
                State::Loading | State::Failed => None,
            };
        }
        let failed = Slot {
            state: State::Failed,
            used: self.frame,
        };
        let tx = self.jobs.as_ref()?;
        let (path, url) = match &key {
            Key::Card(name, px) => {
                let Some(card) = self.data.get(name) else {
                    // Not a real card (or its data hasn't arrived yet: `poll` retries).
                    self.slots.insert(key, failed);
                    return None;
                };
                // The smallest of Scryfall's sizes that has enough pixels.
                let kind =
                    if *px > NORMAL_WIDTH && images::image_url(card, ImageKind::Large).is_some() {
                        ImageKind::Large
                    } else {
                        ImageKind::Normal
                    };
                let Some(path) = images::image_path(card, kind) else {
                    self.slots.insert(key, failed);
                    return None;
                };
                (path, images::image_url(card, kind).map(str::to_string))
            }
            Key::Symbol(code, _) => (images::symbol_path(code), Some(images::symbol_url(code))),
        };
        let job = Job::Image {
            key: key.clone(),
            path: self.root.join(path),
            url: url.filter(|_| self.online),
            ctx: ctx.clone(),
        };
        if tx.send(job).is_ok() {
            self.pending += 1;
            self.slots.insert(
                key,
                Slot {
                    state: State::Loading,
                    used: self.frame,
                },
            );
        }
        None
    }

    /// The card's image for drawing it `width_px` screen pixels wide. While that exact size
    /// is being made, the closest size already there is returned instead.
    pub fn card(
        &mut self,
        ctx: &egui::Context,
        name: &str,
        width_px: u32,
    ) -> Option<TextureHandle> {
        let width_px = width_px.max(1);
        if let Some(t) = self.request(ctx, Key::Card(name.to_string(), width_px)) {
            return Some(t);
        }
        let all = Key::Card(name.to_string(), 0)..=Key::Card(name.to_string(), u32::MAX);
        let frame = self.frame;
        self.slots
            .range_mut(all)
            .filter_map(|(k, s)| match (k, &s.state) {
                (Key::Card(_, px), State::Ready(t)) => Some((px.abs_diff(width_px), t.clone(), s)),
                _ => None,
            })
            .min_by_key(|(d, _, _)| *d)
            .map(|(_, t, slot)| {
                slot.used = frame;
                t
            })
    }

    /// The texture for a symbol such as `{G}` or `{T}`, `side_px` screen pixels across.
    pub fn symbol(
        &mut self,
        ctx: &egui::Context,
        symbol: &str,
        side_px: u32,
    ) -> Option<TextureHandle> {
        let code = images::symbol_code(symbol)?;
        self.request(ctx, Key::Symbol(code, side_px.max(1)))
    }

    /// "Illustrated by …", for the enlarged view of a real card.
    pub fn credit(&self, name: &str) -> Option<String> {
        let card = self.data.get(name)?;
        let artist = card.artist.as_deref()?;
        Some(match (&card.set, &card.collector_number) {
            (Some(set), Some(n)) => {
                format!("Illustrated by {artist} · {} #{n}", set.to_uppercase())
            }
            _ => format!("Illustrated by {artist}"),
        })
    }
}

fn worker(jobs: &Mutex<mpsc::Receiver<Job>>, done: &mpsc::Sender<Done>) {
    loop {
        // Hold the lock only while waiting for a job, not while working on it.
        let job = match jobs.lock() {
            Ok(rx) => rx.recv(),
            Err(_) => return,
        };
        let Ok(job) = job else {
            return; // the app is gone
        };
        let (msg, ctx) = match job {
            Job::Image {
                key,
                path,
                url,
                ctx,
            } => {
                let result = load(&key, &path, url.as_deref());
                (Done::Image { key, result }, ctx)
            }
            Job::Data { names, ctx } => (
                Done::Data(
                    scryfall::fetch(&names)
                        .map(|(d, _)| d)
                        .map_err(|e| e.to_string()),
                ),
                ctx,
            ),
        };
        if done.send(msg).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

fn load(key: &Key, path: &Path, url: Option<&str>) -> Result<ColorImage, String> {
    if !path.exists() {
        match url {
            Some(url) => images::download(url, path).map_err(|e| e.to_string())?,
            None => return Err(format!("{} is not cached", path.display())),
        }
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let decoded = match key {
        Key::Card(_, px) => decode_card(&bytes, *px),
        Key::Symbol(_, px) => decode_svg(&bytes, *px),
    };
    decoded.map_err(|e| {
        // A file that doesn't decode is useless: remove it so it is fetched again.
        let _ = std::fs::remove_file(path);
        format!("{}: {e}", path.display())
    })
}

/// Decode a card image, scale it down to `width` pixels if it is wider (never up: the
/// screen does that better), and cut the corners round (the JPGs have square, filled
/// corners).
pub fn decode_card(bytes: &[u8], width: u32) -> Result<ColorImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let mut rgba = img.to_rgba8();
    if width < rgba.width() {
        let h = (u64::from(rgba.height()) * u64::from(width) / u64::from(rgba.width())) as u32;
        rgba = image::imageops::resize(
            &rgba,
            width.max(1),
            h.max(1),
            image::imageops::FilterType::Lanczos3,
        );
    }
    let (w, h) = (rgba.width() as usize, rgba.height() as usize);
    let mut px = rgba.into_raw();
    round_corners(&mut px, w, h);
    Ok(ColorImage::from_rgba_unmultiplied([w, h], &px))
}

/// A Magic card's corner radius is about 3 mm of its 63 mm width.
const CORNER: f32 = 0.048;

/// Fade out everything outside the rounded-rectangle outline (RGBA, not premultiplied).
pub fn round_corners(px: &mut [u8], w: usize, h: usize) {
    let r = (w as f32 * CORNER).round();
    let ri = r as usize;
    if ri == 0 || ri * 2 > w || ri * 2 > h {
        return;
    }
    for y in 0..ri {
        for x in 0..ri {
            // Distance from the centre of the corner's circle, sampled at the pixel centre.
            let dx = r - (x as f32 + 0.5);
            let dy = r - (y as f32 + 0.5);
            let cover = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            if cover >= 1.0 {
                continue;
            }
            for (cx, cy) in [
                (x, y),
                (w - 1 - x, y),
                (x, h - 1 - y),
                (w - 1 - x, h - 1 - y),
            ] {
                let a = &mut px[(cy * w + cx) * 4 + 3];
                *a = (f32::from(*a) * cover) as u8;
            }
        }
    }
}

/// Rasterise an SVG symbol into a `px` x `px` image.
pub fn decode_svg(bytes: &[u8], px: u32) -> Result<ColorImage, String> {
    use resvg::{tiny_skia, usvg};
    let tree =
        usvg::Tree::from_data(bytes, &usvg::Options::default()).map_err(|e| e.to_string())?;
    let size = tree.size();
    let mut pixmap = tiny_skia::Pixmap::new(px, px).ok_or("empty symbol")?;
    let scale = (px as f32 / size.width()).min(px as f32 / size.height());
    let dx = (px as f32 - size.width() * scale) / 2.0;
    let dy = (px as f32 - size.height() * scale) / 2.0;
    let transform = tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(ColorImage::from_rgba_premultiplied(
        [px as usize, px as usize],
        pixmap.data(),
    ))
}

/// A piece of text that may contain symbols: `"{2}{G}: draw"` is two symbols and a text run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seg<'a> {
    Text(&'a str),
    /// With its braces, e.g. `{G}`.
    Symbol(&'a str),
}

/// Split text into plain runs and `{…}` symbols.
pub fn segments(text: &str) -> Vec<Seg<'_>> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}').map(|c| open + c);
        match close {
            Some(close) if images::symbol_code(&rest[open..=close]).is_some() => {
                if open > 0 {
                    out.push(Seg::Text(&rest[..open]));
                }
                out.push(Seg::Symbol(&rest[open..=close]));
                rest = &rest[close + 1..];
            }
            _ => {
                // Not a symbol: keep the brace as text and look further on.
                let upto = open + 1;
                out.push(Seg::Text(&rest[..upto]));
                rest = &rest[upto..];
            }
        }
    }
    if !rest.is_empty() {
        out.push(Seg::Text(rest));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_splits_into_runs_and_symbols() {
        assert_eq!(
            segments("{2}{G}: draw"),
            [Seg::Symbol("{2}"), Seg::Symbol("{G}"), Seg::Text(": draw")]
        );
        assert_eq!(
            segments("tap for {W/U}."),
            [Seg::Text("tap for "), Seg::Symbol("{W/U}"), Seg::Text(".")]
        );
        assert_eq!(segments("no symbols"), [Seg::Text("no symbols")]);
        assert_eq!(
            segments("odd {brace} {G}"),
            [Seg::Text("odd {"), Seg::Text("brace} "), Seg::Symbol("{G}")]
        );
        assert!(segments("").is_empty());
    }

    #[test]
    fn corners_are_cut_and_the_middle_is_kept() {
        let (w, h) = (100, 140);
        let mut px = vec![255u8; w * h * 4];
        round_corners(&mut px, w, h);
        let alpha = |x: usize, y: usize| px[(y * w + x) * 4 + 3];
        for (x, y) in [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)] {
            assert_eq!(alpha(x, y), 0, "corner {x},{y}");
        }
        assert_eq!(alpha(w / 2, 0), 255);
        assert_eq!(alpha(0, h / 2), 255);
        assert_eq!(alpha(w / 2, h / 2), 255);
        assert_eq!(alpha(6, 6), 255);
    }

    #[test]
    fn a_cached_card_image_is_decoded_scaled_and_rounded() {
        let img = image::RgbImage::from_pixel(488, 680, image::Rgb([200, 30, 30]));
        let mut jpg = Vec::new();
        image::DynamicImage::ImageRgb8(img)
            .write_to(
                &mut std::io::Cursor::new(&mut jpg),
                image::ImageFormat::Jpeg,
            )
            .unwrap();
        let board = decode_card(&jpg, 320).unwrap();
        assert_eq!(board.size, [320, 445]);
        assert_eq!(board.pixels[0].a(), 0);
        assert!(board.pixels[445 / 2 * 320 + 160].r() > 150);
        // Never scaled up: asking for more pixels than the file has gives the file's size.
        let full = decode_card(&jpg, 900).unwrap();
        assert_eq!(full.size, [488, 680]);
        assert!(decode_card(b"not an image", 320).is_err());
    }

    #[test]
    fn an_svg_symbol_is_rasterised() {
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
            <circle cx="50" cy="50" r="50" fill="#9bd3ae"/></svg>"##;
        let img = decode_svg(svg, 64).unwrap();
        assert_eq!(img.size, [64, 64]);
        assert_eq!(img.pixels[0].a(), 0); // outside the circle
        assert!(img.pixels[32 * 64 + 32].g() > 150); // inside
        assert!(decode_svg(b"<nope", 64).is_err());
    }

    /// Wait for the workers, then return what `get` yields.
    fn settle(
        art: &mut Art,
        ctx: &egui::Context,
        get: impl Fn(&mut Art, &egui::Context) -> Option<TextureHandle>,
    ) -> Option<TextureHandle> {
        let _ = get(art, ctx); // the request
        for _ in 0..500 {
            art.poll(ctx);
            if art.pending == 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        get(art, ctx)
    }

    #[test]
    fn cached_images_load_offline_and_missing_ones_fall_back() {
        let root = std::env::temp_dir().join(format!("apnap-art-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let data: ScryfallData = serde_json::from_str(
            r#"{"cards": {
                "Forest": {"name": "Forest", "id": "aaaa-1111", "artist": "A. Painter",
                           "set": "tst", "collector_number": "7",
                           "image_uris": {"normal": "https://cards.scryfall.io/normal/f.jpg",
                                          "large": "https://cards.scryfall.io/large/f.jpg"}},
                "Swamp": {"name": "Swamp", "id": "bbbb-2222",
                          "image_uris": {"normal": "https://cards.scryfall.io/normal/s.jpg"}}
            }}"#,
        )
        .unwrap();
        // Only the Forest's image is in the cache.
        let forest = images::image_path(data.get("Forest").unwrap(), ImageKind::Normal).unwrap();
        std::fs::create_dir_all(root.join(&forest).parent().unwrap()).unwrap();
        let img = image::RgbImage::from_pixel(488, 680, image::Rgb([20, 120, 40]));
        image::DynamicImage::ImageRgb8(img)
            .save(root.join(&forest))
            .unwrap();
        std::fs::create_dir_all(root.join(images::symbol_path("G")).parent().unwrap()).unwrap();
        std::fs::write(
            root.join(images::symbol_path("G")),
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
                <circle cx="5" cy="5" r="5" fill="#9bd3ae"/></svg>"##,
        )
        .unwrap();

        let ctx = egui::Context::default();
        let mut art = Art::with_cache(root.clone(), data, false);
        // Each size asked for is made at exactly that many pixels.
        let board = settle(&mut art, &ctx, |a, c| a.card(c, "Forest", 160)).expect("cached");
        assert_eq!(board.size(), [160, 222]);
        let full = settle(&mut art, &ctx, |a, c| a.card(c, "Forest", 488)).expect("cached");
        assert_eq!(full.size(), [488, 680]);
        // A size that isn't ready yet is stood in for by the closest one that is.
        let stand_in = art.card(&ctx, "Forest", 200).expect("a stand-in");
        assert_eq!(stand_in.size(), [160, 222]);
        let exact = settle(&mut art, &ctx, |a, c| a.card(c, "Forest", 200)).expect("cached");
        assert_eq!(exact.size(), [200, 278]);
        // Wider than `normal`: the `large` file is wanted, and it isn't cached — offline,
        // the widest size already made stands in and nothing is fetched.
        let big = settle(&mut art, &ctx, |a, c| a.card(c, "Forest", 600)).expect("a stand-in");
        assert_eq!(big.size(), [488, 680]);
        assert!(
            art.last_error
                .as_deref()
                .is_some_and(|e| e.contains("not cached"))
        );
        art.last_error = None;
        assert_eq!(
            art.credit("Forest").as_deref(),
            Some("Illustrated by A. Painter · TST #7")
        );
        let symbol = settle(&mut art, &ctx, |a, c| a.symbol(c, "{G}", 18)).expect("cached");
        assert_eq!(symbol.size(), [18, 18]);
        assert!(art.last_error.is_none());

        // Offline, an image that isn't cached is simply not shown — and nothing is fetched.
        assert!(settle(&mut art, &ctx, |a, c| a.card(c, "Swamp", 160)).is_none());
        assert!(
            art.last_error
                .as_deref()
                .is_some_and(|e| e.contains("not cached"))
        );
        let swamp = images::image_path(art.data.get("Swamp").unwrap(), ImageKind::Normal).unwrap();
        assert!(!root.join(swamp).exists());
        assert!(settle(&mut art, &ctx, |a, c| a.symbol(c, "{R}", 18)).is_none());
        // Textures nothing draws any more are dropped.
        let held = art.textures();
        assert!(held >= 4);
        for _ in 0..=UNUSED_FRAMES {
            art.poll(&ctx);
        }
        assert_eq!(art.textures(), 0);
        // A card Scryfall doesn't know (an invented test card) is never even requested.
        assert!(art.card(&ctx, "Test Golem", 160).is_none());
        assert_eq!(art.pending, 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn without_images_nothing_is_requested() {
        let ctx = egui::Context::default();
        let mut art = Art::disabled();
        assert!(art.card(&ctx, "Forest", 160).is_none());
        assert!(art.symbol(&ctx, "{G}", 18).is_none());
        art.poll(&ctx);
        assert_eq!(art.pending, 0);
        assert!(!art.enabled());
    }
}

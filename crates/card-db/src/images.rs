//! Card images and mana symbols from Scryfall, downloaded on demand into the user's cache
//! directory next to the card data (`<cache>/scryfall/images`, `<cache>/scryfall/symbols`).
//! **Never written into the repository** (CLAUDE.md invariant 13).
//!
//! Scryfall's image guidelines: don't crop out or cover the copyright and artist line, don't
//! distort or recolour the card, and credit the artist where an art crop is shown. The app
//! shows full cards (scaled, and turned sideways when tapped) and names the artist under
//! the enlarged card.
//!
//! Images come from `cards.scryfall.io` and symbols from `svgs.scryfall.io`; those file
//! hosts have no rate limit, but each file is downloaded at most once and kept.

use crate::scryfall::{ScryfallCard, ScryfallError, USER_AGENT};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ImageKind {
    /// The whole card (JPG, 488 x 680).
    Normal,
    /// The whole card (JPG, 672 x 936).
    Large,
    /// The illustration only (JPG).
    ArtCrop,
}

/// The only hosts images are ever requested from. The URLs come out of the cache file, so
/// they are checked rather than trusted.
const IMAGE_HOSTS: [&str; 2] = ["https://cards.scryfall.io/", "https://svgs.scryfall.io/"];

pub fn image_url(card: &ScryfallCard, kind: ImageKind) -> Option<&str> {
    let uris = card.image_uris.as_ref()?;
    match kind {
        ImageKind::Normal => uris.normal.as_deref(),
        ImageKind::Large => uris.large.as_deref(),
        ImageKind::ArtCrop => uris.art_crop.as_deref(),
    }
}

/// Where the image is (or will be) cached, relative to the cache directory
/// ([`crate::scryfall::cache_dir`]). `None` if the card data has no printing id.
pub fn image_path(card: &ScryfallCard, kind: ImageKind) -> Option<PathBuf> {
    let id = card.id.as_deref()?;
    // The id is a UUID; refuse anything that could leave the directory.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let dir = match kind {
        ImageKind::Normal => "normal",
        ImageKind::Large => "large",
        ImageKind::ArtCrop => "art_crop",
    };
    Some(
        Path::new("scryfall")
            .join("images")
            .join(dir)
            .join(format!("{id}.jpg")),
    )
}

/// The file name Scryfall uses for a symbol: `{W}` -> `W`, `{2/W}` -> `2W`, `{T}` -> `T`.
/// `None` for anything that isn't a plausible symbol.
pub fn symbol_code(symbol: &str) -> Option<String> {
    let inner = symbol.strip_prefix('{')?.strip_suffix('}')?;
    let code: String = inner.chars().filter(|c| *c != '/').collect();
    let ok = !code.is_empty()
        && code.len() <= 6
        && code
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    ok.then_some(code)
}

pub fn symbol_url(code: &str) -> String {
    format!("https://svgs.scryfall.io/card-symbols/{code}.svg")
}

/// Where a symbol is cached, relative to the cache directory.
pub fn symbol_path(code: &str) -> PathBuf {
    Path::new("scryfall")
        .join("symbols")
        .join(format!("{code}.svg"))
}

/// Download `url` to `path` (written to a temporary file first, so a partial download is
/// never mistaken for the image).
pub fn download(url: &str, path: &Path) -> Result<(), ScryfallError> {
    if !IMAGE_HOSTS.iter().any(|h| url.starts_with(h)) {
        return Err(ScryfallError::Http(format!(
            "not a Scryfall image URL: {url}"
        )));
    }
    let io = |e: std::io::Error| ScryfallError::Io(format!("{}: {e}", path.display()));
    let mut resp = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| ScryfallError::Http(format!("{url}: {e}")))?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(8 * 1024 * 1024)
        .read_to_vec()
        .map_err(|e| ScryfallError::Http(format!("{url}: {e}")))?;
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(io)?;
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, bytes).map_err(io)?;
    std::fs::rename(&tmp, path).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_codes_follow_scryfalls_file_names() {
        assert_eq!(symbol_code("{W}").as_deref(), Some("W"));
        assert_eq!(symbol_code("{10}").as_deref(), Some("10"));
        assert_eq!(symbol_code("{2/W}").as_deref(), Some("2W"));
        assert_eq!(symbol_code("{W/U}").as_deref(), Some("WU"));
        assert_eq!(symbol_code("{T}").as_deref(), Some("T"));
        assert_eq!(symbol_code("{../x}"), None);
        assert_eq!(symbol_code("W"), None);
        assert_eq!(symbol_code("{}"), None);
    }

    #[test]
    fn downloads_are_refused_from_other_hosts() {
        let err = download("https://example.com/x.jpg", Path::new("/nonexistent/x.jpg"));
        assert!(matches!(err, Err(ScryfallError::Http(_))));
    }

    #[test]
    fn image_paths_never_leave_the_cache() {
        let mut card: ScryfallCard = serde_json::from_str(r#"{"name":"Test"}"#).unwrap();
        assert_eq!(image_path(&card, ImageKind::Normal), None);
        card.id = Some("../../etc".into());
        assert_eq!(image_path(&card, ImageKind::Normal), None);
        card.id = Some("2e781bfd-f8cb-4d20-b977-4359f3a21311".into());
        let p = image_path(&card, ImageKind::Normal).unwrap();
        assert!(p.ends_with("scryfall/images/normal/2e781bfd-f8cb-4d20-b977-4359f3a21311.jpg"));
    }
}

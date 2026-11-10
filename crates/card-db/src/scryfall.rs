//! Printed card data from Scryfall (spec §12), fetched at runtime into the user's cache
//! directory. **Nothing fetched here is ever written into the repository** (CLAUDE.md
//! invariant 13): the cache lives under `$APNAP_CACHE_DIR`, or the platform cache directory
//! (`$XDG_CACHE_HOME/apnap`, i.e. `~/.cache/apnap` on Linux).
//!
//! Scryfall's API guidelines: identify the client with a `User-Agent`, send `Accept`, keep
//! to ~10 requests/second, and cache for at least 24 hours. We fetch only the cards the
//! deck pool needs, through the batched `/cards/collection` endpoint (75 names per request),
//! with a pause between requests, and never re-fetch a card that is already cached.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// The fields of a Scryfall card object the engine and UI use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScryfallCard {
    #[serde(default)]
    pub oracle_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub layout: String,
    #[serde(default)]
    pub mana_cost: Option<String>,
    #[serde(default)]
    pub type_line: String,
    #[serde(default)]
    pub oracle_text: Option<String>,
    #[serde(default)]
    pub power: Option<String>,
    #[serde(default)]
    pub toughness: Option<String>,
    #[serde(default)]
    pub colors: Option<Vec<String>>,
    #[serde(default)]
    pub color_identity: Vec<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    /// The Scryfall id of the printing (names the cached image files).
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub image_uris: Option<ImageUris>,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub set: Option<String>,
    #[serde(default)]
    pub collector_number: Option<String>,
    /// "expansion", "core", "funny", "token", … (bulk data).
    #[serde(default)]
    pub set_type: Option<String>,
    #[serde(default)]
    pub border_color: Option<String>,
}

/// Where Scryfall serves this printing's images (we use two of the sizes).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageUris {
    /// The full card, 488 x 680 JPG.
    #[serde(default)]
    pub normal: Option<String>,
    /// The full card, 672 x 936 JPG (for when it is shown larger than `normal`).
    #[serde(default)]
    pub large: Option<String>,
    /// Just the illustration.
    #[serde(default)]
    pub art_crop: Option<String>,
}

impl ScryfallCard {
    /// Cached before the image fields were stored: fetch it again.
    fn is_stale(&self) -> bool {
        self.id.is_none() || self.image_uris.as_ref().is_none_or(|u| u.large.is_none())
    }
}

/// The on-disk cache: cards by name.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScryfallData {
    pub cards: BTreeMap<String, ScryfallCard>,
}

impl ScryfallData {
    pub fn get(&self, name: &str) -> Option<&ScryfallCard> {
        self.cards.get(name)
    }

    pub fn by_oracle_id(&self, id: &str) -> Option<&ScryfallCard> {
        self.cards
            .values()
            .find(|c| c.oracle_id.as_deref() == Some(id))
    }
}

pub(crate) const USER_AGENT: &str = concat!("apnap/", env!("CARGO_PKG_VERSION"));

/// How to get the data, shown whenever it is missing.
pub const FETCH_HINT: &str = "card data is not in the local cache; run `cargo run -p harness -- fetch-cards` (needs network)";

#[derive(Debug)]
pub enum ScryfallError {
    /// The cache is missing (or lacks these cards). See [`FETCH_HINT`].
    NotCached(Vec<String>),
    Io(String),
    Http(String),
    /// Scryfall doesn't know these names.
    NotFound(Vec<String>),
}

impl std::fmt::Display for ScryfallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScryfallError::NotCached(names) => {
                write!(
                    f,
                    "{FETCH_HINT} — missing {} card(s), e.g. {:?}",
                    names.len(),
                    names.first()
                )
            }
            ScryfallError::Io(e) => write!(f, "cache I/O error: {e}"),
            ScryfallError::Http(e) => write!(f, "Scryfall request failed: {e}"),
            ScryfallError::NotFound(n) => write!(f, "Scryfall has no card named {n:?}"),
        }
    }
}

impl std::error::Error for ScryfallError {}

/// The apnap cache directory (not created).
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("APNAP_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from(".cache"))
        .join("apnap")
}

/// True if every one of `names` is cached with everything [`fetch`] would store.
pub fn is_complete(data: &ScryfallData, names: &[String]) -> bool {
    names
        .iter()
        .all(|n| data.get(n).is_some_and(|c| !c.is_stale()))
}

fn cache_file() -> PathBuf {
    cache_dir().join("scryfall").join("cards.json")
}

/// Load whatever is cached (empty if there is no cache yet).
pub fn load_cache() -> Result<ScryfallData, ScryfallError> {
    let path = cache_file();
    match std::fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|e| ScryfallError::Io(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ScryfallData::default()),
        Err(e) => Err(ScryfallError::Io(format!("{}: {e}", path.display()))),
    }
}

fn save_cache(data: &ScryfallData) -> Result<(), ScryfallError> {
    let path = cache_file();
    let io = |e: std::io::Error| ScryfallError::Io(format!("{}: {e}", path.display()));
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(io)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string(data).expect("serializes")).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)
}

/// The cached data for `names`, without touching the network. Errors if any is missing.
pub fn cached(names: &[String]) -> Result<ScryfallData, ScryfallError> {
    let data = load_cache()?;
    let missing: Vec<String> = names
        .iter()
        .filter(|n| data.get(n).is_none())
        .cloned()
        .collect();
    if missing.is_empty() {
        Ok(data)
    } else {
        Err(ScryfallError::NotCached(missing))
    }
}

#[derive(Deserialize)]
struct Collection {
    #[serde(default)]
    data: Vec<ScryfallCard>,
    #[serde(default)]
    not_found: Vec<serde_json::Value>,
}

fn bulk_file() -> PathBuf {
    cache_dir().join("bulk").join("oracle-cards.jsonl.gz")
}

/// How to get the bulk file, shown whenever it is missing.
pub const BULK_HINT: &str = "Scryfall's Oracle card file is not in the local cache; run `cargo run -p harness -- fetch-bulk` (needs network)";

/// Download Scryfall's "Oracle Cards" bulk file (one record per card ever printed, ~25 MB)
/// into the cache. Scryfall asks that bulk data be used for anything card-pool-wide instead
/// of the API. Returns the file's size in bytes.
pub fn fetch_bulk() -> Result<u64, ScryfallError> {
    let http = |e: ureq::Error| ScryfallError::Http(e.to_string());
    let mut listing = ureq::get("https://api.scryfall.com/bulk-data")
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/json")
        .call()
        .map_err(http)?;
    let listing: serde_json::Value = listing.body_mut().read_json().map_err(http)?;
    let uri = listing["data"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["type"] == "oracle_cards"))
        .and_then(|x| x["jsonl_download_uri"].as_str())
        .ok_or_else(|| ScryfallError::Http("no oracle_cards bulk file listed".into()))?
        .to_string();
    if !uri.starts_with("https://data.scryfall.io/") {
        return Err(ScryfallError::Http(format!(
            "unexpected bulk address {uri}"
        )));
    }
    let mut resp = ureq::get(&uri)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(http)?;
    let bytes = resp
        .body_mut()
        .with_config()
        .limit(512 * 1024 * 1024)
        .read_to_vec()
        .map_err(http)?;
    let path = bulk_file();
    let io = |e: std::io::Error| ScryfallError::Io(format!("{}: {e}", path.display()));
    std::fs::create_dir_all(path.parent().expect("has parent")).map_err(io)?;
    let tmp = path.with_extension("part");
    std::fs::write(&tmp, &bytes).map_err(io)?;
    std::fs::rename(&tmp, &path).map_err(io)?;
    Ok(bytes.len() as u64)
}

impl ScryfallCard {
    /// A card that exists in the real game: not a token or other extra, not from a joke
    /// ("funny") or memorabilia set, not silver- or gold-bordered.
    pub fn is_real(&self) -> bool {
        !matches!(
            self.layout.as_str(),
            "token"
                | "double_faced_token"
                | "emblem"
                | "art_series"
                | "scheme"
                | "planar"
                | "vanguard"
                | "augment"
                | "host"
                | "reversible_card"
        ) && !matches!(
            self.set_type.as_deref(),
            Some("funny" | "memorabilia" | "token" | "minigame")
        ) && !matches!(self.border_color.as_deref(), Some("silver" | "gold"))
    }
}

/// Every card in the bulk file, by name (the front face's name for multi-faced cards is
/// also a key).
pub fn load_bulk() -> Result<ScryfallData, ScryfallError> {
    use std::io::{BufRead, BufReader};
    let path = bulk_file();
    let file = std::fs::File::open(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ScryfallError::Io(BULK_HINT.to_string())
        } else {
            ScryfallError::Io(format!("{}: {e}", path.display()))
        }
    })?;
    let mut data = ScryfallData::default();
    for line in BufReader::new(flate2::read::GzDecoder::new(file)).lines() {
        let line = line.map_err(|e| ScryfallError::Io(format!("{}: {e}", path.display())))?;
        let line = line.trim().trim_end_matches(',');
        if !line.starts_with('{') {
            continue;
        }
        let card: ScryfallCard = serde_json::from_str(line)
            .map_err(|e| ScryfallError::Io(format!("{}: {e}", path.display())))?;
        // Art cards and other extras share names with real cards: the real one wins.
        let keep_old = data
            .cards
            .get(&card.name)
            .is_some_and(|old| old.is_real() && !card.is_real());
        if !keep_old {
            data.cards.insert(card.name.clone(), card);
        }
    }
    Ok(data)
}

/// Make sure `names` are in the cache, fetching the missing ones from Scryfall. Returns the
/// full cache and how many cards were downloaded.
pub fn fetch(names: &[String]) -> Result<(ScryfallData, usize), ScryfallError> {
    let mut data = load_cache()?;
    let missing: Vec<&String> = names
        .iter()
        .filter(|n| data.get(n).is_none_or(ScryfallCard::is_stale))
        .collect();
    let mut fetched = 0;
    for (i, chunk) in missing.chunks(75).enumerate() {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(150));
        }
        let body = serde_json::json!({
            "identifiers": chunk.iter().map(|n| serde_json::json!({ "name": n })).collect::<Vec<_>>()
        });
        let mut resp = ureq::post("https://api.scryfall.com/cards/collection")
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .send_json(&body)
            .map_err(|e| ScryfallError::Http(e.to_string()))?;
        let col: Collection = resp
            .body_mut()
            .read_json()
            .map_err(|e| ScryfallError::Http(e.to_string()))?;
        if !col.not_found.is_empty() {
            let names = col
                .not_found
                .iter()
                .map(|v| v["name"].as_str().unwrap_or("?").to_string())
                .collect();
            return Err(ScryfallError::NotFound(names));
        }
        // Scryfall returns cards in request order; key them by the name we asked for.
        if col.data.len() != chunk.len() {
            return Err(ScryfallError::Http(format!(
                "asked for {} cards, got {}",
                chunk.len(),
                col.data.len()
            )));
        }
        for (asked, card) in chunk.iter().zip(col.data) {
            fetched += 1;
            data.cards.insert((*asked).clone(), card);
        }
    }
    if fetched > 0 {
        save_cache(&data)?;
    }
    Ok((data, fetched))
}

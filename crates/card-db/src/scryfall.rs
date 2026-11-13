//! Printed card data from Scryfall (spec §12), fetched at runtime into the user's cache
//! directory. **Nothing fetched here is ever written into the repository** (CLAUDE.md
//! invariant 13): the cache lives under `$APNAP_CACHE_DIR`, or in an `apnap-data` folder next
//! to the executable if there is one (a portable copy, D-095), or the platform cache
//! directory (`$XDG_CACHE_HOME/apnap`, i.e. `~/.cache/apnap` on Linux).
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
    /// The faces of a card with more than one (double-faced, split, adventurer, flip):
    /// each with its own name, cost, type line, text — and, for a two-sided card, its own
    /// picture (`image_uris` of the card itself is then absent).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub card_faces: Vec<ScryfallFace>,
}

/// One face of a [`ScryfallCard`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScryfallFace {
    #[serde(default)]
    pub name: String,
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
    pub image_uris: Option<ImageUris>,
    #[serde(default)]
    pub artist: Option<String>,
}

/// Scryfall layouts whose faces are separate cards' worth of characteristics.
const FACED: [&str; 6] = [
    "modal_dfc",
    "transform",
    "adventure",
    "split",
    "flip",
    "prepare",
];

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
    /// Cached before the image fields (or the faces) were stored: fetch it again.
    fn is_stale(&self) -> bool {
        let no_faces = self.card_faces.is_empty() && FACED.contains(&self.layout.as_str());
        let picture = self
            .image_uris
            .as_ref()
            .or_else(|| self.card_faces.first().and_then(|f| f.image_uris.as_ref()));
        self.id.is_none() || no_faces || picture.is_none_or(|u| u.large.is_none())
    }

    /// Face `i` of the card as a card of its own (face 0 of a one-faced card is the card).
    /// A face with a picture of its own (the back of a double-faced card) gets an id of its
    /// own, `<id>-<i>`, which names its cached image file.
    pub fn face(&self, i: usize) -> Option<ScryfallCard> {
        if self.card_faces.is_empty() {
            return (i == 0).then(|| self.clone());
        }
        let f = self.card_faces.get(i)?;
        let own_picture = f.image_uris.is_some();
        Some(ScryfallCard {
            name: f.name.clone(),
            mana_cost: f.mana_cost.clone(),
            type_line: f.type_line.clone(),
            oracle_text: f.oracle_text.clone(),
            power: f.power.clone(),
            toughness: f.toughness.clone(),
            colors: f.colors.clone().or_else(|| self.colors.clone()),
            image_uris: f.image_uris.clone().or_else(|| self.image_uris.clone()),
            artist: f.artist.clone().or_else(|| self.artist.clone()),
            id: match (&self.id, own_picture && i > 0) {
                (Some(id), true) => Some(format!("{id}-{i}")),
                (id, _) => id.clone(),
            },
            card_faces: Vec::new(),
            ..self.clone()
        })
    }
}

/// The on-disk cache: cards by name.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ScryfallData {
    pub cards: BTreeMap<String, ScryfallCard>,
    /// Every face name of a card with several faces: the key of its card and which face.
    #[serde(skip)]
    faces: BTreeMap<String, (String, usize)>,
}

impl ScryfallData {
    /// The card called `name`: by its full name ("Front // Back") or the name of its front
    /// face.
    pub fn get(&self, name: &str) -> Option<&ScryfallCard> {
        if let Some(c) = self.cards.get(name) {
            return Some(c);
        }
        match self.face_of(name) {
            Some((key, 0)) => self.cards.get(&key),
            _ => None,
        }
    }

    /// The face called `name` as a card of its own ([`ScryfallCard::face`]): what is shown
    /// for one face of a card.
    pub fn face(&self, name: &str) -> Option<ScryfallCard> {
        if let Some(c) = self.cards.get(name) {
            return c.face(0);
        }
        let (key, i) = self.face_of(name)?;
        self.cards.get(&key)?.face(i)
    }

    fn face_of(&self, name: &str) -> Option<(String, usize)> {
        if !self.faces.is_empty() {
            return self.faces.get(name).cloned();
        }
        self.cards.iter().find_map(|(key, c)| {
            c.card_faces
                .iter()
                .position(|f| f.name == name)
                .map(|i| (key.clone(), i))
        })
    }

    /// Index the faces' names (after loading or adding cards).
    pub fn index_faces(&mut self) {
        self.faces.clear();
        for (key, c) in &self.cards {
            for (i, f) in c.card_faces.iter().enumerate() {
                if f.name == *key {
                    continue;
                }
                // Art cards and other extras share face names with real cards: the real
                // one wins.
                let keep_old = self
                    .faces
                    .get(&f.name)
                    .is_some_and(|(old, _)| self.cards[old].is_real() || !c.is_real());
                if !keep_old {
                    self.faces.insert(f.name.clone(), (key.clone(), i));
                }
            }
        }
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

/// The folder that makes a copy of the program portable (D-095): if a directory of this
/// name sits next to the executable, it is the cache — card scripts, printed data, images
/// — and nothing is read from or written to the user's own cache directory.
pub const PORTABLE_DIR: &str = "apnap-data";

/// The portable data folder next to the running executable, if there is one.
pub fn portable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    portable_dir_beside(exe.parent()?)
}

fn portable_dir_beside(exe_dir: &std::path::Path) -> Option<PathBuf> {
    let dir = exe_dir.join(PORTABLE_DIR);
    dir.is_dir().then_some(dir)
}

/// The apnap cache directory (not created): `$APNAP_CACHE_DIR`, else the portable folder
/// next to the executable ([`PORTABLE_DIR`]), else the platform cache directory.
pub fn cache_dir() -> PathBuf {
    choose_cache_dir(
        std::env::var_os("APNAP_CACHE_DIR").map(PathBuf::from),
        portable_dir(),
        dirs::cache_dir(),
    )
}

fn choose_cache_dir(
    env: Option<PathBuf>,
    portable: Option<PathBuf>,
    platform: Option<PathBuf>,
) -> PathBuf {
    env.or(portable).unwrap_or_else(|| {
        platform
            .unwrap_or_else(|| PathBuf::from(".cache"))
            .join("apnap")
    })
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
            .map(|mut d: ScryfallData| {
                d.index_faces();
                d
            })
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

/// Every card in the bulk file, by name ("Front // Back" for a card with several faces;
/// [`ScryfallData::get`] also finds it by its front face's name).
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
    data.index_faces();
    Ok(data)
}

/// The EDHREC popularity rank of every Commander-legal card in the bulk file that has one.
pub fn load_bulk_ranks() -> Result<Vec<(String, u32)>, ScryfallError> {
    use std::io::{BufRead, BufReader};
    #[derive(Deserialize)]
    struct Legalities {
        #[serde(default)]
        commander: String,
    }
    #[derive(Deserialize)]
    struct Row {
        name: String,
        #[serde(default)]
        edhrec_rank: Option<u32>,
        #[serde(default)]
        legalities: Option<Legalities>,
    }
    let path = bulk_file();
    let file = std::fs::File::open(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ScryfallError::Io(BULK_HINT.to_string())
        } else {
            ScryfallError::Io(format!("{}: {e}", path.display()))
        }
    })?;
    let mut out = Vec::new();
    for line in BufReader::new(flate2::read::GzDecoder::new(file)).lines() {
        let line = line.map_err(|e| ScryfallError::Io(format!("{}: {e}", path.display())))?;
        let line = line.trim().trim_end_matches(',');
        if !line.starts_with('{') {
            continue;
        }
        let row: Row = serde_json::from_str(line)
            .map_err(|e| ScryfallError::Io(format!("{}: {e}", path.display())))?;
        if let (Some(rank), Some(l)) = (row.edhrec_rank, row.legalities)
            && l.commander == "legal"
        {
            out.push((row.name, rank));
        }
    }
    Ok(out)
}

/// Every token in the bulk file that has a picture of its own (double-faced token cards
/// don't), in the file's order.
pub fn load_bulk_tokens() -> Result<Vec<ScryfallCard>, ScryfallError> {
    use std::io::{BufRead, BufReader};
    let path = bulk_file();
    let file = std::fs::File::open(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            ScryfallError::Io(BULK_HINT.to_string())
        } else {
            ScryfallError::Io(format!("{}: {e}", path.display()))
        }
    })?;
    let mut out = Vec::new();
    for line in BufReader::new(flate2::read::GzDecoder::new(file)).lines() {
        let line = line.map_err(|e| ScryfallError::Io(format!("{}: {e}", path.display())))?;
        let line = line.trim().trim_end_matches(',');
        // Cheap test first: most of the file's 39,000 records aren't tokens.
        if !line.starts_with('{') || !line.contains("\"layout\":\"token\"") {
            continue;
        }
        let card: ScryfallCard = serde_json::from_str(line)
            .map_err(|e| ScryfallError::Io(format!("{}: {e}", path.display())))?;
        if card.layout == "token" && card.image_uris.is_some() {
            out.push(card);
        }
    }
    Ok(out)
}

/// Does the token card have rules text other than its keywords?
fn token_has_other_text(card: &ScryfallCard) -> bool {
    let text = card.oracle_text.as_deref().unwrap_or("");
    // Reminder text is in parentheses.
    let mut plain = String::new();
    let mut depth = 0;
    for c in text.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            c if depth == 0 => plain.push(c),
            _ => {}
        }
    }
    plain.lines().any(|line| {
        let line = line.trim();
        !line.is_empty()
            && !line.split(',').all(|part| {
                let part = part.trim().to_lowercase();
                card.keywords.iter().any(|k| part == k.to_lowercase())
            })
    })
}

/// The printed token card that shows token definition `def`: the same name, types,
/// subtypes, colors, power/toughness and keywords — and rules text exactly when the
/// definition has abilities. `None` when there is no such card, or when several with
/// different rules text fit (the picture would then show text the token may not have).
pub fn token_printing<'a>(
    tokens: &'a [ScryfallCard],
    def: &card_model::CardDef,
) -> Option<&'a ScryfallCard> {
    use card_model::Color;
    let name = def.name.strip_suffix(" Token").unwrap_or(&def.name);
    let sorted = |mut v: Vec<String>| {
        v.sort();
        v
    };
    let mut kinds: Vec<String> = def.card_types.iter().map(|t| format!("{t:?}")).collect();
    kinds.extend(def.supertypes.iter().map(|t| format!("{t:?}")));
    let kinds = sorted(kinds);
    let mut subtypes: Vec<String> = def.creature_types.clone();
    subtypes.extend(def.land_types.iter().map(|t| format!("{t:?}")));
    let subtypes = sorted(subtypes);
    let colors = sorted(
        def.printed_colors()
            .iter()
            .map(|c| {
                match c {
                    Color::White => "W",
                    Color::Blue => "U",
                    Color::Black => "B",
                    Color::Red => "R",
                    Color::Green => "G",
                }
                .to_string()
            })
            .collect(),
    );
    let keywords = sorted(
        def.keywords
            .iter()
            .map(|k| card_model::describe::keyword(*k).to_lowercase())
            .collect(),
    );
    let fits = |c: &&ScryfallCard| {
        let (left, right) = c.type_line.split_once(" — ").unwrap_or((&c.type_line, ""));
        c.name == name
            && sorted(
                left.split_whitespace()
                    .filter(|w| *w != "Token")
                    .map(String::from)
                    .collect(),
            ) == kinds
            && sorted(right.split_whitespace().map(String::from).collect()) == subtypes
            && c.power == def.power.map(|n| n.to_string())
            && c.toughness == def.toughness.map(|n| n.to_string())
            && sorted(c.colors.clone().unwrap_or_default()) == colors
            && sorted(c.keywords.iter().map(|k| k.to_lowercase()).collect()) == keywords
            && token_has_other_text(c) != def.abilities.is_empty()
    };
    let mut found = tokens.iter().filter(fits);
    let first = found.next()?;
    if !def.abilities.is_empty() && found.any(|c| c.oracle_text != first.oracle_text) {
        return None;
    }
    Some(first)
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
    data.index_faces();
    Ok((data, fetched))
}

#[cfg(test)]
mod tests {
    use super::*;

    // D-095: an explicit `$APNAP_CACHE_DIR` wins, then the portable folder next to the
    // executable, then the platform cache directory.
    #[test]
    fn the_cache_is_the_portable_folder_when_there_is_one() {
        let env = Some(PathBuf::from("/env"));
        let portable = Some(PathBuf::from("/exe/apnap-data"));
        let platform = Some(PathBuf::from("/home/u/.cache"));
        assert_eq!(
            choose_cache_dir(env, portable.clone(), platform.clone()),
            PathBuf::from("/env")
        );
        assert_eq!(
            choose_cache_dir(None, portable, platform.clone()),
            PathBuf::from("/exe/apnap-data")
        );
        assert_eq!(
            choose_cache_dir(None, None, platform),
            PathBuf::from("/home/u/.cache/apnap")
        );
        assert_eq!(
            choose_cache_dir(None, None, None),
            PathBuf::from(".cache/apnap")
        );
    }

    #[test]
    fn a_portable_folder_is_a_directory_next_to_the_executable() {
        let root = std::env::temp_dir().join(format!("apnap-portable-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(portable_dir_beside(&root), None);
        // A file of that name is not a data folder.
        std::fs::write(root.join(PORTABLE_DIR), b"").unwrap();
        assert_eq!(portable_dir_beside(&root), None);
        std::fs::remove_file(root.join(PORTABLE_DIR)).unwrap();
        std::fs::create_dir(root.join(PORTABLE_DIR)).unwrap();
        assert_eq!(portable_dir_beside(&root), Some(root.join(PORTABLE_DIR)));
        std::fs::remove_dir_all(&root).unwrap();
    }
    use card_model::{CardDef, CardType, Color, Keyword};

    fn printed(
        name: &str,
        types: &str,
        pt: Option<(&str, &str)>,
        colors: &[&str],
        text: &str,
        keywords: &[&str],
    ) -> ScryfallCard {
        serde_json::from_value(serde_json::json!({
            "name": name, "layout": "token", "type_line": types,
            "power": pt.map(|p| p.0), "toughness": pt.map(|p| p.1),
            "colors": colors, "oracle_text": text, "keywords": keywords,
            "id": format!("{name}-{types}-{text}"),
            "image_uris": { "normal": "https://cards.scryfall.io/x.jpg" },
        }))
        .unwrap()
    }

    fn token(name: &str, pt: (i32, i32), colors: &[Color], keywords: &[Keyword]) -> CardDef {
        CardDef {
            name: name.to_string(),
            mana_cost: None,
            supertypes: vec![],
            card_types: vec![CardType::Creature],
            land_types: vec![],
            creature_types: vec!["Sprite".to_string()],
            power: Some(pt.0),
            toughness: Some(pt.1),
            keywords: keywords.to_vec(),
            abilities: vec![],
            colors: Some(colors.to_vec()),
            token: true,
            key: None,
            granted: vec![],
            ..CardDef::default()
        }
    }

    // A token is pictured by the printed token card with the same characteristics, and by
    // none when the only candidates differ from it.
    #[test]
    fn a_token_is_matched_to_the_printed_token_with_the_same_characteristics() {
        let tokens = vec![
            printed(
                "Test Sprite",
                "Token Creature — Sprite",
                Some(("1", "1")),
                &["G"],
                "",
                &[],
            ),
            printed(
                "Test Sprite",
                "Token Creature — Sprite",
                Some(("1", "1")),
                &["U"],
                "Flying",
                &["Flying"],
            ),
            printed(
                "Test Sprite",
                "Token Creature — Sprite",
                Some(("1", "1")),
                &["U"],
                "Flying\nWhen this creature dies, draw a card.",
                &["Flying"],
            ),
            printed(
                "Test Sprite",
                "Token Artifact Creature — Sprite",
                Some(("2", "2")),
                &[],
                "Flying (This creature can't be blocked except by creatures with flying or reach.)",
                &["Flying"],
            ),
        ];
        let find = |def: &CardDef| {
            token_printing(&tokens, def).map(|c| tokens.iter().position(|t| t == c).unwrap())
        };
        // Name ("Sprite Token" is the token named Sprite), colour, keywords.
        assert_eq!(
            find(&token("Test Sprite Token", (1, 1), &[Color::Green], &[])),
            Some(0)
        );
        assert_eq!(
            find(&token(
                "Test Sprite Token",
                (1, 1),
                &[Color::Blue],
                &[Keyword::Flying]
            )),
            Some(1)
        );
        // Reminder text isn't rules text; the artifact type is part of the match.
        let mut artifact = token("Test Sprite Token", (2, 2), &[], &[Keyword::Flying]);
        assert_eq!(find(&artifact), None);
        artifact.card_types.insert(0, CardType::Artifact);
        assert_eq!(find(&artifact), Some(3));
        // No printed card: a green flyer, a 3/3, another name.
        assert_eq!(
            find(&token(
                "Test Sprite Token",
                (1, 1),
                &[Color::Green],
                &[Keyword::Flying]
            )),
            None
        );
        assert_eq!(
            find(&token("Test Sprite Token", (3, 3), &[Color::Green], &[])),
            None
        );
        assert_eq!(
            find(&token("Test Pixie Token", (1, 1), &[Color::Green], &[])),
            None
        );
        // The blue flyer without abilities is never shown as the one that draws a card.
        assert!(token_has_other_text(&tokens[2]) && !token_has_other_text(&tokens[1]));
    }

    // A two-sided card: found by its full name or its front face's name; each face is a
    // card of its own with its own picture (the back cached under `<id>-1`). Art cards that
    // share a face name never win over the real card.
    #[test]
    fn faces_of_a_two_sided_card() {
        let face = |name: &str, side: &str| {
            serde_json::json!({
                "name": name, "mana_cost": "", "type_line": "Land",
                "oracle_text": "{T}: Add {U}.",
                "image_uris": {
                    "normal": format!("https://cards.scryfall.io/normal/{side}/a.jpg"),
                    "large": format!("https://cards.scryfall.io/large/{side}/a.jpg"),
                },
            })
        };
        let real: ScryfallCard = serde_json::from_value(serde_json::json!({
            "name": "Test Dawn // Test Dusk", "layout": "modal_dfc", "id": "abc-123",
            "type_line": "Land // Land",
            "card_faces": [face("Test Dawn", "front"), face("Test Dusk", "back")],
        }))
        .unwrap();
        let art: ScryfallCard = serde_json::from_value(serde_json::json!({
            "name": "Test Dawn // Test Dawn", "layout": "art_series", "id": "zzz",
            "set_type": "memorabilia",
            "card_faces": [face("Test Dawn", "front"), face("Test Dawn", "back")],
        }))
        .unwrap();
        assert!(!real.is_stale());
        let mut data = ScryfallData::default();
        data.cards.insert(real.name.clone(), real.clone());
        data.cards.insert(art.name.clone(), art);
        data.index_faces();
        assert_eq!(
            data.get("Test Dawn").unwrap().name,
            "Test Dawn // Test Dusk"
        );
        assert!(
            data.get("Test Dusk").is_none(),
            "a back face isn't the card"
        );
        let front = data.face("Test Dawn").unwrap();
        let back = data.face("Test Dusk").unwrap();
        assert_eq!(
            (front.id.as_deref(), back.id.as_deref()),
            (Some("abc-123"), Some("abc-123-1"))
        );
        assert!(
            crate::images::image_url(&back, crate::images::ImageKind::Normal)
                .unwrap()
                .contains("/back/")
        );
        // The whole card's picture is its front face's.
        assert!(
            crate::images::image_url(&real, crate::images::ImageKind::Normal)
                .unwrap()
                .contains("/front/")
        );
        assert_ne!(
            crate::images::image_path(&front, crate::images::ImageKind::Normal),
            crate::images::image_path(&back, crate::images::ImageKind::Normal)
        );
        // A card cached without its faces is fetched again.
        let mut old = real;
        old.card_faces.clear();
        assert!(old.is_stale());
    }
}

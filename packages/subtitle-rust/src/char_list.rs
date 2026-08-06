//! Load and prepare the character list for recognition.
//!
//! `ppocr_keys.json` (same file used by PaddleOCR: the first entry is an
//! empty string (a placeholder; we treat it as blank). We re-encode for our
//! decoder: index 0 = blank, indices 1..N = actual chars, plus a final space.

use std::fs;
use std::path::Path;

pub fn load_char_list<P: AsRef<Path>>(path: P) -> std::result::Result<Vec<String>, String> {
    let raw: Vec<String> =
        serde_json::from_reader(fs::File::open(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(raw.len() + 1);
    out.push(String::new()); // blank
    for s in raw.iter().skip(1) {
        out.push(s.clone());
    }
    out.push(" ".to_string()); // space
    Ok(out)
}

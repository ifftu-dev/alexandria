//! A minimal reader for Whisper's `tokenizer.json`: GPT-2 byte-level BPE.
//!
//! Whisper's tokenizer is a plain byte-level BPE with no normaliser, the
//! GPT-2 pre-tokeniser regex and special tokens listed in `added_tokens`.
//! That is small enough to implement here, which keeps the Hugging Face
//! `tokenizers` crate (and its C regex engine) out of the app.
//!
//! Merging follows OpenAI's own tokenizer (tiktoken), not the file's merge
//! list: repeatedly join the adjacent pair whose concatenation has the lowest
//! vocabulary id. The two agree on GPT-2's vocabulary, but the multilingual
//! Whisper `tokenizer.json` omits merges (`Ġ t` among them), so following the
//! list would split " teacher" into tokens the model never produces.

use std::collections::HashMap;
use std::ops::Range;

use fancy_regex::Regex;
use serde::Deserialize;

/// GPT-2's pre-tokeniser: contractions, letter runs, digit runs and
/// punctuation runs, each taking at most one leading space.
const PRETOKENIZE: &str =
    r"'s|'t|'re|'ve|'m|'ll|'d| ?\p{L}+| ?\p{N}+| ?[^\s\p{L}\p{N}]+|\s+(?!\S)|\s+";

#[derive(Deserialize)]
struct TokenizerFile {
    model: BpeModel,
    #[serde(default)]
    added_tokens: Vec<AddedToken>,
}

#[derive(Deserialize)]
struct BpeModel {
    vocab: HashMap<String, u32>,
}

#[derive(Deserialize)]
struct AddedToken {
    id: u32,
    content: String,
}

pub struct Tokenizer {
    vocab: HashMap<String, u32>,
    id_to_token: HashMap<u32, String>,
    special: HashMap<String, u32>,
    special_ids: std::collections::HashSet<u32>,
    byte_to_char: [char; 256],
    char_to_byte: HashMap<char, u8>,
    pretokenizer: Regex,
}

/// GPT-2's reversible byte → printable-character map.
fn byte_chars() -> [char; 256] {
    let mut printable: Vec<u32> = (b'!' as u32..=b'~' as u32)
        .chain(0xA1..=0xAC)
        .chain(0xAE..=0xFF)
        .collect();
    let mut chars: Vec<u32> = printable.clone();
    let mut extra = 0;
    for b in 0..256u32 {
        if !printable.contains(&b) {
            printable.push(b);
            chars.push(256 + extra);
            extra += 1;
        }
    }
    let mut table = ['\0'; 256];
    for (b, c) in printable.into_iter().zip(chars) {
        table[b as usize] = char::from_u32(c).unwrap_or('\0');
    }
    table
}

impl Tokenizer {
    pub fn from_json(json: &[u8]) -> anyhow::Result<Self> {
        let file: TokenizerFile = serde_json::from_slice(json)?;
        let mut id_to_token: HashMap<u32, String> = file
            .model
            .vocab
            .iter()
            .map(|(token, &id)| (id, token.clone()))
            .collect();
        let mut special = HashMap::new();
        for token in file.added_tokens {
            id_to_token.insert(token.id, token.content.clone());
            special.insert(token.content, token.id);
        }
        let special_ids = special.values().copied().collect();
        let byte_to_char = byte_chars();
        let char_to_byte = byte_to_char
            .iter()
            .enumerate()
            .map(|(b, &c)| (c, b as u8))
            .collect();
        Ok(Self {
            vocab: file.model.vocab,
            id_to_token,
            special,
            special_ids,
            byte_to_char,
            char_to_byte,
            pretokenizer: Regex::new(PRETOKENIZE)?,
        })
    }

    /// The id of a special token such as `<|endoftext|>`.
    pub fn special(&self, token: &str) -> Option<u32> {
        self.special.get(token).copied()
    }

    /// Pre-tokenised pieces of `text`, in order (" doctor", ".").
    pub fn pieces<'t>(&self, text: &'t str) -> anyhow::Result<Vec<&'t str>> {
        Ok(self
            .pieces_at(text)?
            .into_iter()
            .map(|(_, piece)| piece)
            .collect())
    }

    fn pieces_at<'t>(&self, text: &'t str) -> anyhow::Result<Vec<(usize, &'t str)>> {
        let mut pieces = Vec::new();
        for found in self.pretokenizer.find_iter(text) {
            let found = found?;
            pieces.push((found.start(), found.as_str()));
        }
        Ok(pieces)
    }

    /// BPE ids for `text`, each with the byte range of `text` it covers.
    pub fn encode_with_spans(&self, text: &str) -> anyhow::Result<Vec<(u32, Range<usize>)>> {
        let mut out = Vec::new();
        for (start, piece) in self.pieces_at(text)? {
            let mut at = start;
            for id in self.encode_piece(piece)? {
                // Each character of a byte-level token stands for one byte.
                let len = self.id_to_token.get(&id).map_or(0, |t| t.chars().count());
                out.push((id, at..at + len));
                at += len;
            }
        }
        Ok(out)
    }

    /// BPE ids for one pre-tokenised piece.
    pub fn encode_piece(&self, piece: &str) -> anyhow::Result<Vec<u32>> {
        let mut symbols: Vec<String> = piece
            .bytes()
            .map(|b| self.byte_to_char[b as usize].to_string())
            .collect();
        let mut joined = String::new();
        while symbols.len() > 1 {
            // The leftmost adjacent pair whose concatenation has the lowest id.
            let mut best: Option<(u32, usize)> = None;
            for (i, pair) in symbols.windows(2).enumerate() {
                joined.clear();
                joined.push_str(&pair[0]);
                joined.push_str(&pair[1]);
                if let Some(&id) = self.vocab.get(&joined) {
                    if best.is_none_or(|(best_id, _)| id < best_id) {
                        best = Some((id, i));
                    }
                }
            }
            let Some((_, at)) = best else { break };
            let right = symbols.remove(at + 1);
            symbols[at].push_str(&right);
        }
        symbols
            .iter()
            .map(|symbol| {
                self.vocab
                    .get(symbol)
                    .copied()
                    .ok_or_else(|| anyhow::anyhow!("token {symbol:?} is not in the vocabulary"))
            })
            .collect()
    }

    /// Text for `ids`, skipping special tokens.
    pub fn decode(&self, ids: &[u32]) -> String {
        let mut bytes = Vec::new();
        for id in ids {
            if self.special_ids.contains(id) {
                continue;
            }
            if let Some(token) = self.id_to_token.get(id) {
                bytes.extend(token.chars().filter_map(|c| self.char_to_byte.get(&c)));
            }
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A toy byte-level BPE over a handful of tokens, in the real file's shape.
    /// Merge order is the vocabulary order.
    pub(crate) fn toy() -> Tokenizer {
        let json = serde_json::json!({
            "added_tokens": [
                {"id": 100, "content": "<|endoftext|>"},
                {"id": 101, "content": "<|startoftranscript|>"},
                {"id": 102, "content": "<|notimestamps|>"}
            ],
            "model": {
                "type": "BPE",
                "vocab": {
                    "Ġ": 0, "a": 1, "b": 2, "c": 3, "d": 4, ".": 5, "S": 6, "h": 7, "e": 8,
                    "i": 9, "s": 10, "Ġa": 11, "Ġb": 12, "ab": 13, "Ġab": 14, "Ġc": 15,
                    "Ġi": 16, "Ġis": 17, "Sh": 18, "She": 19, "ĠShe": 20, "ĠS": 21,
                    "ĠSh": 22, "Ã": 23, "©": 24, "Ã©": 25
                },
                // Ignored: merging goes by vocabulary id, so an incomplete
                // merge list (as in the multilingual file) cannot matter.
                "merges": ["Ġ a"]
            }
        });
        Tokenizer::from_json(json.to_string().as_bytes()).expect("toy tokenizer")
    }

    #[test]
    fn byte_map_round_trips_every_byte() {
        let table = byte_chars();
        let unique: std::collections::HashSet<_> = table.iter().collect();
        assert_eq!(unique.len(), 256);
        assert_eq!(table[b' ' as usize], 'Ġ');
        assert_eq!(table[b'a' as usize], 'a');
    }

    #[test]
    fn pretokenizes_like_gpt2() {
        let tokenizer = toy();
        assert_eq!(
            tokenizer.pieces(" She is a doctor.").unwrap(),
            [" She", " is", " a", " doctor", "."]
        );
        assert_eq!(tokenizer.pieces(" don't").unwrap(), [" don", "'t"]);
    }

    #[test]
    fn merges_the_lowest_id_pair_first() {
        let tokenizer = toy();
        assert_eq!(tokenizer.encode_piece(" ab").unwrap(), [14]);
        assert_eq!(tokenizer.encode_piece(" She").unwrap(), [20]);
        assert_eq!(tokenizer.encode_piece(" is").unwrap(), [17]);
        assert_eq!(tokenizer.encode_piece("abc").unwrap(), [13, 3]);
        // Non-ASCII goes through the byte map: "é" is bytes C3 A9.
        assert_eq!(tokenizer.encode_piece("é").unwrap(), [25]);
        assert!(tokenizer.encode_piece("z").is_err());
    }

    #[test]
    fn spans_cover_the_bytes_each_token_encodes() {
        let tokenizer = toy();
        let spans = tokenizer.encode_with_spans(" She is abé.").unwrap();
        assert_eq!(
            spans,
            [
                (20, 0..4),
                (17, 4..7),
                (14, 7..10),
                (25, 10..12),
                (5, 12..13)
            ]
        );
    }

    #[test]
    fn decodes_and_skips_special_tokens() {
        let tokenizer = toy();
        assert_eq!(tokenizer.decode(&[101, 20, 17, 102, 25, 100]), " She isé");
        assert_eq!(tokenizer.special("<|notimestamps|>"), Some(102));
        assert_eq!(tokenizer.special("<|hi|>"), None);
    }
}

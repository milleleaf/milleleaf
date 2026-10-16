//! How an import recognised each node in an edited file, and how sure it is.
//!
//! *Certain*: the file carries the node's identity (a content-control tag, a defined name, a
//! shape or slide name). *Best guess*: the identity was lost and recovered from other evidence
//! (a slide recognised by the frames on it, a live field found again by the text it shows).
//! Best guesses are listed in the import report and flagged in the edit layer with
//! `ox::match`, so a reviewer can confirm them (and clear the flag with `"ox::match": null`).

use serde::Serialize;
use serde_json::json;

use crate::model::Layer;

/// Attribute that flags a node in an edit layer whose match was a best guess.
pub const MATCH_ATTR: &str = "ox::match";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    Certain,
    BestGuess,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Match {
    /// The node recognised (for a field found by its text: the frame or paragraph holding it).
    pub path: String,
    pub confidence: Confidence,
    /// The evidence, in words: "content control tag", "4 of 4 named frames" …
    pub by: String,
}

impl Match {
    pub fn certain(path: impl Into<String>, by: impl Into<String>) -> Self {
        Match {
            path: path.into(),
            confidence: Confidence::Certain,
            by: by.into(),
        }
    }

    pub fn best_guess(path: impl Into<String>, by: impl Into<String>) -> Self {
        Match {
            path: path.into(),
            confidence: Confidence::BestGuess,
            by: by.into(),
        }
    }
}

/// The best guesses among `matches`.
pub fn best_guesses(matches: &[Match]) -> Vec<&Match> {
    matches
        .iter()
        .filter(|m| m.confidence == Confidence::BestGuess)
        .collect()
}

/// Adds `ox::match` to every node of `layer` whose match was a best guess. Nodes the layer
/// doesn't touch need no flag: nothing in it depends on them.
pub fn flag_best_guesses(layer: &mut Layer, matches: &[Match]) {
    for m in best_guesses(matches) {
        for n in layer.data.iter_mut().filter(|n| n.path == m.path) {
            let by = match n.attributes.get(MATCH_ATTR).and_then(|v| v.get("by")) {
                Some(prev) => format!("{}; {}", prev.as_str().unwrap_or(""), m.by),
                None => m.by.clone(),
            };
            n.attributes.insert(
                MATCH_ATTR.into(),
                json!({ "confidence": "best-guess", "by": by }),
            );
        }
    }
}

/// Names that occur more than once, with their counts: a duplicated slide, a pasted paragraph or a
/// copied sheet carries the same `ox:` identity as its original, so neither copy is certain.
pub fn duplicates<'a>(names: impl IntoIterator<Item = &'a str>) -> std::collections::BTreeMap<String, usize> {
    let mut n: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for name in names {
        *n.entry(name.to_string()).or_default() += 1;
    }
    n.retain(|_, c| *c > 1);
    n
}

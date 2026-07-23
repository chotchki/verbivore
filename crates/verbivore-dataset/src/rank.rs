//! Intent phrase -> element ranking, the text half of grounding. Classical
//! matching for v1 (token overlap + role synonyms): the a11y tree hands us
//! accessible names, so string similarity carries most of the weight and a
//! learned ranker has to EARN its slot by beating this (same bar the effect
//! model faces vs SSIM). Container scoping happens before this — callers crop
//! the candidate set, we only order it.

use crate::ElementLabel;

/// Candidate index + score, sorted best-first. Scores are comparable only
/// within one call — they rank, they don't calibrate.
#[derive(Debug, Clone, PartialEq)]
pub struct Ranked {
    pub index: usize,
    pub score: f32,
}

/// Words a phrase uses to mean a role, mapped onto the canonical role list.
const ROLE_SYNONYMS: &[(&str, &[&str])] = &[
    ("button", &["button", "btn"]),
    ("link", &["link", "anchor", "hyperlink"]),
    ("textbox", &["textbox", "field", "input", "textfield"]),
    ("searchbox", &["searchbox", "search"]),
    ("checkbox", &["checkbox", "check"]),
    ("radio", &["radio"]),
    ("combobox", &["combobox", "dropdown", "select", "combo", "picker"]),
    ("listbox", &["listbox"]),
    ("option", &["option"]),
    ("menuitem", &["menuitem", "menu"]),
    ("tab", &["tab"]),
    ("switch", &["switch", "toggle"]),
    ("slider", &["slider"]),
    ("spinbutton", &["spinbutton", "spinner", "stepper"]),
];

const STOPWORDS: &[&str] = &["the", "a", "an", "in", "on", "of", "to", "for", "with"];

const ROLE_MATCH_BONUS: f32 = 0.5;
const ROLE_MISMATCH_PENALTY: f32 = 0.25;
const EXACT_NAME_BONUS: f32 = 1.0;
const SUBSTRING_BONUS: f32 = 0.5;

/// Orders candidates by how well they answer the phrase. Ties keep input order,
/// so callers can prefer earlier (e.g. more prominent) elements for free.
pub fn rank(intent: &str, candidates: &[ElementLabel]) -> Vec<Ranked> {
    let phrase_tokens = tokenize(intent);
    let wanted_role = infer_role(&phrase_tokens);
    // Role words describe WHAT the element is, not what it's named.
    let name_tokens: Vec<&str> = phrase_tokens
        .iter()
        .map(String::as_str)
        .filter(|t| !is_role_word(t))
        .collect();

    let mut ranked: Vec<Ranked> = candidates
        .iter()
        .enumerate()
        .map(|(index, c)| {
            let mut score = 0.0f32;
            if let Some(wanted) = wanted_role {
                if c.role == wanted {
                    score += ROLE_MATCH_BONUS;
                } else {
                    score -= ROLE_MISMATCH_PENALTY;
                }
            }
            if let Some(name) = &c.name {
                score += name_score(&name_tokens, name);
            }
            Ranked { index, score }
        })
        .collect();
    ranked.sort_by(|a, b| b.score.total_cmp(&a.score));
    ranked
}

fn name_score(query: &[&str], name: &str) -> f32 {
    if query.is_empty() {
        return 0.0;
    }
    let name_tokens = tokenize(name);
    let name_lower = name.to_lowercase();
    let query_joined = query.join(" ");

    let overlap = query
        .iter()
        .filter(|q| name_tokens.iter().any(|n| n == *q))
        .count();
    let union = query.len() + name_tokens.len() - overlap;
    let jaccard = if union == 0 {
        0.0
    } else {
        overlap as f32 / union as f32
    };

    let mut score = jaccard;
    if name_lower == query_joined {
        score += EXACT_NAME_BONUS;
    } else if name_lower.contains(&query_joined) {
        score += SUBSTRING_BONUS;
    }
    score
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty() && !STOPWORDS.contains(t))
        .map(str::to_owned)
        .collect()
}

fn infer_role(tokens: &[String]) -> Option<&'static str> {
    for (role, synonyms) in ROLE_SYNONYMS {
        if tokens.iter().any(|t| synonyms.contains(&t.as_str())) {
            return Some(role);
        }
    }
    None
}

fn is_role_word(token: &str) -> bool {
    ROLE_SYNONYMS
        .iter()
        .any(|(_, synonyms)| synonyms.contains(&token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Bbox;

    fn candidate(role: &str, name: &str) -> ElementLabel {
        ElementLabel {
            bbox: Bbox {
                x: 0.0,
                y: 0.0,
                w: 10.0,
                h: 10.0,
            },
            role: role.into(),
            name: (!name.is_empty()).then(|| name.to_owned()),
        }
    }

    fn top<'a>(intent: &str, cands: &'a [ElementLabel]) -> &'a ElementLabel {
        &cands[rank(intent, cands)[0].index]
    }

    #[test]
    fn role_word_disambiguates_same_name() {
        let cands = [
            candidate("link", "Submit feedback"),
            candidate("button", "Submit order"),
            candidate("button", "Cancel"),
        ];
        let best = top("the submit button", &cands);
        assert_eq!((best.role.as_str(), best.name.as_deref()), ("button", Some("Submit order")));
    }

    #[test]
    fn exact_name_beats_partial() {
        let cands = [
            candidate("button", "Save as draft"),
            candidate("button", "Save"),
        ];
        assert_eq!(top("save", &cands).name.as_deref(), Some("Save"));
    }

    #[test]
    fn role_synonyms_reach_the_canonical_role() {
        let cands = [
            candidate("textbox", "Filter results"),
            candidate("combobox", "Filter results"),
        ];
        assert_eq!(top("the filter dropdown", &cands).role, "combobox");
        assert_eq!(top("filter field", &cands).role, "textbox");
    }

    #[test]
    fn search_maps_to_searchbox() {
        let cands = [
            candidate("textbox", "Username"),
            candidate("searchbox", ""),
        ];
        assert_eq!(top("search", &cands).role, "searchbox");
    }

    #[test]
    fn no_signal_keeps_input_order() {
        let cands = [candidate("button", "Alpha"), candidate("button", "Beta")];
        let ranked = rank("zzz nothing matches", &cands);
        assert_eq!(ranked[0].index, 0, "stable order on all-equal scores");
        assert_eq!(ranked[0].score, ranked[1].score);
    }
}

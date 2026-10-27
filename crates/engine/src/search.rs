//! SuperSearch: the `search` text of `list_families` as tokens that AND together
//! (`tag:serif foundry:"Monotype Imaging" weight:>500 italic char:ß`). Everything that is not a
//! known token stays a plain word; the plain words are matched as one phrase, as before tokens.

use crate::{Category, License};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Term {
    Tag(String),
    Foundry(String),
    Designer(String),
    Category(Category),
    License(License),
    Weight(f32, f32),
    WeightAbove(f32),
    WeightBelow(f32),
    Italic,
    Variable,
    Active,
    Inactive,
    Pinned,
    System,
    /// Glyph search, case kept.
    Chars(String),
}

#[derive(Debug, Default, PartialEq)]
pub(crate) struct Search {
    /// Plain words, lowercase, joined by single spaces.
    pub text: Option<String>,
    pub terms: Vec<Term>,
}

/// Splits on whitespace outside double quotes; the quotes themselves are dropped.
fn split(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in input.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn weight(v: &str) -> Option<Term> {
    let n = |s: &str| s.trim().parse::<f32>().ok();
    Some(if let Some(x) = v.strip_prefix('>') {
        Term::WeightAbove(n(x)?)
    } else if let Some(x) = v.strip_prefix('<') {
        Term::WeightBelow(n(x)?)
    } else if let Some((a, b)) = v.split_once('-') {
        Term::Weight(n(a)?, n(b)?)
    } else {
        let w = n(v)?;
        Term::Weight(w, w)
    })
}

fn term(tok: &str) -> Option<Term> {
    let lower = tok.to_lowercase();
    match lower.as_str() {
        "italic" => return Some(Term::Italic),
        "variable" => return Some(Term::Variable),
        "active" => return Some(Term::Active),
        "inactive" => return Some(Term::Inactive),
        "pinned" => return Some(Term::Pinned),
        "system" => return Some(Term::System),
        _ => {}
    }
    let (key, value) = tok.split_once(':')?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let lv = value.to_lowercase();
    Some(match key.to_lowercase().as_str() {
        "tag" => Term::Tag(lv),
        "foundry" => Term::Foundry(lv),
        "designer" => Term::Designer(lv),
        "category" => Term::Category(Category::parse(&lv)?),
        "license" => match lv.as_str() {
            "licensed" | "personal" | "client" | "unknown" => Term::License(License::parse(&lv)),
            _ => return None,
        },
        "weight" => weight(&lv)?,
        "char" | "glyph" => Term::Chars(value.into()),
        _ => return None,
    })
}

pub(crate) fn parse(input: &str) -> Search {
    let mut words = Vec::new();
    let mut terms = Vec::new();
    for tok in split(input) {
        match term(&tok) {
            Some(t) => terms.push(t),
            None => words.push(tok.to_lowercase()),
        }
    }
    Search { text: (!words.is_empty()).then(|| words.join(" ")), terms }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_words_stay_one_phrase() {
        assert_eq!(parse("  Times New  "), Search { text: Some("times new".into()), terms: vec![] });
        assert_eq!(parse(""), Search::default());
        // Unknown keys and bad values are plain words.
        assert_eq!(parse("foo:bar category:fancy weight:heavy tag:").text.as_deref(), Some("foo:bar category:fancy weight:heavy tag:"));
    }

    #[test]
    fn tokens() {
        let s = parse(r#"Arial Tag:Client foundry:"Monotype Imaging" designer:x category:serif license:client italic VARIABLE active inactive pinned system"#);
        assert_eq!(s.text.as_deref(), Some("arial"));
        assert_eq!(
            s.terms,
            vec![
                Term::Tag("client".into()),
                Term::Foundry("monotype imaging".into()),
                Term::Designer("x".into()),
                Term::Category(Category::Serif),
                Term::License(License::Client),
                Term::Italic,
                Term::Variable,
                Term::Active,
                Term::Inactive,
                Term::Pinned,
                Term::System,
            ]
        );
        let s = parse("weight:400 weight:>500 weight:<300 weight:300-700 char:ẞ glyph:中文");
        assert_eq!(
            s.terms,
            vec![
                Term::Weight(400.0, 400.0),
                Term::WeightAbove(500.0),
                Term::WeightBelow(300.0),
                Term::Weight(300.0, 700.0),
                Term::Chars("ẞ".into()),
                Term::Chars("中文".into()),
            ]
        );
        assert_eq!(s.text, None);
    }
}

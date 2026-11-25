//! Languages a font can fully cover: for each, the characters a typographer calls required.
//! `ranges` are inclusive codepoint ranges, `extra` the single letters beyond them.

use crate::glyphs::covers;

pub struct Language {
    /// Lowercase English name, the token of `lang:<id>`.
    pub id: &'static str,
    pub name: &'static str,
    ranges: &'static [(u32, u32)],
    extra: &'static str,
}

impl Language {
    pub fn chars(&self) -> impl Iterator<Item = u32> + '_ {
        self.ranges.iter().flat_map(|&(a, b)| a..=b).chain(self.extra.chars().map(u32::from))
    }

    /// Whether `coverage` (see `glyphs::coverage`) maps every required character.
    pub fn covered_by(&self, coverage: &[[u32; 2]]) -> bool {
        self.chars().all(|c| covers(coverage, c))
    }
}

const LATIN: &[(u32, u32)] = &[(0x41, 0x5A), (0x61, 0x7A)];

// ponytail: Japanese lists kana plus, like Chinese and Korean, a small common-character
// sample, not the thousands a full school or newspaper set needs; a font passes if it has the sample.
// Raise it with a real frequency list (e.g. Joyo kanji, GB 2312 level 1) if false positives show up.
pub const LANGUAGES: &[Language] = &[
    Language { id: "english", name: "English", ranges: LATIN, extra: "" },
    Language { id: "indonesian", name: "Indonesian", ranges: LATIN, extra: "" },
    Language { id: "malay", name: "Malay", ranges: LATIN, extra: "" },
    Language {
        id: "vietnamese",
        name: "Vietnamese",
        ranges: LATIN,
        extra: "ÀÁẢÃẠĂẰẮẲẴẶÂẦẤẨẪẬĐÈÉẺẼẸÊỀẾỂỄỆÌÍỈĨỊÒÓỎÕỌÔỒỐỔỖỘƠỜỚỞỠỢÙÚỦŨỤƯỪỨỬỮỰỲÝỶỸỴ\
                àáảãạăằắẳẵặâầấẩẫậđèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵ",
    },
    Language { id: "french", name: "French", ranges: LATIN, extra: "ÀÂÆÇÉÈÊËÎÏÔŒÙÛÜŸàâæçéèêëîïôœùûüÿ" },
    Language { id: "german", name: "German", ranges: LATIN, extra: "ÄÖÜäöüß" },
    Language { id: "spanish", name: "Spanish", ranges: LATIN, extra: "ÁÉÍÓÚÜÑáéíóúüñ¿¡" },
    Language { id: "portuguese", name: "Portuguese", ranges: LATIN, extra: "ÁÀÂÃÇÉÊÍÓÔÕÚáàâãçéêíóôõú" },
    Language { id: "polish", name: "Polish", ranges: LATIN, extra: "ĄĆĘŁŃÓŚŹŻąćęłńóśźż" },
    Language { id: "turkish", name: "Turkish", ranges: LATIN, extra: "ÇĞİÖŞÜçğıöşü" },
    Language { id: "russian", name: "Russian", ranges: &[(0x410, 0x44F)], extra: "Ёё" },
    Language { id: "greek", name: "Greek", ranges: &[(0x391, 0x3A1), (0x3A3, 0x3A9), (0x3B1, 0x3C9)], extra: "ΆΈΉΊΌΎΏάέήίόύώ" },
    // Arabic letters hamza..ghain and fa..yeh (no diacritics, no Persian or Urdu additions).
    Language { id: "arabic", name: "Arabic", ranges: &[(0x621, 0x63A), (0x641, 0x64A)], extra: "" },
    Language { id: "hebrew", name: "Hebrew", ranges: &[(0x5D0, 0x5EA)], extra: "" },
    // Consonants, vowels and tone marks; Thai digits left out.
    Language { id: "thai", name: "Thai", ranges: &[(0xE01, 0xE3A), (0xE40, 0xE4E)], extra: "" },
    // Vowels, consonants, vowel signs and virama; digits and the extended consonants left out.
    Language { id: "hindi", name: "Hindi", ranges: &[(0x905, 0x939), (0x93E, 0x94D)], extra: "" },
    Language { id: "japanese", name: "Japanese", ranges: &[(0x3041, 0x3096), (0x30A1, 0x30FA)], extra: "日本語人大中小年月水火木金土" },
    Language { id: "korean", name: "Korean", ranges: &[], extra: "가나다라마바사아자차카타파하한국어" },
    Language { id: "chinese", name: "Chinese", ranges: &[], extra: "的一是不了人我在有他这中大来上国个到说们为子和你地出道也时年得就那" },
];

/// The language with this id (case-insensitive).
pub fn language(id: &str) -> Option<&'static Language> {
    LANGUAGES.iter().find(|l| l.id.eq_ignore_ascii_case(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cov(chars: impl Iterator<Item = u32>) -> Vec<[u32; 2]> {
        let mut v: Vec<u32> = chars.collect();
        v.sort_unstable();
        v.dedup();
        v.into_iter().map(|c| [c, c]).collect()
    }

    #[test]
    fn ids_are_unique_and_sets_are_assigned_characters() {
        for l in LANGUAGES {
            assert_eq!(LANGUAGES.iter().filter(|x| x.id == l.id).count(), 1, "{}", l.id);
            assert!(l.chars().count() > 0);
            assert!(l.chars().all(|c| char::from_u32(c).is_some_and(|c| !c.is_control())), "{}", l.id);
        }
    }

    #[test]
    fn set_sizes() {
        let n = |id| language(id).unwrap().chars().count();
        assert_eq!(n("indonesian"), 52, "A-Z a-z only");
        assert_eq!(n("malay"), 52);
        assert_eq!(n("vietnamese"), 52 + 134, "134 letters beyond A-Z in both cases");
        assert_eq!(n("polish"), 52 + 18, "nine special letters, two cases");
        assert_eq!(n("turkish"), 52 + 12);
        assert_eq!(n("russian"), 66);
        assert_eq!(n("hebrew"), 27);
    }

    #[test]
    fn covered_by_needs_every_character() {
        let tr = language("turkish").unwrap();
        let all = cov(tr.chars());
        assert!(tr.covered_by(&all));
        let without_dotless_i = cov(tr.chars().filter(|&c| c != 'ı' as u32));
        assert!(!tr.covered_by(&without_dotless_i));
        assert!(language("INDONESIAN").is_some() && language("klingon").is_none());
        assert!(language("indonesian").unwrap().covered_by(&cov(tr.chars())), "Latin A-Z is enough for Indonesian");
        assert!(!language("vietnamese").unwrap().covered_by(&all));
    }
}

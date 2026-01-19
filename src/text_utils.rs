use pinyin::ToPinyin;

/// Build a compact pinyin index string from multiple text fragments.
/// The format is "full|initials" joined by spaces for multiple fragments.
pub fn build_pinyin_index<'a, I>(texts: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let mut parts = Vec::new();
    for text in texts {
        if let Some(part) = build_single_index(text) {
            parts.push(part);
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

fn build_single_index(source: &str) -> Option<String> {
    let mut syllables: Vec<String> = Vec::new();
    let mut initials = String::new();

    for maybe in source.to_pinyin() {
        let Some(pinyin) = maybe else {
            continue;
        };
        let plain = pinyin.plain();
        if plain.is_empty() {
            continue;
        }

        let syllable = plain.to_ascii_lowercase();
        if syllable.is_empty() {
            continue;
        }

        if let Some(initial) = syllable.chars().next() {
            initials.push(initial);
        }
        syllables.push(syllable);
    }

    if syllables.is_empty() {
        return None;
    }

    let joined = syllables.join("");
    if joined.is_empty() {
        return None;
    }

    if !initials.is_empty() && initials != joined {
        Some(format!("{joined}|{initials}"))
    } else {
        Some(joined)
    }
}

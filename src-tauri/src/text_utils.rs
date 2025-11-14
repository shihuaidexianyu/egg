use pinyin::ToPinyin;

/// Extend the given keyword list with pinyin variants so that
/// fuzzy matching can work with full pinyin and initials.
pub fn extend_keywords_with_pinyin(keywords: &mut Vec<String>) {
    let mut additions = Vec::new();
    for keyword in keywords.iter() {
        extend_single_keyword(keyword, &mut additions);
    }

    if additions.is_empty() {
        return;
    }

    keywords.extend(additions);
}

fn extend_single_keyword(source: &str, target: &mut Vec<String>) {
    // Track whether at least one Chinese character produced a syllable.
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
        return;
    }

    // 连写形式，例如 "weixin"。
    let joined = syllables.join("");
    if !joined.is_empty() {
        target.push(joined);
    }

    // 带空格的形式，例如 "wei xin"，有利于分词匹配。
    if syllables.len() > 1 {
        let spaced = syllables.join(" ");
        if !spaced.is_empty() {
            target.push(spaced);
        }
    }

    // 首字母缩写，例如 "wx"。
    if !initials.is_empty() {
        target.push(initials);
    }
}

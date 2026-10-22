//! OCR text clean-up: reading order of recognised boxes and line merging for RU/EN.
//! OCR engines return one string per visual line. Left as they are, a sentence wrapped across lines
//! is split in the index and a numbered clause list loses its structure. `layout` orders boxes into
//! rows; `reflow` joins mid-sentence breaks, removes line-end hyphenation and keeps every list item
//! (`2.`, `3.1.`, `a)`, `-`) on a line of its own with its own number.

/// One recognised text box; `y` is the vertical centre (larger = higher on the page), as in Vision.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) struct Obs {
    pub x: f64,
    pub y: f64,
    pub h: f64,
    pub text: String,
}

/// Boxes to lines in reading order: rows by vertical centre (tolerant of boxes of different
/// heights), left to right inside a row.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn layout(mut obs: Vec<Obs>) -> Vec<String> {
    obs.sort_by(|a, b| b.y.partial_cmp(&a.y).unwrap_or(std::cmp::Ordering::Equal));
    let mut rows: Vec<(f64, f64, Vec<Obs>)> = vec![]; // (centre, height, boxes)
    for o in obs {
        match rows.last_mut() {
            Some((cy, ch, boxes)) if (*cy - o.y).abs() <= 0.5 * ch.min(o.h) => {
                let n = boxes.len() as f64;
                *cy = (*cy * n + o.y) / (n + 1.0);
                *ch = ch.min(o.h);
                boxes.push(o);
            }
            _ => rows.push((o.y, o.h, vec![o])),
        }
    }
    rows.into_iter()
        .map(|(_, _, mut b)| {
            b.sort_by(|p, q| p.x.partial_cmp(&q.x).unwrap_or(std::cmp::Ordering::Equal));
            b.into_iter().map(|o| o.text).collect::<Vec<_>>().join(" ")
        })
        .collect()
}

/// Start of a numbered or bulleted item: `2.`, `3.1.`, `4)`, `a)`, `(5)`, `-`, `•`.
fn is_item(line: &str) -> bool {
    let l = line.trim_start();
    let mut it = l.chars();
    let Some(c0) = it.next() else { return false };
    if matches!(c0, '-' | '–' | '—' | '•' | '·' | '*') {
        return it.next().is_some_and(char::is_whitespace);
    }
    let mut rest = l;
    if let Some(r) = rest.strip_prefix('(') {
        rest = r;
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let mut after = &rest[digits..];
        // 3.1.2. style
        while let Some(r) = after.strip_prefix('.') {
            let d = r.chars().take_while(char::is_ascii_digit).count();
            if d == 0 || d > 3 {
                break;
            }
            after = &r[d..];
        }
        if let Some(r) = after.strip_prefix(['.', ')']) {
            return r.chars().next().is_some_and(char::is_whitespace);
        }
        return false;
    }
    // a) б)
    let mut ch = rest.chars();
    matches!((ch.next(), ch.next(), ch.next()), (Some(a), Some(')'), Some(w)) if a.is_alphabetic() && w.is_whitespace())
}

fn ends_sentence(line: &str) -> bool {
    let t = line.trim_end().trim_end_matches(['»', '"', '”', '\'', ')']);
    t.ends_with(['.', '!', '?', ':', ';', '…'])
}

fn all_caps(line: &str) -> bool {
    let letters: Vec<char> = line.chars().filter(|c| c.is_alphabetic()).collect();
    letters.len() >= 3 && letters.iter().all(|c| c.is_uppercase())
}

fn hyphen_break(line: &str) -> bool {
    let t = line.trim_end();
    let mut r = t.chars().rev();
    matches!(r.next(), Some('-' | '\u{ad}' | '¬')) && r.next().is_some_and(char::is_alphabetic)
}

/// Merge OCR line breaks. Pure function of the text; the page width is estimated from the longest line.
pub fn reflow(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let full = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64;
    // (is_item, text)
    let mut blocks: Vec<(bool, String)> = vec![];
    let mut last_len = 0usize;
    for line in lines {
        let len = line.chars().count();
        let item = is_item(line);
        let join = match blocks.last() {
            Some((_, prev)) if !item && !all_caps(prev) && !all_caps(line) => {
                let wide = last_len as f64;
                let starts_lower = line
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_lowercase() || ",;:)".contains(c));
                if hyphen_break(prev) {
                    wide >= 0.5 * full && line.chars().next().is_some_and(char::is_lowercase)
                } else if ends_sentence(prev) {
                    false
                } else {
                    (wide >= 0.5 * full && starts_lower) || wide >= 0.8 * full
                }
            }
            _ => false,
        };
        if join {
            let (_, prev) = blocks.last_mut().expect("join implies a previous block");
            if hyphen_break(prev) {
                let cut = prev.trim_end().trim_end_matches(['-', '\u{ad}', '¬']).len();
                prev.truncate(cut);
            } else {
                prev.push(' ');
            }
            prev.push_str(line);
        } else {
            blocks.push((item, line.to_string()));
        }
        last_len = len;
    }
    let mut out = String::new();
    for (i, (item, b)) in blocks.iter().enumerate() {
        if i > 0 {
            // items of one list stay on consecutive lines (one Markdown list); everything else is its own paragraph
            out.push_str(if *item && blocks[i - 1].0 { "\n" } else { "\n\n" });
        }
        out.push_str(b);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obs(x: f64, y: f64, h: f64, t: &str) -> Obs {
        Obs {
            x,
            y,
            h,
            text: t.into(),
        }
    }

    #[test]
    fn rows_ignore_height_differences() {
        // taller first box would sort above a lower neighbour with a plain top-edge comparison
        let l = layout(vec![
            obs(0.1, 0.80, 0.030, "Поставщик) и Акционерное"),
            obs(0.1, 0.84, 0.020, "Общество (далее"),
            obs(0.5, 0.80, 0.020, "общество «Балтийская»"),
        ]);
        assert_eq!(
            l,
            vec![
                "Общество (далее",
                "Поставщик) и Акционерное общество «Балтийская»"
            ]
        );
    }

    #[test]
    fn numbers_and_wrapped_sentences() {
        let raw = "ДОГОВОР ПОСТАВКИ № 17/2026\nг. Москва, 14 марта 2026 года\n\
1. Предмет договора. Поставщик обязуется передать в собственность\nПокупателя оборудование согласно спецификации.\n\
2. Срок поставки. Товар поставляется в течение тридцати календарных\nдней с момента получения предоплаты.\n\
3. Цена и порядок расчётов.";
        let out = reflow(raw);
        assert_eq!(
            out,
            "ДОГОВОР ПОСТАВКИ № 17/2026\n\nг. Москва, 14 марта 2026 года\n\n\
1. Предмет договора. Поставщик обязуется передать в собственность Покупателя оборудование согласно спецификации.\n\
2. Срок поставки. Товар поставляется в течение тридцати календарных дней с момента получения предоплаты.\n\
3. Цена и порядок расчётов."
        );
    }

    #[test]
    fn dehyphenates_ru_and_en() {
        let raw = "Стороны обязуются не разглашать условия до-\nговора третьим лицам без согласия.\n\
The supplier shall deliver the equip-\nment within thirty days of payment.";
        let out = reflow(raw);
        assert!(out.contains("условия договора третьим"), "{out}");
        assert!(out.contains("equipment within"), "{out}");
    }

    #[test]
    fn item_markers() {
        for s in [
            "2. text",
            "3.1. text",
            "4) text",
            "(5) text",
            "а) текст",
            "- text",
            "• text",
        ] {
            assert!(is_item(s), "{s}");
        }
        for s in [
            "2026 года",
            "14 марта",
            "2 480 000 рублей",
            "г. Москва",
            "0,1 процента",
        ] {
            assert!(!is_item(s), "{s}");
        }
    }

    #[test]
    fn short_lines_stay_separate() {
        assert_eq!(
            reflow("Это достаточно длинная строка текста.\nСторона А\nСторона Б"),
            "Это достаточно длинная строка текста.\n\nСторона А\n\nСторона Б"
        );
    }
}

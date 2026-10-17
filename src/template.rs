/// Every `(name, index)` reference in the template, in order, including the
/// alternatives of a `{a|b}` group.
pub fn fields(tpl: &str) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else {
            break;
        };
        for field in rest[start + 1..start + end].split('|') {
            let (name, idx) = match field.split_once(':') {
                Some((n, i)) => (n, i.parse().unwrap_or(0)),
                None => (field, 0),
            };
            out.push((name.trim().to_owned(), idx));
        }
        rest = &rest[start + end + 1..];
    }
    out
}

/// Field names in the template that `known` does not list.
pub fn unknown(tpl: &str, known: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = fields(tpl)
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| !known.contains(&n.as_str()))
        .collect();
    out.dedup();
    out
}

pub fn render(tpl: &str, lookup: impl Fn(&str, usize) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('}') else {
            out.push_str(&rest[start..]);
            return clean(&out);
        };
        let value = rest[start + 1..start + end].split('|').find_map(|field| {
            let (name, idx) = match field.split_once(':') {
                Some((n, i)) => (n, i.parse().unwrap_or(0)),
                None => (field, 0),
            };
            lookup(name, idx)
                .map(|v| sanitize(&v))
                .filter(|v| !v.is_empty())
        });
        out.push_str(value.as_deref().unwrap_or_default());
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    clean(&out)
}

pub fn sanitize(v: &str) -> String {
    v.trim().replace(['/', '\0'], "-")
}

fn clean(s: &str) -> String {
    s.split('/')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup(name: &str, idx: usize) -> Option<String> {
        match (name, idx) {
            ("take.year", _) => Some("2024".into()),
            ("keyword", 0) => Some("beach".into()),
            ("keyword", 1) => Some("a/b".into()),
            ("place.name", _) => Some(" Boston ".into()),
            _ => None,
        }
    }

    #[test]
    fn substitutes_and_indexes() {
        assert_eq!(
            render("{take.year}/{keyword:0}/{keyword:1}", lookup),
            "2024/beach/a-b"
        );
        assert_eq!(render("{place.name}", lookup), "Boston");
    }

    #[test]
    fn falls_back_through_alternatives() {
        assert_eq!(render("{nope|keyword:1|take.year}", lookup), "a-b");
        assert_eq!(render("{nope|keyword:7}/x", lookup), "x");
        assert_eq!(render("{keyword:0|take.year}", lookup), "beach");
    }

    #[test]
    fn missing_collapses_segments() {
        assert_eq!(render("{take.year}/{keyword:5}/x", lookup), "2024/x");
        assert_eq!(render("/{nope}/", lookup), "");
    }

    #[test]
    fn lists_fields_and_unknown_names() {
        assert_eq!(
            fields("{take.year}/{keyword:1|label:0}/x{bad"),
            vec![
                ("take.year".to_owned(), 0),
                ("keyword".to_owned(), 1),
                ("label".to_owned(), 0)
            ]
        );
        assert_eq!(
            unknown(
                "{take.year}/{keywrd:0|label}/{keywrd}",
                &["take.year", "label"]
            ),
            vec!["keywrd".to_owned()]
        );
    }

    #[test]
    fn unterminated_is_literal() {
        assert_eq!(render("a/{take", lookup), "a/{take");
    }
}

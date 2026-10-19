//! Reuse CN dialogue only when its scenario structure matches the JP script.
//! Engine commands and asset references always come from the JP source.
use super::{message::collect_tags, parser::extract_chara_name};
use std::ops::Range;

struct Part {
    anchor: String,
    span: Range<usize>,
    replace: bool,
}

fn parts(text: &str) -> Vec<Part> {
    let mut result: Vec<Part> = Vec::new();
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let raw = line.trim_end_matches(['\r', '\n']);
        let end = offset + raw.len();
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with(['＄', '$']) || trimmed.starts_with("//") {
            offset += line.len();
            continue;
        }
        let (anchor, span, replace) = if raw.starts_with('＠') {
            ("speaker".to_owned(), offset..end, true)
        } else if raw.starts_with('？') && raw.contains('：') {
            let colon = raw.find('：').unwrap() + '：'.len_utf8();
            (raw[..colon].to_owned(), offset + colon..end, true)
        } else if let Some(name) = extract_chara_name(raw) {
            let start = raw.len() - name.len() - usize::from(raw.ends_with(']'));
            (
                raw[..start].to_owned(),
                offset + start..offset + start + name.len(),
                true,
            )
        } else if (trimmed.starts_with('[')
            && trimmed.ends_with(']')
            && trimmed.matches('[').count() == 1)
            || trimmed.starts_with('？')
        {
            (trimmed.to_owned(), offset..end, false)
        } else {
            if let Some(last) = result.last_mut().filter(|last| {
                last.anchor == "text" && text[last.span.end..offset].trim().is_empty()
            }) {
                last.span.end = end;
                offset += line.len();
                continue;
            }
            ("text".to_owned(), offset..end, true)
        };
        result.push(Part {
            anchor,
            span,
            replace,
        });
        offset += line.len();
    }
    result
}

pub fn merge(jp: &str, cn: &str) -> Result<String, String> {
    let source = parts(jp);
    let translated = parts(cn);
    if source.len() != translated.len() {
        return Err("scenario structure differs".into());
    }
    let mut output = String::with_capacity(jp.len());
    let mut offset = 0;
    for (a, b) in source.iter().zip(&translated) {
        if a.anchor != b.anchor {
            return Err("scenario commands differ".into());
        }
        if !a.replace {
            continue;
        }
        let jp_text = &jp[a.span.clone()];
        let cn_text = &cn[b.span.clone()];
        // Line wrapping and ruby may differ by language. All other inline
        // commands/placeholders must retain their order and multiplicity.
        let tags = |value: &str| {
            collect_tags(value)
                .into_iter()
                .filter(|t| t != "[r]")
                .collect::<Vec<_>>()
        };
        if tags(jp_text) != tags(cn_text) {
            return Err("dialogue control tags differ".into());
        }
        output.push_str(&jp[offset..a.span.start]);
        output.push_str(cn_text);
        offset = a.span.end;
    }
    output.push_str(&jp[offset..]);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preserves_jp_commands_and_accepts_dialogue_wrapping() {
        let jp = "＄jp\n[charaSet A 123 1 名前]\n＠名前\n一行[r]二行\n[k]\n[end]\n";
        let cn = "＄cn\n[charaSet A 123 1 名字]\n＠名字\n第一行\n第二行\n[k]\n[end]\n";
        let merged = merge(jp, cn).unwrap();
        assert!(merged.starts_with("＄jp\n"));
        assert!(merged.contains("＠名字\n第一行\n第二行"));
        assert!(merge(jp, &cn.replace("[end]", "[jump other]")).is_err());
        assert!(merge(jp, &cn.replace("第一行", "[%1]第一行")).is_err());
    }
}

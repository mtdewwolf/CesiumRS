//! ShaderProgram's workaround for mismatched vertex/fragment uniform precision.
use std::collections::{HashMap, HashSet};
pub fn handle_uniform_precision_mismatches(
    vertex: &str,
    fragment: &str,
    highp_float: bool,
    highp_int: bool,
) -> (String, HashMap<String, String>) {
    if highp_float && highp_int {
        return (fragment.into(), HashMap::new());
    }
    fn names(source: &str) -> Vec<String> {
        let mut names = vec![];
        let mut cursor = 0;
        while let Some(start) = source[cursor..].find("uniform").map(|n| n + cursor) {
            let line_end =
                source[start..].find(['\n', '\r', '\u{2028}', '\u{2029}']).map_or(source.len(), |n| start + n);
            let mut accepted = None;
            for (offset, c) in source[start..line_end].char_indices() {
                if matches!(c, '=' | '[' | ';') {
                    let at = start + offset;
                    let first_brace = source[at..].find(['{', '}']).map(|n| source.as_bytes()[at + n]);
                    if first_brace != Some(b'}') {
                        accepted = Some(at);
                        break;
                    }
                }
            }
            if let Some(end) = accepted {
                let text = source[start..end].trim();
                let name = &text[text.rfind(' ').map_or(0, |n| n + 1)..];
                names.push(name.into());
                cursor = end;
            } else {
                cursor = start + 7;
            }
        }
        names
    }
    let vertex_names = names(vertex);
    let fragment_names: HashSet<_> = names(fragment).into_iter().collect();
    let mut fragment = fragment.to_string();
    let mut duplicates = HashMap::new();
    for name in vertex_names {
        if fragment_names.contains(&name) {
            let duplicate = format!("czm_mediump_{name}");
            // Shader text supplies the name, so metacharacters must stay literals.
            let pattern = format!(r"{}\b", regex::escape(&name));
            let re = regex::Regex::new(&pattern).expect("escaped identifier is a valid pattern");
            fragment = re.replace_all(&fragment, regex::NoExpand(&duplicate)).into_owned();
            duplicates.insert(duplicate, name);
        }
    }
    (fragment, duplicates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metacharacter_uniform_names_stay_literals() {
        let vertex = "uniform float a(b;";
        let fragment = "uniform float a(b;\nvoid main() { gl_FragColor = vec4(a(b)); }";
        let (text, names) = handle_uniform_precision_mismatches(vertex, fragment, false, false);
        assert!(text.contains("czm_mediump_a(b"));
        assert!(!text.contains("uniform float a(b;"));
        assert_eq!(names.get("czm_mediump_a(b").map(String::as_str), Some("a(b"));
    }
}

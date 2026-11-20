//! Key-by-key merge for `key:value` files such as Minecraft's `options.txt`.
//! Keeps line order and every key the pack does not mention.

use std::collections::BTreeMap;

pub fn merge(
    existing: &str,
    sep: &str,
    defaults: &BTreeMap<String, String>,
    force: &BTreeMap<String, String>,
) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for line in existing.lines() {
        match line.split_once(sep) {
            Some((k, _)) if !k.is_empty() => {
                seen.insert(k.to_string());
                match force.get(k) {
                    Some(v) => out.push(format!("{k}{sep}{v}")),
                    None => out.push(line.to_string()),
                }
            }
            _ => out.push(line.to_string()),
        }
    }
    for (k, v) in force.iter().chain(defaults.iter()) {
        if seen.insert(k.clone()) {
            out.push(format!("{k}{sep}{v}"));
        }
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn merges() {
        let d: BTreeMap<_, _> = [("key_jump".into(), "key.keyboard.space".into()), ("guiScale".into(), "2".into())].into();
        let f: BTreeMap<_, _> = [("resourcePacks".into(), "[\"vanilla\",\"file/pack\"]".into())].into();
        let got = merge("version:3465\nguiScale:4\nresourcePacks:[]\n", ":", &d, &f);
        assert_eq!(got, "version:3465\nguiScale:4\nresourcePacks:[\"vanilla\",\"file/pack\"]\nkey_jump:key.keyboard.space\n");
    }
}

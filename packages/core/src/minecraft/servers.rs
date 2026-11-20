//! Adds the pack's servers to `servers.dat` (uncompressed NBT) without
//! touching servers the player added.

use crate::manifest::Server;
use anyhow::Result;
use fastnbt::Value;
use std::collections::HashMap;
use std::path::Path;

pub fn ensure_servers(game_dir: &Path, servers: &[Server]) -> Result<()> {
    if servers.is_empty() {
        return Ok(());
    }
    let file = game_dir.join("servers.dat");
    let mut root: HashMap<String, Value> = match std::fs::read(&file) {
        Ok(b) => fastnbt::from_bytes(&b).unwrap_or_default(),
        Err(_) => HashMap::new(),
    };
    let mut list = match root.remove("servers") {
        Some(Value::List(l)) => l,
        _ => Vec::new(),
    };
    let has = |list: &Vec<Value>, ip: &str| {
        list.iter().any(|v| match v {
            Value::Compound(c) => matches!(c.get("ip"), Some(Value::String(s)) if s.eq_ignore_ascii_case(ip)),
            _ => false,
        })
    };
    let mut changed = false;
    for s in servers.iter().rev() {
        if !has(&list, &s.address) {
            let mut c = HashMap::new();
            c.insert("name".to_string(), Value::String(s.name.clone()));
            c.insert("ip".to_string(), Value::String(s.address.clone()));
            list.insert(0, Value::Compound(c));
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    root.insert("servers".into(), Value::List(list));
    std::fs::create_dir_all(game_dir)?;
    std::fs::write(&file, fastnbt::to_bytes(&Value::Compound(root))?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adds_once() {
        let d = tempfile::tempdir().unwrap();
        let s = vec![Server { name: "Gango".into(), address: "mc.example.org".into() }];
        ensure_servers(d.path(), &s).unwrap();
        ensure_servers(d.path(), &s).unwrap();
        let root: HashMap<String, Value> = fastnbt::from_bytes(&std::fs::read(d.path().join("servers.dat")).unwrap()).unwrap();
        match &root["servers"] {
            Value::List(l) => assert_eq!(l.len(), 1),
            _ => panic!(),
        }
    }
}

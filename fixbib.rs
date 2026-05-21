// fixbib.rs
//
// Usage:
//   rustc fixbib.rs
//   ./fixbib main.tex --write
//
// Without --write it only prints a summary.
//
// Backups are written as:
//   main.tex.bak
//   refs.bib.bak

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct BibEntry {
    file_idx: usize,
    kind: String,
    old_key: String,
    body: String,
    fields: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct BibFile {
    path: PathBuf,
    specials: Vec<String>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 || args.len() > 3 {
        eprintln!("Usage: {} main.tex [--dry-run]", args[0]);
        std::process::exit(1);
    }

    let tex_path = PathBuf::from(&args[1]);
    let do_write = !args.iter().any(|a| a == "--dry-run");

    let tex = fs::read_to_string(&tex_path)?;
    let bib_paths = find_bib_files(&tex, &tex_path);

    if bib_paths.is_empty() {
        eprintln!("No bibliography found. Expected \\bibliography{{...}} or \\addbibresource{{...}}.");
        std::process::exit(1);
    }

    let mut bib_files = Vec::new();
    let mut entries = Vec::new();

    for path in bib_paths {
        let raw = fs::read_to_string(&path)?;
        let file_idx = bib_files.len();
        let (specials, mut file_entries) = parse_bib(&raw, file_idx);
        bib_files.push(BibFile { path, specials });
        entries.append(&mut file_entries);
    }

    let (used_keys, keep_all) = collect_used_citation_keys(&tex);

    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, e) in entries.iter().enumerate() {
        groups.entry(entry_signature(e)).or_default().push(i);
    }

    let mut rep_for_old: HashMap<String, usize> = HashMap::new();
    let mut kept_reps: HashSet<usize> = HashSet::new();

    for idxs in groups.values() {
        let chosen = idxs
            .iter()
            .copied()
            .find(|&i| used_keys.contains(&entries[i].old_key))
            .unwrap_or(idxs[0]);

        let group_is_used = keep_all || idxs.iter().any(|&i| used_keys.contains(&entries[i].old_key));

        for &i in idxs {
            rep_for_old.insert(entries[i].old_key.clone(), chosen);
        }

        if group_is_used {
            kept_reps.insert(chosen);
        }
    }

    let mut kept: Vec<usize> = kept_reps.iter().copied().collect();
    kept.sort_by_key(|&i| (entries[i].file_idx, i));

    let mut new_for_rep: HashMap<usize, String> = HashMap::new();
    let mut used_new_keys: HashSet<String> = HashSet::new();

    for &i in &kept {
        let base = make_new_key(&entries[i]);
        let unique = unique_key(base, &mut used_new_keys);
        new_for_rep.insert(i, unique);
    }

    let mut old_to_new: HashMap<String, String> = HashMap::new();
    let mut new_key_year: HashMap<String, i32> = HashMap::new();

    for (old, rep) in &rep_for_old {
        if let Some(new_key) = new_for_rep.get(rep) {
            old_to_new.insert(old.clone(), new_key.clone());
        }
    }

    for (&rep, new_key) in &new_for_rep {
        new_key_year.insert(new_key.clone(), entry_year_i32(&entries[rep]).unwrap_or(9999));
    }

    let new_tex = rewrite_tex_citations(&tex, &old_to_new, &new_key_year);
    let new_bibs = render_bib_files(&bib_files, &entries, &kept, &new_for_rep);

    println!("Found bibliography files:");
    for bf in &bib_files {
        println!("  {}", bf.path.display());
    }

    println!();
    println!("Original entries: {}", entries.len());
    println!("Used citation keys in tex: {}", used_keys.len());
    println!("Kept entries after duplicate/unused removal: {}", kept.len());
    println!("Removed entries: {}", entries.len().saturating_sub(kept.len()));

    println!();
    println!("Citation key rewrites:");
    let mut rewrites: Vec<_> = old_to_new.iter().collect();
    rewrites.sort_by(|a, b| a.0.cmp(b.0));
    for (old, new) in rewrites.iter().take(30) {
        if old != new {
            println!("  {} -> {}", old, new);
        }
    }
    if rewrites.len() > 30 {
        println!("  ...");
    }

    let missing: Vec<_> = used_keys
        .iter()
        .filter(|k| *k != "*" && !old_to_new.contains_key(*k))
        .collect();

    if !missing.is_empty() {
        println!();
        println!("Warning: cited keys not found in bib file:");
        for k in missing {
            println!("  {}", k);
        }
    }

    if do_write {
        write_with_backup(&tex_path, &new_tex)?;

        for (path, content) in new_bibs {
            write_with_backup(&path, &content)?;
        }

        println!();
        println!("Wrote updated files. Backups have suffix .bak.");
    } else {
        println!();
        println!("Dry run. Re-run without --dry-run to modify files.");
    }

    Ok(())
}

fn find_bib_files(tex: &str, tex_path: &Path) -> Vec<PathBuf> {
    let base_dir = tex_path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for content in find_command_brace_args(tex, "bibliography") {
        for item in content.split(',') {
            let name = item.trim();
            if name.is_empty() {
                continue;
            }
            let mut p = base_dir.join(name);
            if p.extension().is_none() {
                p.set_extension("bib");
            }
            if seen.insert(p.clone()) {
                out.push(p);
            }
        }
    }

    for content in find_command_brace_args(tex, "addbibresource") {
        let name = content.trim();
        if name.is_empty() {
            continue;
        }
        let mut p = base_dir.join(name);
        if p.extension().is_none() {
            p.set_extension("bib");
        }
        if seen.insert(p.clone()) {
            out.push(p);
        }
    }

    out
}

fn find_command_brace_args(tex: &str, target: &str) -> Vec<String> {
    let mut out = Vec::new();
    let b = tex.as_bytes();
    let mut i = 0;

    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }

        let start = i;
        i += 1;

        let cmd_start = i;
        while i < b.len() && b[i].is_ascii_alphabetic() {
            i += 1;
        }

        if cmd_start == i {
            i = start + 1;
            continue;
        }

        let cmd = &tex[cmd_start..i];

        if cmd != target {
            continue;
        }

        let mut j = i;

        loop {
            skip_ws(tex, &mut j);
            if j < b.len() && b[j] == b'[' {
                if let Some(close) = find_matching(tex, j, b'[', b']') {
                    j = close + 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        skip_ws(tex, &mut j);

        if j < b.len() && b[j] == b'{' {
            if let Some(close) = find_matching(tex, j, b'{', b'}') {
                out.push(tex[j + 1..close].to_string());
                i = close + 1;
            }
        }
    }

    out
}

fn parse_bib(raw: &str, file_idx: usize) -> (Vec<String>, Vec<BibEntry>) {
    let mut specials = Vec::new();
    let mut entries = Vec::new();

    let b = raw.as_bytes();
    let mut i = 0;

    while i < b.len() {
        let Some(rel) = raw[i..].find('@') else {
            break;
        };

        let at = i + rel;
        let mut j = at + 1;

        skip_ws(raw, &mut j);

        let kind_start = j;
        while j < b.len() && b[j].is_ascii_alphabetic() {
            j += 1;
        }

        if kind_start == j {
            i = at + 1;
            continue;
        }

        let kind = raw[kind_start..j].to_string();
        let kind_l = kind.to_ascii_lowercase();

        skip_ws(raw, &mut j);

        if j >= b.len() || !(b[j] == b'{' || b[j] == b'(') {
            i = j;
            continue;
        }

        let open = b[j];
        let close_ch = if open == b'{' { b'}' } else { b')' };

        let Some(close) = find_matching(raw, j, open, close_ch) else {
            break;
        };

        let full_raw = raw[at..=close].to_string();
        let content = &raw[j + 1..close];

        if kind_l == "string" || kind_l == "preamble" || kind_l == "comment" {
            specials.push(full_raw);
            i = close + 1;
            continue;
        }

        let Some(comma) = find_top_level_comma(content) else {
            specials.push(full_raw);
            i = close + 1;
            continue;
        };

        let key = content[..comma].trim().to_string();
        let body = content[comma + 1..].to_string();

        if key.is_empty() {
            specials.push(full_raw);
            i = close + 1;
            continue;
        }

        let fields = parse_fields(&body);

        entries.push(BibEntry {
            file_idx,
            kind,
            old_key: key,
            body,
            fields,
        });

        i = close + 1;
    }

    (specials, entries)
}

fn parse_fields(body: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let b = body.as_bytes();
    let mut i = 0;

    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b',') {
            i += 1;
        }

        let name_start = i;

        while i < b.len()
            && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'-')
        {
            i += 1;
        }

        if name_start == i {
            i += 1;
            continue;
        }

        let name = body[name_start..i].trim().to_ascii_lowercase();

        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }

        if i >= b.len() || b[i] != b'=' {
            continue;
        }

        i += 1;

        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }

        let val_start = i;
        let mut depth = 0i32;
        let mut quote = false;

        while i < b.len() {
            match b[i] {
                b'\\' => {
                    i += 2;
                    continue;
                }
                b'"' if depth == 0 => {
                    quote = !quote;
                }
                b'{' if !quote => {
                    depth += 1;
                }
                b'}' if !quote => {
                    depth -= 1;
                }
                b',' if depth == 0 && !quote => {
                    break;
                }
                _ => {}
            }
            i += 1;
        }

        let value = body[val_start..i].trim().to_string();
        map.insert(name, value);

        if i < b.len() && b[i] == b',' {
            i += 1;
        }
    }

    map
}

fn collect_used_citation_keys(tex: &str) -> (HashSet<String>, bool) {
    let mut used = HashSet::new();
    let mut keep_all = false;
    let b = tex.as_bytes();
    let mut i = 0;

    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }

        if let Some((_cmd, s, e)) = cite_group_at(tex, i) {
            let group = &tex[s..e];

            for k in group.split(',') {
                let key = k.trim();

                if key.is_empty() {
                    continue;
                }

                if key == "*" {
                    keep_all = true;
                }

                used.insert(key.to_string());
            }

            i = e + 1;
        } else {
            i += 1;
        }
    }

    (used, keep_all)
}

fn rewrite_tex_citations(
    tex: &str,
    old_to_new: &HashMap<String, String>,
    new_key_year: &HashMap<String, i32>,
) -> String {
    let mut out = String::new();
    let b = tex.as_bytes();
    let mut i = 0;
    let mut last = 0;

    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }

        if let Some((cmd, s, e)) = cite_group_at(tex, i) {
            let old_group = &tex[s..e];

            let mut keys: Vec<String> = old_group
                .split(',')
                .map(|k| k.trim().to_string())
                .filter(|k| !k.is_empty())
                .map(|k| old_to_new.get(&k).cloned().unwrap_or(k))
                .collect();

            if keys.len() > 1 && !keys.iter().any(|k| k == "*") && cmd != "nocite" {
                let mut indexed: Vec<(usize, String)> = keys.into_iter().enumerate().collect();

                indexed.sort_by(|a, b| {
                    let ya = *new_key_year.get(&a.1).unwrap_or(&9999);
                    let yb = *new_key_year.get(&b.1).unwrap_or(&9999);
                    ya.cmp(&yb).then(a.0.cmp(&b.0))
                });

                keys = indexed.into_iter().map(|(_, k)| k).collect();
            }

            out.push_str(&tex[last..s]);
            out.push_str(&keys.join(","));
            last = e;
            i = e + 1;
        } else {
            i += 1;
        }
    }

    out.push_str(&tex[last..]);
    out
}

fn cite_group_at(tex: &str, pos: usize) -> Option<(String, usize, usize)> {
    let b = tex.as_bytes();

    if pos >= b.len() || b[pos] != b'\\' {
        return None;
    }

    let mut i = pos + 1;
    let cmd_start = i;

    while i < b.len() && b[i].is_ascii_alphabetic() {
        i += 1;
    }

    if cmd_start == i {
        return None;
    }

    let cmd = tex[cmd_start..i].to_string();

    if !is_cite_cmd(&cmd) {
        return None;
    }

    if i < b.len() && b[i] == b'*' {
        i += 1;
    }

    loop {
        skip_ws(tex, &mut i);

        if i < b.len() && b[i] == b'[' {
            let close = find_matching(tex, i, b'[', b']')?;
            i = close + 1;
        } else {
            break;
        }
    }

    skip_ws(tex, &mut i);

    if i < b.len() && b[i] == b'{' {
        let close = find_matching(tex, i, b'{', b'}')?;
        return Some((cmd, i + 1, close));
    }

    None
}

fn is_cite_cmd(cmd: &str) -> bool {
    let c = cmd.to_ascii_lowercase();

    if c.starts_with("declare")
        || c.starts_with("new")
        || c.starts_with("renew")
        || c.starts_with("provide")
    {
        return false;
    }

    c == "nocite"
        || c.starts_with("cite")
        || c.ends_with("cite")
        || c.ends_with("cites")
        || c.contains("cite")
}

fn render_bib_files(
    bib_files: &[BibFile],
    entries: &[BibEntry],
    kept: &[usize],
    new_for_rep: &HashMap<usize, String>,
) -> Vec<(PathBuf, String)> {
    let kept_set: HashSet<usize> = kept.iter().copied().collect();
    let mut out = Vec::new();

    for (file_idx, bf) in bib_files.iter().enumerate() {
        let mut s = String::new();

        for sp in &bf.specials {
            s.push_str(sp.trim_end());
            s.push_str("\n\n");
        }

        for (i, e) in entries.iter().enumerate() {
            if e.file_idx != file_idx || !kept_set.contains(&i) {
                continue;
            }

            let new_key = new_for_rep.get(&i).unwrap();

            s.push('@');
            s.push_str(&e.kind);
            s.push('{');
            s.push_str(new_key);
            s.push(',');

            if e.body.starts_with('\n') {
                s.push_str(&e.body);
            } else {
                s.push('\n');
                s.push_str(&e.body);
            }

            if !s.ends_with('\n') {
                s.push('\n');
            }

            s.push_str("}\n\n");
        }

        out.push((bf.path.clone(), s));
    }

    out
}

fn entry_signature(e: &BibEntry) -> String {
    let author = e
        .fields
        .get("author")
        .or_else(|| e.fields.get("editor"))
        .map(|s| latex_plain(s))
        .unwrap_or_default();

    let title = e
        .fields
        .get("title")
        .map(|s| latex_plain(s))
        .unwrap_or_default();

    let year = entry_year(e).unwrap_or_else(|| "0000".to_string());

    if title.trim().is_empty() {
        format!("key:{}", e.old_key.to_ascii_lowercase())
    } else {
        format!("{}|{}|{}", normalize_spaces(&author), normalize_spaces(&title), year)
    }
}

fn make_new_key(e: &BibEntry) -> String {
    let author_raw = e
        .fields
        .get("author")
        .or_else(|| e.fields.get("editor"))
        .map(String::as_str)
        .unwrap_or("");

    let title_raw = e.fields.get("title").map(String::as_str).unwrap_or("");

    let author = author_component(author_raw);
    let title = title_component(title_raw);
    let year = entry_year(e).unwrap_or_else(|| "0000".to_string());

    format!("{}_{}_{}", author, title, year)
}

fn author_component(raw: &str) -> String {
    let first = raw.split(" and ").next().unwrap_or(raw).trim();

    let family_raw = if let Some(pos) = first.find(',') {
        &first[..pos]
    } else {
        first
    };

    let words = words_from_latex(family_raw);

    if words.is_empty() {
        return "anon".to_string();
    }

    if first.contains(',') {
        words.join("")
    } else {
        words.last().cloned().unwrap_or_else(|| "anon".to_string())
    }
}

fn title_component(raw: &str) -> String {
    let stop: HashSet<&str> = [
        "a", "an", "the", "on", "of", "for", "and", "or", "in", "to", "with", "by", "from",
    ]
    .iter()
    .copied()
    .collect();

    for w in words_from_latex(raw) {
        if !stop.contains(w.as_str()) {
            return w;
        }
    }

    "untitled".to_string()
}

fn entry_year(e: &BibEntry) -> Option<String> {
    e.fields
        .get("year")
        .or_else(|| e.fields.get("date"))
        .and_then(|s| first_four_digit_year(s))
}

fn entry_year_i32(e: &BibEntry) -> Option<i32> {
    entry_year(e).and_then(|y| y.parse::<i32>().ok())
}

fn first_four_digit_year(s: &str) -> Option<String> {
    let bytes = s.as_bytes();

    for i in 0..bytes.len().saturating_sub(3) {
        if bytes[i].is_ascii_digit()
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
        {
            return Some(s[i..i + 4].to_string());
        }
    }

    None
}

fn unique_key(base: String, used: &mut HashSet<String>) -> String {
    if used.insert(base.clone()) {
        return base;
    }

    for n in 2.. {
        let candidate = format!("{}_{}", base, n);
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!()
}

fn latex_plain(raw: &str) -> String {
    words_from_latex(raw).join(" ")
}

fn words_from_latex(raw: &str) -> Vec<String> {
    let mut s = raw.trim().to_string();

    loop {
        let t = s.trim();

        if t.len() >= 2 && t.starts_with('{') && t.ends_with('}') {
            s = t[1..t.len() - 1].to_string();
            continue;
        }

        if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
            s = t[1..t.len() - 1].to_string();
            continue;
        }

        break;
    }

    let mut out = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            while let Some(&p) = chars.peek() {
                if p.is_alphabetic() {
                    chars.next();
                } else {
                    break;
                }
            }
            continue;
        }

        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                if lc.is_ascii_alphanumeric() {
                    out.push(lc);
                } else {
                    out.push(' ');
                }
            }
        } else {
            out.push(' ');
        }
    }

    out.split_whitespace()
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

fn normalize_spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_top_level_comma(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    let mut depth = 0i32;
    let mut quote = false;
    let mut i = 0;

    while i < b.len() {
        match b[i] {
            b'\\' => {
                i += 2;
                continue;
            }
            b'"' if depth == 0 => {
                quote = !quote;
            }
            b'{' if !quote => {
                depth += 1;
            }
            b'}' if !quote => {
                depth -= 1;
            }
            b',' if depth == 0 && !quote => {
                return Some(i);
            }
            _ => {}
        }

        i += 1;
    }

    None
}

fn find_matching(s: &str, open_idx: usize, open: u8, close: u8) -> Option<usize> {
    let b = s.as_bytes();

    if open_idx >= b.len() || b[open_idx] != open {
        return None;
    }

    let mut depth = 1i32;
    let mut i = open_idx + 1;

    while i < b.len() {
        if b[i] == b'\\' {
            i += 2;
            continue;
        }

        if b[i] == open {
            depth += 1;
        } else if b[i] == close {
            depth -= 1;

            if depth == 0 {
                return Some(i);
            }
        }

        i += 1;
    }

    None
}

fn skip_ws(s: &str, i: &mut usize) {
    let b = s.as_bytes();

    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn write_with_backup(path: &Path, content: &str) -> std::io::Result<()> {
    let backup = backup_path(path);

    if path.exists() {
        fs::copy(path, &backup)?;
    }

    fs::write(path, content)
}

fn backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "backup".to_string());

    path.with_file_name(format!("{}.bak", file_name))
}
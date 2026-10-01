// fixbib.rs
//
// Usage:
//   rustc fixbib.rs
//   ./fixbib main.tex [--no-delete]
//
// --no-delete keeps references that are not cited anywhere.
//
// The tex file and referenced bibliography files are updated in place with .bak backups.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

mod common;

use common::{collapse_whitespace, skip_ws};

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

#[derive(Clone, Debug)]
struct TexFile {
    path: PathBuf,
    content: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tex_path = common::input_path_arg("main.tex [--no-delete]");
    let no_delete = std::env::args().any(|a| a == "--no-delete");
    let root_dir = tex_path.parent().unwrap_or_else(|| Path::new("."));
    let tex_files = collect_tex_files(&tex_path)?;
    let bib_paths = find_bib_files(&tex_files, root_dir);

    if bib_paths.is_empty() {
        eprintln!(
            "No bibliography found. Expected \\bibliography{{...}} or \\addbibresource{{...}}."
        );
        std::process::exit(1);
    }

    let mut bib_files = Vec::new();
    let mut entries = Vec::new();

    for path in bib_paths {
        let raw = std::fs::read_to_string(&path)?;
        let file_idx = bib_files.len();
        let (specials, mut file_entries) = parse_bib(&raw, file_idx);
        bib_files.push(BibFile { path, specials });
        entries.append(&mut file_entries);
    }

    let used_keys: HashSet<String> = citation_order(&tex_files, root_dir).into_iter().collect();
    let keep_all = no_delete || used_keys.contains("*");

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

        let group_is_used = keep_all
            || idxs
                .iter()
                .any(|&i| used_keys.contains(&entries[i].old_key));

        for &i in idxs {
            rep_for_old.insert(entries[i].old_key.clone(), chosen);
        }

        if group_is_used {
            kept_reps.insert(chosen);
        }
    }

    let mut kept: Vec<usize> = kept_reps.iter().copied().collect();
    kept.sort_by_key(|&i| (entries[i].file_idx, i));

    let (mut kept, new_for_rep, merged_into) = assign_keys(&entries, &kept);

    let mut old_to_new: HashMap<String, String> = HashMap::new();
    let mut new_key_year: HashMap<String, i32> = HashMap::new();

    for (old, rep) in &rep_for_old {
        let rep = merged_into.get(rep).unwrap_or(rep);
        if let Some(new_key) = new_for_rep.get(rep) {
            old_to_new.insert(old.clone(), new_key.clone());
        }
    }

    for (&rep, new_key) in &new_for_rep {
        new_key_year.insert(
            new_key.clone(),
            entry_year_i32(&entries[rep]).unwrap_or(9999),
        );
    }

    let new_tex_files: Vec<_> = tex_files
        .iter()
        .map(|tf| TexFile {
            path: tf.path.clone(),
            content: rewrite_tex_citations(&tf.content, &old_to_new, &new_key_year),
        })
        .collect();

    // Order entries by first citation in the rewritten document. rev() so the first
    // occurrence wins; uncited entries stay at the end in their old order (stable sort).
    let first_cite: HashMap<String, usize> = citation_order(&new_tex_files, root_dir)
        .into_iter()
        .enumerate()
        .rev()
        .map(|(pos, key)| (key, pos))
        .collect();
    kept.sort_by_key(|i| first_cite.get(&new_for_rep[i]).copied().unwrap_or(usize::MAX));

    let new_bibs = render_bib_files(&bib_files, &entries, &kept, &new_for_rep);

    for (path, content) in new_bibs {
        common::write_with_backup(&path, &content)?;
    }

    for tf in new_tex_files {
        common::write_with_backup(&tf.path, &tf.content)?;
    }

    eprintln!("Found bibliography files:");
    for bf in &bib_files {
        eprintln!("  {}", bf.path.display());
    }

    eprintln!();
    eprintln!("Scanned tex files:");
    for tf in &tex_files {
        eprintln!("  {}", tf.path.display());
    }

    eprintln!();
    eprintln!("Original entries: {}", entries.len());
    eprintln!("Used citation keys in tex files: {}", used_keys.len());
    eprintln!(
        "Kept entries after duplicate/unused removal: {}",
        kept.len()
    );
    eprintln!(
        "Removed entries: {}",
        entries.len().saturating_sub(kept.len())
    );

    eprintln!();
    eprintln!("Citation key rewrites:");
    let mut rewrites: Vec<_> = old_to_new.iter().collect();
    rewrites.sort_by(|a, b| a.0.cmp(b.0));
    for (old, new) in rewrites.iter().take(30) {
        if old != new {
            eprintln!("  {} -> {}", old, new);
        }
    }
    if rewrites.len() > 30 {
        eprintln!("  ...");
    }

    let missing: Vec<_> = used_keys
        .iter()
        .filter(|k| *k != "*" && !old_to_new.contains_key(*k))
        .collect();

    if !missing.is_empty() {
        eprintln!();
        eprintln!("Warning: cited keys not found in bib file:");
        for k in missing {
            eprintln!("  {}", k);
        }
    }

    eprintln!();
    eprintln!("Wrote updated files. Backups have suffix .bak.");

    Ok(())
}

fn collect_tex_files(root_path: &Path) -> std::io::Result<Vec<TexFile>> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let root_dir = root_path.parent().unwrap_or_else(|| Path::new("."));
    collect_tex_files_inner(root_path, root_dir, &mut seen, &mut out)?;
    Ok(out)
}

fn collect_tex_files_inner(
    path: &Path,
    root_dir: &Path,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<TexFile>,
) -> std::io::Result<()> {
    let seen_path = path_identity(path);

    if !seen.insert(seen_path) {
        return Ok(());
    }

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) if !out.is_empty() => {
            eprintln!("Warning: skipping {}: {}", path.display(), e);
            return Ok(());
        }
        Err(e) => return Err(e),
    };

    let included_paths = find_include_tex_files(&content, path, root_dir);

    out.push(TexFile {
        path: path.to_path_buf(),
        content,
    });

    for included_path in included_paths {
        collect_tex_files_inner(&included_path, root_dir, seen, out)?;
    }

    Ok(())
}

fn find_bib_files(tex_files: &[TexFile], root_dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for tex_file in tex_files {
        let base_dir = tex_file.path.parent().unwrap_or_else(|| Path::new("."));

        for content in find_command_brace_args(&tex_file.content, "bibliography") {
            for item in content.split(',') {
                let name = item.trim();
                if name.is_empty() {
                    continue;
                }
                let p = resolve_path(name, "bib", root_dir, base_dir);
                if seen.insert(path_identity(&p)) {
                    out.push(p);
                }
            }
        }

        for content in find_command_brace_args(&tex_file.content, "addbibresource") {
            let name = content.trim();
            if name.is_empty() {
                continue;
            }
            let p = resolve_path(name, "bib", root_dir, base_dir);
            if seen.insert(path_identity(&p)) {
                out.push(p);
            }
        }
    }

    out
}

const INCLUDE_CMDS: [&str; 3] = ["include", "input", "subfile"];

fn find_include_tex_files(tex: &str, tex_path: &Path, root_dir: &Path) -> Vec<PathBuf> {
    let base_dir = tex_path.parent().unwrap_or_else(|| Path::new("."));
    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for cmd in INCLUDE_CMDS {
        for content in find_command_brace_args(tex, cmd) {
            let name = content.trim();
            if name.is_empty() {
                continue;
            }
            let p = resolve_path(name, "tex", root_dir, base_dir);
            if seen.insert(path_identity(&p)) {
                out.push(p);
            }
        }
    }

    out
}

// LaTeX resolves paths against the main file's directory (the compile dir), not the
// including file's. Fall back to the including file's dir for subfiles/import layouts.
fn resolve_path(name: &str, ext: &str, root_dir: &Path, file_dir: &Path) -> PathBuf {
    let with_ext = |dir: &Path| {
        let mut p = dir.join(name);
        if p.extension().is_none() {
            p.set_extension(ext);
        }
        p
    };
    let from_root = with_ext(root_dir);
    let from_file = with_ext(file_dir);

    if !from_root.exists() && from_file.exists() {
        from_file
    } else {
        from_root
    }
}

fn path_identity(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `tex` with every comment blanked out by spaces, so byte offsets still match `tex`.
// ponytail: `%` inside \verb or verbatim environments is treated as a comment too.
fn mask_comments(tex: &str) -> String {
    let mut b = tex.as_bytes().to_vec();
    let mut i = 0;

    while i < b.len() {
        if b[i] == b'\\' {
            i += 1;
        } else if b[i] == b'%' {
            while i < b.len() && b[i] != b'\n' {
                b[i] = b' ';
                i += 1;
            }
        }
        i += 1;
    }

    String::from_utf8(b).expect("comments are blanked from '%' to newline, whole chars only")
}

fn find_command_brace_args(tex: &str, target: &str) -> Vec<String> {
    let tex = &mask_comments(tex);
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
            j = skip_ws(tex, j);
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

        j = skip_ws(tex, j);

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

        j = skip_ws(raw, j);

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

        j = skip_ws(raw, j);

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

/// Cited keys in document order (with repeats): \include, \input and \subfile are
/// descended into where they occur, starting at the main file `tex_files[0]`.
fn citation_order(tex_files: &[TexFile], root_dir: &Path) -> Vec<String> {
    let by_id: HashMap<PathBuf, &TexFile> = tex_files
        .iter()
        .map(|tf| (path_identity(&tf.path), tf))
        .collect();
    let mut out = Vec::new();

    if let Some(main) = tex_files.first() {
        citation_order_inner(main, &by_id, root_dir, &mut HashSet::new(), &mut out);
    }

    out
}

fn citation_order_inner(
    tf: &TexFile,
    by_id: &HashMap<PathBuf, &TexFile>,
    root_dir: &Path,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<String>,
) {
    if !seen.insert(path_identity(&tf.path)) {
        return;
    }

    let tex = &mask_comments(&tf.content);
    let base_dir = tf.path.parent().unwrap_or_else(|| Path::new("."));
    let b = tex.as_bytes();
    let mut i = 0;

    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }

        let accept = |c: &str| is_cite_cmd(c) || INCLUDE_CMDS.contains(&c);

        let Some((cmd, s, e)) = command_arg_at(tex, i, accept) else {
            i += 1;
            continue;
        };

        if is_cite_cmd(&cmd) {
            out.extend(
                tex[s..e]
                    .split(',')
                    .map(str::trim)
                    .filter(|k| !k.is_empty())
                    .map(String::from),
            );
        } else {
            let p = resolve_path(tex[s..e].trim(), "tex", root_dir, base_dir);
            if let Some(child) = by_id.get(&path_identity(&p)) {
                citation_order_inner(child, by_id, root_dir, seen, out);
            }
        }

        i = e + 1;
    }
}

fn rewrite_tex_citations(
    tex: &str,
    old_to_new: &HashMap<String, String>,
    new_key_year: &HashMap<String, i32>,
) -> String {
    let mut out = String::new();
    let code = mask_comments(tex);
    let b = code.as_bytes();
    let mut i = 0;
    let mut last = 0;

    while i < b.len() {
        if b[i] != b'\\' {
            i += 1;
            continue;
        }

        if let Some((cmd, s, e)) = command_arg_at(&code, i, is_cite_cmd) {
            let old_group = &code[s..e];

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

/// `(command, arg_start, arg_end)` for a command at `pos` that `accept`s, skipping
/// a `*` and optional `[...]` arguments before its `{...}` argument.
fn command_arg_at(
    tex: &str,
    pos: usize,
    accept: fn(&str) -> bool,
) -> Option<(String, usize, usize)> {
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

    if !accept(&cmd) {
        return None;
    }

    if i < b.len() && b[i] == b'*' {
        i += 1;
    }

    loop {
        i = skip_ws(tex, i);

        if i < b.len() && b[i] == b'[' {
            let close = find_matching(tex, i, b'[', b']')?;
            i = close + 1;
        } else {
            break;
        }
    }

    i = skip_ws(tex, i);

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
    let mut out = Vec::new();

    for (file_idx, bf) in bib_files.iter().enumerate() {
        let mut s = String::new();

        for sp in &bf.specials {
            s.push_str(sp.trim_end());
            s.push_str("\n\n");
        }

        for &i in kept {
            let e = &entries[i];

            if e.file_idx != file_idx {
                continue;
            }

            let new_key = &new_for_rep[&i];

            s.push('@');
            s.push_str(&e.kind);
            s.push('{');
            s.push_str(new_key);
            s.push_str(",\n");
            s.push_str(&format_bib_body(&e.body));
            s.push_str("}\n\n");
        }

        out.push((bf.path.clone(), s));
    }

    out
}

const BIB_INDENT: &str = "  ";

/// One field per line as `name = value`, indented by `BIB_INDENT`; continuation
/// lines of multi-line values get one extra level.
fn format_bib_body(body: &str) -> String {
    let mut fields = Vec::new();
    let mut rest = body;

    loop {
        let (field, tail) = match find_top_level_comma(rest) {
            Some(c) => (&rest[..c], Some(&rest[c + 1..])),
            None => (rest, None),
        };

        let field = field.trim();
        if !field.is_empty() {
            fields.push(format_bib_field(field));
        }

        match tail {
            Some(t) => rest = t,
            None => break,
        }
    }

    let mut out = fields.join(",\n");
    out.push('\n');
    out
}

fn format_bib_field(field: &str) -> String {
    let field = match field.split_once('=') {
        Some((name, value)) if is_field_name(name.trim()) => {
            format!("{} = {}", name.trim(), value.trim())
        }
        _ => field.to_string(),
    };

    field
        .lines()
        .enumerate()
        .map(|(n, line)| match (n, line.trim()) {
            (_, "") => String::new(),
            (0, t) => format!("{BIB_INDENT}{t}"),
            (_, t) => format!("{BIB_INDENT}{BIB_INDENT}{t}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_field_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

/// Final dedup pass: run after the keys are normalized, so entries that only
/// differ in spelling (author order, punctuation, casing) and therefore end up
/// with the same key and the same title collapse into one.
fn assign_keys(
    entries: &[BibEntry],
    kept: &[usize],
) -> (Vec<usize>, HashMap<usize, String>, HashMap<usize, usize>) {
    let mut new_for_rep: HashMap<usize, String> = HashMap::new();
    let mut merged_into: HashMap<usize, usize> = HashMap::new();
    let mut used_new_keys: HashSet<String> = HashSet::new();
    let mut by_base: HashMap<String, Vec<usize>> = HashMap::new();
    let mut out = Vec::new();

    for &i in kept {
        let base = make_new_key(&entries[i]);

        let dup = by_base.get(&base).and_then(|group| {
            group
                .iter()
                .copied()
                .find(|&j| same_title(&entries[j], &entries[i]))
        });

        if let Some(first) = dup {
            merged_into.insert(i, first);
            continue;
        }

        by_base.entry(base.clone()).or_default().push(i);
        new_for_rep.insert(i, unique_key(base, &mut used_new_keys));
        out.push(i);
    }

    (out, new_for_rep, merged_into)
}

// ponytail: same normalized title is enough to call it the same work here, the
// key already pins author and year. Compare more fields (doi, pages) if that
// ever merges two distinct papers.
fn same_title(a: &BibEntry, b: &BibEntry) -> bool {
    let ta = a.fields.get("title").map(|s| latex_plain(s)).unwrap_or_default();
    let tb = b.fields.get("title").map(|s| latex_plain(s)).unwrap_or_default();

    !ta.is_empty() && ta == tb
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

        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if let Some(folded) = ascii_fold_char(c) {
            out.push_str(folded);
        } else if c.is_alphanumeric() {
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

fn ascii_fold_char(c: char) -> Option<&'static str> {
    match c {
        '\u{0300}'..='\u{036f}' | '\u{1ab0}'..='\u{1aff}' | '\u{1dc0}'..='\u{1dff}'
        | '\u{20d0}'..='\u{20ff}' | '\u{fe20}'..='\u{fe2f}' => Some(""),
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' | 'Ǎ' | 'Ǟ' | 'Ǡ' | 'Ǻ'
        | 'Ȁ' | 'Ȃ' | 'Ȧ' | 'Ḁ' | 'Ạ' | 'Ả' | 'Ấ' | 'Ầ' | 'Ẩ' | 'Ẫ' | 'Ậ' | 'Ắ'
        | 'Ằ' | 'Ẳ' | 'Ẵ' | 'Ặ' => Some("a"),
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' | 'ǎ' | 'ǟ' | 'ǡ' | 'ǻ'
        | 'ȁ' | 'ȃ' | 'ȧ' | 'ḁ' | 'ạ' | 'ả' | 'ấ' | 'ầ' | 'ẩ' | 'ẫ' | 'ậ' | 'ắ'
        | 'ằ' | 'ẳ' | 'ẵ' | 'ặ' => Some("a"),
        'Æ' | 'Ǣ' | 'Ǽ' => Some("ae"),
        'æ' | 'ǣ' | 'ǽ' => Some("ae"),
        'Ḃ' | 'Ḅ' | 'Ḇ' => Some("b"),
        'ḃ' | 'ḅ' | 'ḇ' => Some("b"),
        'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' | 'Ḉ' => Some("c"),
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' | 'ḉ' => Some("c"),
        'Ð' | 'Ď' | 'Đ' | 'Ḋ' | 'Ḍ' | 'Ḏ' | 'Ḑ' | 'Ḓ' => Some("d"),
        'ð' | 'ď' | 'đ' | 'ḋ' | 'ḍ' | 'ḏ' | 'ḑ' | 'ḓ' => Some("d"),
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' | 'Ȅ' | 'Ȇ' | 'Ȩ' | 'Ḕ'
        | 'Ḗ' | 'Ḙ' | 'Ḛ' | 'Ḝ' | 'Ẹ' | 'Ẻ' | 'Ẽ' | 'Ế' | 'Ề' | 'Ể' | 'Ễ' | 'Ệ' => {
            Some("e")
        }
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' | 'ȅ' | 'ȇ' | 'ȩ' | 'ḕ'
        | 'ḗ' | 'ḙ' | 'ḛ' | 'ḝ' | 'ẹ' | 'ẻ' | 'ẽ' | 'ế' | 'ề' | 'ể' | 'ễ' | 'ệ' => {
            Some("e")
        }
        'Ḟ' => Some("f"),
        'ḟ' => Some("f"),
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' | 'Ǧ' | 'Ǵ' | 'Ḡ' => Some("g"),
        'ĝ' | 'ğ' | 'ġ' | 'ģ' | 'ǧ' | 'ǵ' | 'ḡ' => Some("g"),
        'Ĥ' | 'Ħ' | 'Ȟ' | 'Ḣ' | 'Ḥ' | 'Ḧ' | 'Ḩ' | 'Ḫ' => Some("h"),
        'ĥ' | 'ħ' | 'ȟ' | 'ḣ' | 'ḥ' | 'ḧ' | 'ḩ' | 'ḫ' => Some("h"),
        'Ì' | 'Í' | 'Î' | 'Ï' | 'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' | 'Ǐ' | 'Ȉ' | 'Ȋ' | 'Ḭ'
        | 'Ḯ' | 'Ỉ' | 'Ị' => Some("i"),
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' | 'ǐ' | 'ȉ' | 'ȋ' | 'ḭ'
        | 'ḯ' | 'ỉ' | 'ị' => Some("i"),
        'Ĵ' => Some("j"),
        'ĵ' => Some("j"),
        'Ķ' | 'Ǩ' | 'Ḱ' | 'Ḳ' | 'Ḵ' => Some("k"),
        'ķ' | 'ǩ' | 'ḱ' | 'ḳ' | 'ḵ' => Some("k"),
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ŀ' | 'Ł' | 'Ḷ' | 'Ḹ' | 'Ḻ' | 'Ḽ' => Some("l"),
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' | 'ḷ' | 'ḹ' | 'ḻ' | 'ḽ' => Some("l"),
        'Ḿ' | 'Ṁ' | 'Ṃ' => Some("m"),
        'ḿ' | 'ṁ' | 'ṃ' => Some("m"),
        'Ñ' | 'Ń' | 'Ņ' | 'Ň' | 'Ǹ' | 'Ṅ' | 'Ṇ' | 'Ṉ' | 'Ṋ' => Some("n"),
        'ñ' | 'ń' | 'ņ' | 'ň' | 'ǹ' | 'ṅ' | 'ṇ' | 'ṉ' | 'ṋ' => Some("n"),
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' | 'Ơ' | 'Ǒ' | 'Ǫ' | 'Ǭ'
        | 'Ȍ' | 'Ȏ' | 'Ȫ' | 'Ȭ' | 'Ȯ' | 'Ȱ' | 'Ṍ' | 'Ṏ' | 'Ṑ' | 'Ṓ' | 'Ọ' | 'Ỏ'
        | 'Ố' | 'Ồ' | 'Ổ' | 'Ỗ' | 'Ộ' | 'Ớ' | 'Ờ' | 'Ở' | 'Ỡ' | 'Ợ' => Some("o"),
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' | 'ơ' | 'ǒ' | 'ǫ' | 'ǭ'
        | 'ȍ' | 'ȏ' | 'ȫ' | 'ȭ' | 'ȯ' | 'ȱ' | 'ṍ' | 'ṏ' | 'ṑ' | 'ṓ' | 'ọ' | 'ỏ'
        | 'ố' | 'ồ' | 'ổ' | 'ỗ' | 'ộ' | 'ớ' | 'ờ' | 'ở' | 'ỡ' | 'ợ' => Some("o"),
        'Œ' => Some("oe"),
        'œ' => Some("oe"),
        'Ṕ' | 'Ṗ' => Some("p"),
        'ṕ' | 'ṗ' => Some("p"),
        'Ŕ' | 'Ŗ' | 'Ř' | 'Ȑ' | 'Ȓ' | 'Ṙ' | 'Ṛ' | 'Ṝ' | 'Ṟ' => Some("r"),
        'ŕ' | 'ŗ' | 'ř' | 'ȑ' | 'ȓ' | 'ṙ' | 'ṛ' | 'ṝ' | 'ṟ' => Some("r"),
        'Ś' | 'Ŝ' | 'Ş' | 'Š' | 'Ș' | 'Ṡ' | 'Ṣ' | 'Ṥ' | 'Ṧ' | 'Ṩ' => Some("s"),
        'ś' | 'ŝ' | 'ş' | 'š' | 'ș' | 'ṡ' | 'ṣ' | 'ṥ' | 'ṧ' | 'ṩ' | 'ſ' => Some("s"),
        'ẞ' | 'ß' => Some("ss"),
        'Ţ' | 'Ť' | 'Ŧ' | 'Ț' | 'Ṫ' | 'Ṭ' | 'Ṯ' | 'Ṱ' => Some("t"),
        'ţ' | 'ť' | 'ŧ' | 'ț' | 'ṫ' | 'ṭ' | 'ṯ' | 'ṱ' => Some("t"),
        'Þ' | 'þ' => Some("th"),
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' | 'Ư' | 'Ǔ' | 'Ǖ'
        | 'Ǘ' | 'Ǚ' | 'Ǜ' | 'Ȕ' | 'Ȗ' | 'Ṳ' | 'Ṵ' | 'Ṷ' | 'Ṹ' | 'Ṻ' | 'Ụ' | 'Ủ'
        | 'Ứ' | 'Ừ' | 'Ử' | 'Ữ' | 'Ự' => Some("u"),
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' | 'ư' | 'ǔ' | 'ǖ'
        | 'ǘ' | 'ǚ' | 'ǜ' | 'ȕ' | 'ȗ' | 'ṳ' | 'ṵ' | 'ṷ' | 'ṹ' | 'ṻ' | 'ụ' | 'ủ'
        | 'ứ' | 'ừ' | 'ử' | 'ữ' | 'ự' => Some("u"),
        'Ṽ' | 'Ṿ' => Some("v"),
        'ṽ' | 'ṿ' => Some("v"),
        'Ŵ' | 'Ẁ' | 'Ẃ' | 'Ẅ' | 'Ẇ' | 'Ẉ' => Some("w"),
        'ŵ' | 'ẁ' | 'ẃ' | 'ẅ' | 'ẇ' | 'ẉ' => Some("w"),
        'Ẋ' | 'Ẍ' => Some("x"),
        'ẋ' | 'ẍ' => Some("x"),
        'Ý' | 'Ŷ' | 'Ÿ' | 'Ȳ' | 'Ẏ' | 'Ỳ' | 'Ỵ' | 'Ỷ' | 'Ỹ' => Some("y"),
        'ý' | 'ÿ' | 'ŷ' | 'ȳ' | 'ẏ' | 'ỳ' | 'ỵ' | 'ỷ' | 'ỹ' => Some("y"),
        'Ź' | 'Ż' | 'Ž' | 'Ẑ' | 'Ẓ' | 'Ẕ' => Some("z"),
        'ź' | 'ż' | 'ž' | 'ẑ' | 'ẓ' | 'ẕ' => Some("z"),
        _ => None,
    }
}

fn normalize_spaces(s: &str) -> String {
    collapse_whitespace(s)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: &str, author: &str, title: &str, year: &str) -> BibEntry {
        let mut fields = HashMap::new();
        fields.insert("author".to_string(), author.to_string());
        fields.insert("title".to_string(), title.to_string());
        fields.insert("year".to_string(), year.to_string());

        BibEntry {
            file_idx: 0,
            kind: "article".to_string(),
            old_key: key.to_string(),
            body: String::new(),
            fields,
        }
    }

    #[test]
    fn input_and_subfile_children_are_followed() {
        let tex = "\\input{figures/pipeline}\n\\subfile{chap}\n\\includegraphics{img.png}\n";
        let found = find_include_tex_files(tex, Path::new("main.tex"), Path::new(""));

        assert_eq!(
            found,
            vec![PathBuf::from("figures/pipeline.tex"), PathBuf::from("chap.tex")]
        );
    }

    #[test]
    fn commented_out_commands_are_ignored() {
        let tex = "% \\input{gone}\n\\input{kept} % \\cite{b}\n50\\% \\cite{a,% old\n c}\n";

        let found = find_include_tex_files(tex, Path::new("main.tex"), Path::new(""));
        assert_eq!(found, vec![PathBuf::from("kept.tex")]);

        let main = TexFile {
            path: PathBuf::from("main.tex"),
            content: tex.to_string(),
        };
        assert_eq!(citation_order(&[main], Path::new("")), vec!["a", "c"]);

        let map = HashMap::from([("a".to_string(), "x".to_string()), ("b".to_string(), "y".to_string())]);
        let out = rewrite_tex_citations(tex, &map, &HashMap::new());
        assert_eq!(out, "% \\input{gone}\n\\input{kept} % \\cite{b}\n50\\% \\cite{x,c}\n");
    }

    #[test]
    fn citations_follow_inputs_where_they_occur() {
        let tex = |path: &str, content: &str| TexFile {
            path: PathBuf::from(path),
            content: content.to_string(),
        };
        let files = [
            tex("main.tex", "\\cite{a}\n\\input{chap}\n\\cite[p.~1]{c, a}\n"),
            tex("chap.tex", "\\citep{b}"),
        ];

        assert_eq!(citation_order(&files, Path::new("")), vec!["a", "b", "c", "a"]);
    }

    #[test]
    fn nested_inputs_resolve_against_main_dir() {
        let dir = std::env::temp_dir().join(format!("fixbib-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("b")).unwrap();
        std::fs::create_dir_all(dir.join("c")).unwrap();
        std::fs::write(dir.join("b/a.tex"), "").unwrap();

        let found = find_include_tex_files("\\input{b/a}", &dir.join("c/a.tex"), &dir);
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(found, vec![dir.join("b/a.tex")]);
    }

    #[test]
    fn bib_fields_are_reindented_uniformly() {
        let body = "\n\tauthor={Smith, John and\n               Doe, Jane},\n      title =   \"Quantum, Things\",  year=2020,\n abstract = {One.\n\n      Two.},\n\n  ";

        assert_eq!(
            format_bib_body(body),
            "  author = {Smith, John and\n    Doe, Jane},\n  title = \"Quantum, Things\",\n  year = 2020,\n  abstract = {One.\n\n    Two.}\n"
        );
    }

    #[test]
    fn duplicates_are_merged_after_keys_are_normalized() {
        let entries = vec![
            entry("a", "Smith, John", "Quantum Things", "2020"),
            entry("b", "John Smith", "Quantum things.", "2020"),
            entry("c", "Smith, John", "Quantum Widgets", "2020"),
        ];

        let (kept, new_for_rep, merged_into) = assign_keys(&entries, &[0, 1, 2]);

        assert_eq!(kept, vec![0, 2]);
        assert_eq!(merged_into.get(&1), Some(&0));
        assert_eq!(new_for_rep[&0], "smith_quantum_2020");
        assert_eq!(new_for_rep[&2], "smith_quantum_2020_2");
    }
}

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

pub fn input_path_arg(usage: &str) -> PathBuf {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "program".to_string());
    let mut positional = args.filter(|a| !a.starts_with("--"));
    let Some(input_path) = positional.next() else {
        eprintln!("Usage: {program} {usage}");
        std::process::exit(2);
    };

    if positional.next().is_some() {
        eprintln!("Usage: {program} {usage}");
        std::process::exit(2);
    }

    PathBuf::from(input_path)
}

pub fn has_flag(name: &str) -> bool {
    env::args().any(|a| a == name)
}

pub fn write_with_backup(path: &Path, content: &str) -> io::Result<()> {
    let backup = backup_path(path);

    if path.exists() {
        fs::copy(path, &backup)?;
    }

    fs::write(path, content)
}

pub fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn skip_ws(s: &str, mut idx: usize) -> usize {
    while idx < s.len() {
        let c = s[idx..].chars().next().unwrap();

        if c.is_whitespace() {
            idx += c.len_utf8();
        } else {
            break;
        }
    }

    idx
}

fn backup_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "backup".to_string());

    path.with_file_name(format!("{file_name}.bak"))
}

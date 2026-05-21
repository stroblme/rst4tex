use std::{
    env, fs,
    io::{self, Read},
};

#[derive(Clone, Copy)]
enum MathMode {
    Bracket,
    Dollars,
}

fn main() -> io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() > 3 {
        eprintln!("Usage: {} [input.tex|-] [output.tex]", args[0]);
        std::process::exit(2);
    }

    let input = if args.len() >= 2 && args[1] != "-" {
        fs::read_to_string(&args[1])?
    } else {
        let mut s = String::new();
        io::stdin().read_to_string(&mut s)?;
        s
    };

    let output = process(&input);

    if args.len() == 3 {
        fs::write(&args[2], output)?;
    } else if args.len() == 2 && args[1] != "-" {
        fs::write(&args[1], output)?;
    } else {
        print!("{output}");
    }

    Ok(())
}

fn process(input: &str) -> String {
    let mut out = String::new();
    let mut para: Vec<String> = Vec::new();
    let mut ignored_env: Option<String> = None;
    let mut display_math: Option<MathMode> = None;

    for line in input.lines() {
        let trimmed = line.trim_start();

        if let Some(env) = ignored_env.clone() {
            append_line(&mut out, line);
            if line_contains_end_env(line, &env) {
                ignored_env = None;
            }
            continue;
        }

        if let Some(mode) = display_math {
            append_line(&mut out, line);
            if display_math_ends(line, mode) {
                display_math = None;
            }
            continue;
        }

        if trimmed.is_empty() {
            flush_paragraph(&mut out, &mut para);
            append_line(&mut out, line);
            continue;
        }

        if trimmed.starts_with('%') || contains_unescaped_percent(line) {
            flush_paragraph(&mut out, &mut para);
            append_line(&mut out, line);
            continue;
        }

        if let Some(mode) = display_math_starts(trimmed) {
            flush_paragraph(&mut out, &mut para);
            append_line(&mut out, line);
            if !display_math_ends_after_start(trimmed, mode) {
                display_math = Some(mode);
            }
            continue;
        }

        if let Some(env) = begin_env_at_start(trimmed) {
            flush_paragraph(&mut out, &mut para);
            append_line(&mut out, line);

            if is_ignored_env(&env) && !line_contains_end_env(line, &env) {
                ignored_env = Some(env);
            }

            continue;
        }

        if end_env_at_start(trimmed).is_some() {
            flush_paragraph(&mut out, &mut para);
            append_line(&mut out, line);
            continue;
        }

        if starts_supported_paragraph_prefix(trimmed) && !para.is_empty() {
            flush_paragraph(&mut out, &mut para);
        }

        if is_command_barrier(line) {
            flush_paragraph(&mut out, &mut para);
            append_line(&mut out, line);
            continue;
        }

        para.push(line.to_string());
    }

    flush_paragraph(&mut out, &mut para);
    out
}

fn flush_paragraph(out: &mut String, para: &mut Vec<String>) {
    if para.is_empty() {
        return;
    }

    let indent = leading_ws(&para[0]).to_string();

    let mut pieces: Vec<String> = para
        .iter()
        .map(|line| {
            if line.starts_with(&indent) {
                line[indent.len()..].trim().to_string()
            } else {
                line.trim().to_string()
            }
        })
        .filter(|s| !s.is_empty())
        .collect();

    if pieces.is_empty() {
        para.clear();
        return;
    }

    let mut first_prefix = String::new();
    let mut continuation_prefix = String::new();

    if let Some((prefix, continuation, rest)) = take_paragraph_prefix(&pieces[0]) {
        first_prefix = prefix;
        continuation_prefix = continuation;
        pieces[0] = rest.trim_start().to_string();
    }

    let joined = collapse_whitespace(&pieces.join(" "));
    let sentences = split_sentences(&joined);

    for (i, sentence) in sentences.iter().enumerate() {
        out.push_str(&indent);

        if i == 0 {
            out.push_str(&first_prefix);
        } else {
            out.push_str(&continuation_prefix);
        }

        out.push_str(sentence.trim());
        out.push('\n');
    }

    para.clear();
}

fn split_sentences(s: &str) -> Vec<String> {
    let chars: Vec<(usize, char)> = s.char_indices().collect();
    let mut result = Vec::new();

    let mut start = 0usize;
    let mut i = 0usize;

    while i < chars.len() {
        let (byte_idx, ch) = chars[i];

        if matches!(ch, '.' | '!' | '?') {
            if ch == '.' && is_decimal_point(&chars, i) {
                i += 1;
                continue;
            }

            if ch == '.' && is_abbreviation(s, byte_idx) {
                i += 1;
                continue;
            }

            let mut j = i + 1;
            let mut end = byte_idx + ch.len_utf8();

            while j < chars.len() && is_sentence_closer(chars[j].1) {
                end = chars[j].0 + chars[j].1.len_utf8();
                j += 1;
            }

            if j == chars.len() {
                let piece = s[start..end].trim();
                if !piece.is_empty() {
                    result.push(piece.to_string());
                }
                start = s.len();
                break;
            }

            if chars[j].1.is_whitespace() {
                let mut next = j;
                while next < chars.len() && chars[next].1.is_whitespace() {
                    next += 1;
                }

                if next == chars.len() {
                    let piece = s[start..end].trim();
                    if !piece.is_empty() {
                        result.push(piece.to_string());
                    }
                    start = s.len();
                    break;
                }

                let next_ch = chars[next].1;
                let should_split = ch != '.' || !next_ch.is_lowercase();

                if should_split {
                    let piece = s[start..end].trim();
                    if !piece.is_empty() {
                        result.push(piece.to_string());
                    }
                    start = chars[next].0;
                    i = next;
                    continue;
                }
            }
        }

        i += 1;
    }

    if start < s.len() {
        let piece = s[start..].trim();
        if !piece.is_empty() {
            result.push(piece.to_string());
        }
    }

    result
}

fn is_decimal_point(chars: &[(usize, char)], i: usize) -> bool {
    if i == 0 || i + 1 >= chars.len() {
        return false;
    }

    chars[i - 1].1.is_ascii_digit() && chars[i + 1].1.is_ascii_digit()
}

fn is_abbreviation(s: &str, dot_byte: usize) -> bool {
    let mut prefix = s[..dot_byte].trim_end();

    prefix = prefix.trim_end_matches(|c: char| {
        matches!(c, ')' | ']' | '}' | '"' | '\'' | '’' | '”')
    });

    let lower = prefix.to_lowercase();

    if lower.ends_with("et al") {
        return true;
    }

    let token = prefix
        .split_whitespace()
        .last()
        .unwrap_or("")
        .trim_matches(|c: char| {
            c.is_ascii_punctuation() && c != '.'
        });

    let token_lower = token.trim_end_matches('.').to_lowercase();

    const ABBREVS: &[&str] = &[
        "e.g", "i.e", "cf", "vs", "etc", "fig", "figs", "eq", "eqs", "sec", "secs",
        "ch", "chap", "app", "ref", "refs", "no", "nos", "dr", "mr", "mrs", "ms",
        "prof", "inc", "ltd", "jr", "sr",
    ];

    if ABBREVS.contains(&token_lower.as_str()) {
        return true;
    }

    // Catches U.S., U.K., Ph.D., etc.
    if token.contains('.') {
        return true;
    }

    // Catches initials like "A. Einstein".
    let letters: String = token.chars().filter(|c| c.is_alphabetic()).collect();
    letters.chars().count() == 1
        && letters
            .chars()
            .next()
            .map(|c| c.is_uppercase())
            .unwrap_or(false)
}

fn is_sentence_closer(c: char) -> bool {
    matches!(
        c,
        ')' | ']' | '}' | '"' | '\'' | '’' | '”' | '»'
    )
}

fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn leading_ws(s: &str) -> &str {
    let n = s
        .char_indices()
        .find(|(_, c)| !c.is_whitespace())
        .map(|(i, _)| i)
        .unwrap_or(s.len());

    &s[..n]
}

fn append_line(out: &mut String, line: &str) {
    out.push_str(line);
    out.push('\n');
}

fn begin_env_at_start(s: &str) -> Option<String> {
    let rest = s.strip_prefix(r"\begin{")?;
    let end = rest.find('}')?;
    Some(rest[..end].to_string())
}

fn end_env_at_start(s: &str) -> Option<String> {
    let rest = s.strip_prefix(r"\end{")?;
    let end = rest.find('}')?;
    Some(rest[..end].to_string())
}

fn line_contains_end_env(line: &str, env: &str) -> bool {
    line.contains(&format!(r"\end{{{env}}}"))
}

fn is_ignored_env(env: &str) -> bool {
    matches!(
        env,
        "figure"
            | "figure*"
            | "table"
            | "table*"
            | "equation"
            | "equation*"
            | "align"
            | "align*"
            | "alignat"
            | "alignat*"
            | "gather"
            | "gather*"
            | "multline"
            | "multline*"
            | "flalign"
            | "flalign*"
            | "split"
            | "cases"
            | "array"
            | "tabular"
            | "tabular*"
            | "tabularx"
            | "longtable"
            | "tikzpicture"
            | "picture"
            | "verbatim"
            | "lstlisting"
            | "minted"
            | "algorithm"
            | "algorithmic"
            | "algorithm2e"
            | "thebibliography"
    )
}

fn display_math_starts(s: &str) -> Option<MathMode> {
    if s.starts_with(r"\[") {
        Some(MathMode::Bracket)
    } else if s.starts_with("$$") {
        Some(MathMode::Dollars)
    } else {
        None
    }
}

fn display_math_ends(line: &str, mode: MathMode) -> bool {
    match mode {
        MathMode::Bracket => line.contains(r"\]"),
        MathMode::Dollars => line.contains("$$"),
    }
}

fn display_math_ends_after_start(line: &str, mode: MathMode) -> bool {
    match mode {
        MathMode::Bracket => line.get(2..).unwrap_or("").contains(r"\]"),
        MathMode::Dollars => line.get(2..).unwrap_or("").contains("$$"),
    }
}

fn contains_unescaped_percent(line: &str) -> bool {
    let mut backslashes = 0usize;

    for c in line.chars() {
        if c == '\\' {
            backslashes += 1;
        } else {
            if c == '%' && backslashes % 2 == 0 {
                return true;
            }
            backslashes = 0;
        }
    }

    false
}

fn is_command_barrier(line: &str) -> bool {
    let s = line.trim_start();

    if s.ends_with(r"\\") {
        return true;
    }

    if starts_supported_paragraph_prefix(s) {
        return false;
    }

    s.starts_with('\\')
}

fn starts_supported_paragraph_prefix(s: &str) -> bool {
    starts_command(s, r"\item") || starts_command(s, r"\noindent")
}

fn starts_command(s: &str, command: &str) -> bool {
    if !s.starts_with(command) {
        return false;
    }

    match s[command.len()..].chars().next() {
        None => true,
        Some(c) => !c.is_alphabetic(),
    }
}

fn take_paragraph_prefix(s: &str) -> Option<(String, String, String)> {
    if starts_command(s, r"\item") {
        let mut idx = r"\item".len();
        idx = skip_ws(s, idx);

        if s[idx..].starts_with('[') {
            let mut depth = 0usize;

            for (off, c) in s[idx..].char_indices() {
                if c == '[' {
                    depth += 1;
                } else if c == ']' {
                    depth -= 1;
                    if depth == 0 {
                        idx += off + c.len_utf8();
                        break;
                    }
                }
            }

            idx = skip_ws(s, idx);
        }

        let prefix = s[..idx].to_string();
        let continuation = " ".repeat(prefix.chars().count());
        let rest = s[idx..].to_string();

        return Some((prefix, continuation, rest));
    }

    if starts_command(s, r"\noindent") {
        let mut idx = r"\noindent".len();
        idx = skip_ws(s, idx);

        let prefix = s[..idx].to_string();
        let continuation = String::new();
        let rest = s[idx..].to_string();

        return Some((prefix, continuation, rest));
    }

    None
}

fn skip_ws(s: &str, mut idx: usize) -> usize {
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
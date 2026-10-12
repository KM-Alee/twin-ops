pub(super) fn logical_lines(content: &str) -> Result<Vec<String>, &'static str> {
    let mut lines = Vec::new();
    let mut pending = String::new();
    let mut continued = false;
    for line in content.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.starts_with(['#', ';']) {
            continue;
        }
        let slashes = line.chars().rev().take_while(|ch| *ch == '\\').count();
        let continuation = slashes % 2 == 1;
        pending.push_str(if continuation {
            &line[..line.len() - 1]
        } else {
            line
        });
        if continuation {
            pending.push(' ');
            continued = true;
        } else {
            lines.push(std::mem::take(&mut pending));
            continued = false;
        }
    }
    if continued {
        return Err("unterminated line continuation");
    }
    Ok(lines)
}

pub(super) fn words(value: &str) -> Result<Vec<String>, &'static str> {
    let mut chars = value.chars();
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut closed_quote = false;
    while let Some(ch) = chars.next() {
        if closed_quote && !ch.is_whitespace() {
            return Err("quoted item must end at whitespace");
        }
        if ch == '\\' {
            let escaped = chars.next().ok_or("unterminated escape")?;
            let decoded = match escaped {
                's' => ' ',
                't' => '\t',
                'n' => '\n',
                'r' => '\r',
                'b' => '\u{8}',
                'f' => '\u{c}',
                'v' => '\u{b}',
                'a' => '\u{7}',
                '\\' | '\'' | '"' => escaped,
                'x' | 'u' | 'U' => {
                    let count = match escaped {
                        'x' => 2,
                        'u' => 4,
                        _ => 8,
                    };
                    let mut digits = String::new();
                    for _ in 0..count {
                        digits.push(chars.next().ok_or("incomplete character escape")?);
                    }
                    let code =
                        u32::from_str_radix(&digits, 16).map_err(|_| "invalid character escape")?;
                    char::from_u32(code)
                        .filter(|ch| *ch != '\0')
                        .ok_or("invalid escaped character")?
                }
                _ => return Err("unsupported character escape"),
            };
            word.push(decoded);
            started = true;
        } else if let Some(open) = quote {
            if ch == open {
                quote = None;
                closed_quote = true;
            } else {
                word.push(ch);
            }
        } else if ch == '\'' || ch == '"' {
            if started {
                return Err("quote must start an item");
            }
            quote = Some(ch);
            started = true;
        } else if ch.is_whitespace() {
            if started {
                words.push(std::mem::take(&mut word));
                started = false;
                closed_quote = false;
            }
        } else {
            word.push(ch);
            started = true;
        }
    }
    if quote.is_some() {
        return Err("unterminated quoted item");
    }
    if started {
        words.push(word);
    }
    Ok(words)
}

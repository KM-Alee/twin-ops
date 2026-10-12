/// One simple command of a shell line: its words plus any files it writes through
/// output redirection. Pipelines and lists (`|`, `&&`, `||`, `;`, `&`) split commands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SimpleCommand {
    pub words: Vec<String>,
    pub redirect_targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Separator,
    Redirect,
}

pub(crate) fn split_commands(line: &str) -> Vec<SimpleCommand> {
    let mut commands = Vec::new();
    let mut current = SimpleCommand {
        words: Vec::new(),
        redirect_targets: Vec::new(),
    };
    let mut expect_target = false;
    for token in tokenize(line) {
        match token {
            Token::Separator => {
                expect_target = false;
                if !current.words.is_empty() || !current.redirect_targets.is_empty() {
                    commands.push(current);
                }
                current = SimpleCommand {
                    words: Vec::new(),
                    redirect_targets: Vec::new(),
                };
            }
            Token::Redirect => expect_target = true,
            Token::Word(word) if expect_target => {
                expect_target = false;
                current.redirect_targets.push(word);
            }
            Token::Word(word) => current.words.push(word),
        }
    }
    if !current.words.is_empty() || !current.redirect_targets.is_empty() {
        commands.push(current);
    }
    commands
}

fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut has_word = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\'' => {
                has_word = true;
                for inner in chars.by_ref() {
                    if inner == '\'' {
                        break;
                    }
                    word.push(inner);
                }
            }
            '"' => {
                has_word = true;
                while let Some(inner) = chars.next() {
                    match inner {
                        '"' => break,
                        '\\' => {
                            if let Some(escaped) = chars.next() {
                                word.push(escaped);
                            }
                        }
                        other => word.push(other),
                    }
                }
            }
            '\\' => {
                if let Some(escaped) = chars.next() {
                    if escaped != '\n' {
                        has_word = true;
                        word.push(escaped);
                    }
                }
            }
            '#' if !has_word => {
                for inner in chars.by_ref() {
                    if inner == '\n' {
                        break;
                    }
                }
                flush(&mut tokens, &mut word, &mut has_word);
                tokens.push(Token::Separator);
            }
            ' ' | '\t' => flush(&mut tokens, &mut word, &mut has_word),
            '\n' | ';' | '|' | '(' | ')' | '{' | '}' if !is_brace_word(ch, has_word) => {
                flush(&mut tokens, &mut word, &mut has_word);
                if ch == '|' && chars.peek() == Some(&'&') {
                    chars.next();
                }
                tokens.push(Token::Separator);
            }
            '&' => {
                if chars.peek() == Some(&'>') {
                    chars.next();
                    if chars.peek() == Some(&'>') {
                        chars.next();
                    }
                    flush(&mut tokens, &mut word, &mut has_word);
                    tokens.push(Token::Redirect);
                } else if has_word && word.ends_with('>') {
                    word.push('&');
                } else {
                    flush(&mut tokens, &mut word, &mut has_word);
                    tokens.push(Token::Separator);
                }
            }
            '>' => {
                let descriptor = has_word && word.chars().all(|c| c.is_ascii_digit());
                if has_word && !descriptor {
                    flush(&mut tokens, &mut word, &mut has_word);
                } else {
                    word.clear();
                    has_word = false;
                }
                if chars.peek() == Some(&'>') || chars.peek() == Some(&'|') {
                    chars.next();
                }
                if chars.peek() == Some(&'&') {
                    // `>&2` duplicates a descriptor; it never names a file.
                    chars.next();
                    while chars
                        .peek()
                        .is_some_and(|c| c.is_ascii_digit() || *c == '-')
                    {
                        chars.next();
                    }
                    continue;
                }
                tokens.push(Token::Redirect);
            }
            '<' => {
                flush(&mut tokens, &mut word, &mut has_word);
                // Input redirection reads a file; skip its operand.
                while chars.peek() == Some(&'<') {
                    chars.next();
                }
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                while chars.peek().is_some_and(|c| !c.is_whitespace()) {
                    chars.next();
                }
            }
            other => {
                has_word = true;
                word.push(other);
            }
        }
    }
    flush(&mut tokens, &mut word, &mut has_word);
    tokens
}

fn is_brace_word(ch: char, has_word: bool) -> bool {
    matches!(ch, '{' | '}') && has_word
}

fn flush(tokens: &mut Vec<Token>, word: &mut String, has_word: &mut bool) {
    if *has_word {
        tokens.push(Token::Word(std::mem::take(word)));
        *has_word = false;
    }
}

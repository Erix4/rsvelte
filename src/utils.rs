use std::fmt::Debug;

pub struct Coord {
    pub line: usize,
    pub col: usize,
}

pub fn read_until<F>(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    stop_cond: F,
    coord: &mut Coord,
) -> String
where
    F: Fn(char) -> bool,
{
    let mut result = String::new();
    while let Some(&ch) = chars.peek() {
        if stop_cond(ch) {
            break;
        }
        chars.next();
        update_coord(ch, coord);
        result.push(ch);
    }
    result
}

/// Reads characters until the target string is found (target is included)
pub fn read_until_string(chars: &mut std::iter::Peekable<std::str::Chars>, target: &str, coord: &mut Coord) -> String {
    let mut result = String::new();
    let target_len = target.len();
    let mut buffer = String::new();

    while let Some(&ch) = chars.peek() {
        buffer.push(ch);
        chars.next();
        if buffer.ends_with(target) {
            // Remove the target from the end of the result
            let len_to_remove = target_len;
            result.truncate(result.len().saturating_sub(len_to_remove));
            break;
        }
        update_coord(ch, coord);
        result.push(ch);

        // Keep buffer size manageable
        if buffer.len() > target_len {
            buffer.remove(0);
        }
    }
    
    result
}

pub fn update_coord(ch: char, coord: &mut Coord) {
    if ch == '\n' {
        coord.line += 1;
        coord.col = 0;
    } else {
        coord.col += 1;
    }
}

pub fn expect_next(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    expected: char,
    coord: &mut Coord,
) {
    match chars.next() {
        Some(ch) if ch == expected => {
            update_coord(ch, coord);
        }
        Some(ch) => panic!(
            "Expected '{}' at line {}, col {}, found '{}'",
            expected, coord.line, coord.col, ch
        ),
        None => panic!(
            "Expected '{}' at line {}, col {}, found end of input",
            expected, coord.line, coord.col
        ),
    }
}

pub enum CompileError {
    LocError { line: usize, col: usize, message: String },
    GenericError(String),
}

pub fn generic_error(message: &str) -> CompileError {
    CompileError::GenericError(message.to_string())
}

impl From<CompileError> for String {
    fn from(err: CompileError) -> Self {
        match err {
            CompileError::LocError { line, col, message } => {
                format!("Error at line {}, col {}: {}", line, col, message)
            }
            CompileError::GenericError(message) => message,
        }
    }
}

impl From<std::io::Error> for CompileError {
    fn from(err: std::io::Error) -> Self {
        CompileError::GenericError(err.to_string())
    }
}

impl From<syn::Error> for CompileError {
    fn from(err: syn::Error) -> Self {
        CompileError::GenericError(err.to_string())
    }
}

impl Debug for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CompileError::LocError { line, col, message } => {
                write!(f, "Error at line {}, col {}: {}", line, col, message)
            }
            CompileError::GenericError(message) => write!(f, "{}", message),
        }
    }
}

impl std::fmt::Display for CompileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
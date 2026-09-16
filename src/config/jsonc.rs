//! Span-aware JSONC parser: JSON plus `//` and `/* */` comments and trailing
//! commas. Strictly rejects JSON5-only syntax (unquoted keys, single quotes).

/// A JSONC value with a source line for diagnostics.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null {
        line: usize,
    },
    Bool {
        value: bool,
        line: usize,
    },
    Number {
        value: f64,
        line: usize,
    },
    String {
        value: String,
        line: usize,
    },
    Array {
        items: Vec<Value>,
        line: usize,
    },
    Object {
        members: Vec<(String, Value)>,
        line: usize,
    },
}

impl Value {
    pub const fn line(&self) -> usize {
        match self {
            Self::Null { line }
            | Self::Bool { line, .. }
            | Self::Number { line, .. }
            | Self::String { line, .. }
            | Self::Array { line, .. }
            | Self::Object { line, .. } => *line,
        }
    }

    pub fn as_object(&self) -> Option<&Vec<(String, Value)>> {
        match self {
            Self::Object { members, .. } => Some(members),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Self::Number { value, .. } if *value >= 0.0 && value.fract() == 0.0 => {
                Some(*value as u64)
            }
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String { value, .. } => Some(value),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool { value, .. } => Some(*value),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array { items, .. } => Some(items.as_slice()),
            _ => None,
        }
    }
}

/// Parse JSONC. Returns `(line, column, message)` on error, 1-based.
pub fn parse(source: &str) -> Result<Value, (usize, usize, String)> {
    let mut parser = Parser {
        bytes: source.as_bytes(),
        pos: 0,
        line: 1,
        line_start: 0,
    };
    parser.skip_ws();
    let value = parser.value()?;
    parser.skip_ws();
    if parser.pos < parser.bytes.len() {
        return Err(parser.error("unexpected trailing content"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    line_start: usize,
}

impl Parser<'_> {
    fn error(&self, message: impl Into<String>) -> (usize, usize, String) {
        (self.line, self.pos - self.line_start + 1, message.into())
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.line_start = self.pos;
        }
        Some(b)
    }

    fn skip_ws(&mut self) {
        loop {
            match self.peek() {
                Some(b' ') | Some(b'\t') | Some(b'\r') | Some(b'\n') => {
                    self.bump();
                }
                Some(b'/') if self.bytes.get(self.pos + 1) == Some(&b'/') => {
                    while let Some(b) = self.peek() {
                        if b == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some(b'/') if self.bytes.get(self.pos + 1) == Some(&b'*') => {
                    self.bump();
                    self.bump();
                    loop {
                        match (self.peek(), self.bytes.get(self.pos + 1)) {
                            (Some(b'*'), Some(b'/')) => {
                                self.bump();
                                self.bump();
                                break;
                            }
                            (None, _) | (Some(_), None) => return, // unterminated: caller errors on EOF
                            _ => {
                                self.bump();
                            }
                        }
                    }
                }
                _ => break,
            }
        }
    }

    fn value(&mut self) -> Result<Value, (usize, usize, String)> {
        self.skip_ws();
        let line = self.line;
        match self.peek() {
            None => Err(self.error("unexpected end of input")),
            Some(b'{') => self.object(line),
            Some(b'[') => self.array(line),
            Some(b'"') => Ok(Value::String {
                value: self.string()?,
                line,
            }),
            Some(b't') => self.literal("true", Value::Bool { value: true, line }),
            Some(b'f') => self.literal("false", Value::Bool { value: false, line }),
            Some(b'n') => self.literal("null", Value::Null { line }),
            Some(b) if b == b'-' || b.is_ascii_digit() => self.number(line),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn literal(&mut self, text: &str, value: Value) -> Result<Value, (usize, usize, String)> {
        if self.bytes[self.pos..].starts_with(text.as_bytes()) {
            for _ in text.bytes() {
                self.bump();
            }
            Ok(value)
        } else {
            Err(self.error(format!("expected `{text}`")))
        }
    }

    fn number(&mut self, line: usize) -> Result<Value, (usize, usize, String)> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.bump();
        }
        while self
            .peek()
            .is_some_and(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
        {
            self.bump();
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.error("invalid number"))?;
        text.parse::<f64>()
            .map(|value| Value::Number { value, line })
            .map_err(|_| self.error("invalid number"))
    }

    fn string(&mut self) -> Result<String, (usize, usize, String)> {
        if self.peek() != Some(b'"') {
            return Err(self.error("expected string"));
        }
        self.bump();
        let mut out = String::new();
        loop {
            match self.bump() {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => return Ok(out),
                Some(b'\\') => match self.bump() {
                    Some(b'"') => out.push('"'),
                    Some(b'\\') => out.push('\\'),
                    Some(b'/') => out.push('/'),
                    Some(b'b') => out.push('\u{0008}'),
                    Some(b'f') => out.push('\u{000C}'),
                    Some(b'n') => out.push('\n'),
                    Some(b'r') => out.push('\r'),
                    Some(b't') => out.push('\t'),
                    Some(b'u') => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            let b = self.bump().ok_or_else(|| self.error("bad escape"))?;
                            let digit = (b as char)
                                .to_digit(16)
                                .ok_or_else(|| self.error("bad unicode escape"))?;
                            code = code * 16 + digit;
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    }
                    _ => return Err(self.error("bad escape")),
                },
                Some(b) if b < 0x20 => return Err(self.error("control character in string")),
                Some(b) => {
                    // Collect UTF-8 continuation bytes.
                    let len = utf8_len(b);
                    let start = self.pos - 1;
                    for _ in 1..len {
                        self.bump();
                    }
                    let text = std::str::from_utf8(&self.bytes[start..self.pos])
                        .map_err(|_| self.error("invalid utf-8"))?;
                    out.push_str(text);
                }
            }
        }
    }

    fn array(&mut self, line: usize) -> Result<Value, (usize, usize, String)> {
        self.bump(); // '['
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.bump();
            return Ok(Value::Array { items, line });
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip_ws();
                    if self.peek() == Some(b']') {
                        self.bump(); // trailing comma
                        return Ok(Value::Array { items, line });
                    }
                }
                Some(b']') => {
                    self.bump();
                    return Ok(Value::Array { items, line });
                }
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
    }

    fn object(&mut self, line: usize) -> Result<Value, (usize, usize, String)> {
        self.bump(); // '{'
        let mut members = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.bump();
            return Ok(Value::Object { members, line });
        }
        loop {
            self.skip_ws();
            let key_line = self.line;
            let key = self.string().map_err(|mut e| {
                if e.2.contains("expected string") {
                    e.2 = "expected string key (unquoted keys are invalid JSONC)".into();
                }
                let _ = key_line;
                e
            })?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.error("expected `:`"));
            }
            self.bump();
            let value = self.value()?;
            members.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => {
                    self.bump();
                    self.skip_ws();
                    if self.peek() == Some(b'}') {
                        self.bump(); // trailing comma
                        return Ok(Value::Object { members, line });
                    }
                }
                Some(b'}') => {
                    self.bump();
                    return Ok(Value::Object { members, line });
                }
                _ => return Err(self.error("expected `,` or `}`")),
            }
        }
    }
}

const fn utf8_len(first: u8) -> usize {
    if first >= 0xF0 {
        4
    } else if first >= 0xE0 {
        3
    } else if first >= 0xC0 {
        2
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_with_comments_and_trailing_commas() {
        let value = parse("{\n  // c\n  \"a\": [1, 2,],\n}").unwrap();
        let members = value.as_object().unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].0, "a");
    }

    #[test]
    fn rejects_json5_only_syntax() {
        assert!(parse("{ a: 1 }").is_err());
        assert!(parse("{ 'a': 1 }").is_err());
    }

    #[test]
    fn reports_exact_line_column() {
        let (line, column, _) = parse("{\n  \"a\": ,\n}").unwrap_err();
        assert_eq!(line, 2);
        assert_eq!(column, 8);
    }
}

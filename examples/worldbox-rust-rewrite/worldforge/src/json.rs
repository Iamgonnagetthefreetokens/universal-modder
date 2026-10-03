//! A small JSON reader.
//!
//! The bridge's wire format is JSON lines, and `worldforge mcview` has to read
//! it, so the crate needs a parser. It is deliberately small -- no dependencies,
//! no serializer beyond the hand-written writers in [`crate::bridge`] -- and it
//! never panics on bad input, which matters because the input is a socket.

use std::fmt;

/// A parsed JSON value.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Json::Null => f.write_str("null"),
            Json::Bool(b) => write!(f, "{b}"),
            Json::Num(n) => write!(f, "{n}"),
            Json::Str(s) => write!(f, "{s:?}"),
            Json::Arr(a) => write!(f, "[{} items]", a.len()),
            Json::Obj(o) => write!(f, "{{{}}}", o.len()),
        }
    }
}

impl Json {
    /// The value for `key`, if this is an object with one.
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(o) => o.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Num(n) => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Json::Bool(b) => Some(*b),
            Json::Num(n) => Some(*n != 0.0),
            _ => None,
        }
    }

    pub fn as_arr(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(a) => Some(a),
            _ => None,
        }
    }

    /// `[id, name, ...]` style rows: the value at `index`, if it is an array.
    pub fn at(&self, index: usize) -> Option<&Json> {
        self.as_arr().and_then(|a| a.get(index))
    }

    pub fn arr_len(&self) -> usize {
        self.as_arr().map(|a| a.len()).unwrap_or(0)
    }
}

/// Parse one JSON value; trailing whitespace is fine, trailing text is not.
pub fn parse(text: &str) -> Result<Json, String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut p = Parser { bytes, i: 0 };
    p.skip_ws();
    let value = p.value()?;
    p.skip_ws();
    if p.i < p.bytes.len() {
        return Err(format!("trailing text at offset {}", p.i));
    }
    Ok(value)
}

struct Parser {
    bytes: Vec<char>,
    i: usize,
}

impl Parser {
    fn skip_ws(&mut self) {
        while self.i < self.bytes.len() && self.bytes[self.i].is_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.bytes.get(self.i).copied()
    }

    fn eat(&mut self, c: char) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            Err(format!("expected `{c}` at offset {}", self.i))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(Json::Str(self.string()?)),
            Some('t') | Some('f') => self.literal(),
            Some('n') => {
                if self.bytes[self.i..].starts_with(&['n', 'u', 'l', 'l']) {
                    self.i += 4;
                    Ok(Json::Null)
                } else {
                    Err(format!("bad literal at offset {}", self.i))
                }
            }
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            other => Err(format!("unexpected {other:?} at offset {}", self.i)),
        }
    }

    fn literal(&mut self) -> Result<Json, String> {
        if self.bytes[self.i..].starts_with(&['t', 'r', 'u', 'e']) {
            self.i += 4;
            Ok(Json::Bool(true))
        } else if self.bytes[self.i..].starts_with(&['f', 'a', 'l', 's', 'e']) {
            self.i += 5;
            Ok(Json::Bool(false))
        } else {
            Err(format!("bad literal at offset {}", self.i))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        if self.peek() == Some('-') {
            self.i += 1;
        }
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() || "+-.eE".contains(c) {
                self.i += 1;
            } else {
                break;
            }
        }
        let text: String = self.bytes[start..self.i].iter().collect();
        text.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("bad number `{text}`"))
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat('"')?;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self.peek().ok_or("unterminated escape")?;
                    self.i += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => {
                            let hex: String = self
                                .bytes
                                .get(self.i..self.i + 4)
                                .ok_or("short \\u escape")?
                                .iter()
                                .collect();
                            self.i += 4;
                            let code = u32::from_str_radix(&hex, 16)
                                .map_err(|_| format!("bad \\u escape `{hex}`"))?;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        other => return Err(format!("bad escape `\\{other}`")),
                    }
                }
                c => out.push(c),
            }
        }
        Err("unterminated string".into())
    }

    fn array(&mut self) -> Result<Json, String> {
        self.eat('[')?;
        let mut out = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.i += 1;
            return Ok(Json::Arr(out));
        }
        loop {
            self.skip_ws();
            out.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.i += 1;
                }
                Some(']') => {
                    self.i += 1;
                    return Ok(Json::Arr(out));
                }
                other => return Err(format!("expected `,` or `]`, found {other:?}")),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.eat('{')?;
        let mut out = Vec::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.i += 1;
            return Ok(Json::Obj(out));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.eat(':')?;
            self.skip_ws();
            let value = self.value()?;
            out.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(',') => {
                    self.i += 1;
                }
                Some('}') => {
                    self.i += 1;
                    return Ok(Json::Obj(out));
                }
                other => return Err(format!("expected `,` or `}}`, found {other:?}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars() {
        assert_eq!(parse("null").unwrap(), Json::Null);
        assert_eq!(parse(" true ").unwrap(), Json::Bool(true));
        assert_eq!(parse("-12.5").unwrap(), Json::Num(-12.5));
        assert_eq!(parse("\"hi\"").unwrap(), Json::Str("hi".into()));
        assert_eq!(parse("1e3").unwrap(), Json::Num(1000.0));
    }

    #[test]
    fn nested_structures() {
        let v = parse(r#"{"a":[1,2,{"b":"c\nd"}],"e":{}}"#).unwrap();
        assert_eq!(v.get("a").unwrap().arr_len(), 3);
        assert_eq!(v.get("a").unwrap().at(2).unwrap().get("b").unwrap().as_str(), Some("c\nd"));
        assert_eq!(v.get("e").unwrap().arr_len(), 0);
    }

    #[test]
    fn empty_arrays_and_objects() {
        assert_eq!(parse("[]").unwrap().arr_len(), 0);
        assert_eq!(parse("{}").unwrap().get("x"), None);
        assert_eq!(parse("[[],[]]").unwrap().arr_len(), 2);
    }

    #[test]
    fn escapes() {
        let v = parse(r#""a\"b\\c\u0041""#).unwrap();
        assert_eq!(v.as_str(), Some("a\"b\\cA"));
    }

    #[test]
    fn bad_input_is_an_error() {
        for bad in [
            "", "{", "[1,", "{\"a\"}", "{'a':1}", "nul", "tru", "1 2", "\"unterminated", "{\"a\":1,}",
        ] {
            assert!(parse(bad).is_err(), "`{bad}` should not parse");
        }
    }

    #[test]
    fn accessors_do_not_panic_on_the_wrong_type() {
        let v = parse(r#"{"s":"x","n":3,"b":true,"a":[1]}"#).unwrap();
        assert_eq!(v.get("s").unwrap().as_i64(), None);
        assert_eq!(v.get("n").unwrap().as_str(), None);
        assert_eq!(v.get("b").unwrap().as_bool(), Some(true));
        assert_eq!(v.get("a").unwrap().as_arr().unwrap().len(), 1);
        assert_eq!(v.get("missing"), None);
        assert_eq!(v.as_str(), None);
    }
}

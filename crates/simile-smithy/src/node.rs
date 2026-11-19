use serde_json::{Map, Number, Value};

use crate::error::{Error, Result};

pub(crate) fn parse(source: &str) -> Result<Value> {
    let mut parser = NodeParser {
        source: source.as_bytes(),
        cursor: 0,
    };
    let value = parser.value()?;
    parser.separators();
    if parser.cursor != parser.source.len() {
        return Err(parser.invalid("unexpected trailing input"));
    }
    Ok(value)
}

struct NodeParser<'a> {
    source: &'a [u8],
    cursor: usize,
}

impl NodeParser<'_> {
    fn value(&mut self) -> Result<Value> {
        self.separators();
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Value::String),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => {
                let identifier = self.identifier()?;
                match identifier.as_str() {
                    "true" => Ok(Value::Bool(true)),
                    "false" => Ok(Value::Bool(false)),
                    "null" => Ok(Value::Null),
                    _ => Ok(Value::String(identifier)),
                }
            }
            None => Err(self.invalid("expected a node value")),
        }
    }

    fn object(&mut self) -> Result<Value> {
        self.expect(b'{')?;
        let mut object = Map::new();
        loop {
            self.separators();
            if self.consume(b'}') {
                return Ok(Value::Object(object));
            }
            let key = if self.peek() == Some(b'"') {
                self.string()?
            } else {
                self.identifier()?
            };
            self.separators();
            self.expect(b':')?;
            object.insert(key, self.value()?);
        }
    }

    fn array(&mut self) -> Result<Value> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        loop {
            self.separators();
            if self.consume(b']') {
                return Ok(Value::Array(values));
            }
            values.push(self.value()?);
        }
    }

    fn string(&mut self) -> Result<String> {
        let start = self.cursor;
        self.expect(b'"')?;
        while let Some(byte) = self.peek() {
            match byte {
                b'\\' => {
                    self.cursor += 1;
                    if self.peek().is_none() {
                        return Err(self.invalid("unterminated string escape"));
                    }
                    self.cursor += 1;
                }
                b'"' => {
                    self.cursor += 1;
                    let raw = std::str::from_utf8(&self.source[start..self.cursor])
                        .map_err(|error| self.invalid(error.to_string()))?;
                    return serde_json::from_str(raw)
                        .map_err(|error| self.invalid(error.to_string()));
                }
                _ => self.cursor += 1,
            }
        }
        Err(self.invalid("unterminated string"))
    }

    fn number(&mut self) -> Result<Value> {
        let start = self.cursor;
        while matches!(
            self.peek(),
            Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
        ) {
            self.cursor += 1;
        }
        let raw = std::str::from_utf8(&self.source[start..self.cursor])
            .map_err(|error| self.invalid(error.to_string()))?;
        if let Ok(value) = raw.parse::<i64>() {
            return Ok(Value::Number(Number::from(value)));
        }
        if let Ok(value) = raw.parse::<u64>() {
            return Ok(Value::Number(Number::from(value)));
        }
        raw.parse::<f64>()
            .ok()
            .and_then(Number::from_f64)
            .map(Value::Number)
            .ok_or_else(|| self.invalid(format!("invalid number `{raw}`")))
    }

    fn identifier(&mut self) -> Result<String> {
        let start = self.cursor;
        while matches!(
            self.peek(),
            Some(b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'#' | b'$')
        ) {
            self.cursor += 1;
        }
        if start == self.cursor {
            return Err(self.invalid("expected an identifier"));
        }
        std::str::from_utf8(&self.source[start..self.cursor])
            .map(str::to_owned)
            .map_err(|error| self.invalid(error.to_string()))
    }

    fn separators(&mut self) {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n' | b',')) {
                self.cursor += 1;
            }
            if self.source.get(self.cursor..self.cursor + 2) == Some(b"//") {
                while !matches!(self.peek(), None | Some(b'\n')) {
                    self.cursor += 1;
                }
                continue;
            }
            break;
        }
    }

    fn expect(&mut self, expected: u8) -> Result<()> {
        if self.consume(expected) {
            Ok(())
        } else {
            Err(self.invalid(format!("expected `{}`", char::from(expected))))
        }
    }

    fn consume(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.cursor).copied()
    }

    fn invalid(&self, detail: impl Into<String>) -> Error {
        Error::Invalid {
            context: "node value",
            detail: format!("{} at byte {}", detail.into(), self.cursor),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn parses_smithy_node_syntax_with_optional_commas() {
        let value = super::parse(
            r#"[
                { json: { id: "or_123", complete: true } }
                { json: [1, 2, 3] }
            ]"#,
        )
        .unwrap_or_else(|error| panic!("node value must parse: {error}"));
        assert_eq!(
            value,
            json!([
                { "json": { "id": "or_123", "complete": true } },
                { "json": [1, 2, 3] }
            ])
        );
    }
}

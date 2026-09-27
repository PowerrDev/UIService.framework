//! Just enough of Apple's XML property list format for the Dock's
//! configuration and an app bundle's `Info.plist`: `dict`, `array`, `key`,
//! `string`, `integer`, `true`/`false`. No allocation: the document is
//! parsed into a fixed table of nodes that borrow their text from it.
//!
//! Entities in text are limited to the five XML ones and are decoded on
//! request ([`Plist::string_into`]); a `string` whose text needs none can be
//! read in place ([`Plist::string`]).

/// How many values one document can hold.
pub const MAX_NODES: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Value<'a> {
    Dict { first: u16, count: u16 },
    Array { first: u16, count: u16 },
    String(&'a str),
    Integer(i64),
    Bool(bool),
}

/// A node: its value, the key it sits under (in a dict), and the next
/// sibling, so a container's children are a linked run.
#[derive(Clone, Copy, Debug)]
struct Node<'a> {
    value: Value<'a>,
    key: &'a str,
    next: u16,
}

const NONE: u16 = u16::MAX;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Syntax,
    TooLarge,
    NoRoot,
}

pub struct Plist<'a> {
    nodes: [Node<'a>; MAX_NODES],
    count: usize,
}

/// A reference to one value in a document.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Id(u16);

struct Cursor<'a> {
    text: &'a str,
    at: usize,
}

enum Token<'a> {
    Open(&'a str),
    Close(&'a str),
    Empty(&'a str),
    Text,
    End,
}

impl<'a> Cursor<'a> {
    fn next(&mut self) -> Result<Token<'a>, Error> {
        loop {
            let rest = &self.text[self.at..];
            if rest.is_empty() {
                return Ok(Token::End);
            }
            if let Some(stripped) = rest.strip_prefix("<?") {
                let end = stripped.find("?>").ok_or(Error::Syntax)?;
                self.at += 2 + end + 2;
                continue;
            }
            if let Some(stripped) = rest.strip_prefix("<!--") {
                let end = stripped.find("-->").ok_or(Error::Syntax)?;
                self.at += 4 + end + 3;
                continue;
            }
            if let Some(stripped) = rest.strip_prefix("<!") {
                let end = stripped.find('>').ok_or(Error::Syntax)?;
                self.at += 2 + end + 1;
                continue;
            }
            if let Some(stripped) = rest.strip_prefix('<') {
                let end = stripped.find('>').ok_or(Error::Syntax)?;
                self.at += 1 + end + 1;
                let inside = stripped[..end].trim();
                if let Some(name) = inside.strip_prefix('/') {
                    return Ok(Token::Close(name.trim()));
                }
                if let Some(body) = inside.strip_suffix('/') {
                    return Ok(Token::Empty(tag_name(body)));
                }
                return Ok(Token::Open(tag_name(inside)));
            }
            let end = rest.find('<').unwrap_or(rest.len());
            self.at += end;
            if !rest[..end].trim().is_empty() {
                return Ok(Token::Text);
            }
        }
    }

    /// The text up to `</name>` (empty if the element closes at once).
    fn text_until_close(&mut self, name: &str) -> Result<&'a str, Error> {
        let rest = &self.text[self.at..];
        let end = rest.find('<').ok_or(Error::Syntax)?;
        self.at += end;
        match self.next()? {
            Token::Close(closing) if closing == name => Ok(&rest[..end]),
            _ => Err(Error::Syntax),
        }
    }
}

/// `plist version="1.0"` -> `plist`.
fn tag_name(inside: &str) -> &str {
    inside.split(|character: char| character.is_ascii_whitespace()).next().unwrap_or("")
}

impl<'a> Plist<'a> {
    pub fn parse(text: &'a str) -> Result<Self, Error> {
        let mut plist = Self {
            nodes: [Node { value: Value::Bool(false), key: "", next: NONE }; MAX_NODES],
            count: 0,
        };
        let mut cursor = Cursor { text, at: 0 };

        // Up to the first value, inside <plist> or not.
        loop {
            match cursor.next()? {
                Token::Open("plist") => continue,
                Token::Open(name) => {
                    plist.value(&mut cursor, Token::Open(name), "")?;
                    return Ok(plist);
                }
                Token::Empty(name) => {
                    plist.value(&mut cursor, Token::Empty(name), "")?;
                    return Ok(plist);
                }
                Token::End | Token::Close("plist") => return Err(Error::NoRoot),
                _ => return Err(Error::Syntax),
            }
        }
    }

    fn push(&mut self, value: Value<'a>, key: &'a str) -> Result<u16, Error> {
        if self.count == MAX_NODES {
            return Err(Error::TooLarge);
        }
        self.nodes[self.count] = Node { value, key, next: NONE };
        self.count += 1;
        Ok((self.count - 1) as u16)
    }

    /// Parse the value that `token` opened; returns its node.
    fn value(&mut self, cursor: &mut Cursor<'a>, token: Token<'a>, key: &'a str) -> Result<u16, Error> {
        match token {
            Token::Empty("true") => self.push(Value::Bool(true), key),
            Token::Empty("false") => self.push(Value::Bool(false), key),
            Token::Empty("string") => self.push(Value::String(""), key),
            Token::Empty("dict") => self.push(Value::Dict { first: NONE, count: 0 }, key),
            Token::Empty("array") => self.push(Value::Array { first: NONE, count: 0 }, key),
            Token::Open("string") => {
                let text = cursor.text_until_close("string")?;
                self.push(Value::String(text), key)
            }
            Token::Open("integer") => {
                let text = cursor.text_until_close("integer")?.trim();
                let (negative, digits) = match text.strip_prefix('-') {
                    Some(digits) => (true, digits),
                    None => (false, text),
                };
                if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err(Error::Syntax);
                }
                let mut value: i64 = 0;
                for byte in digits.bytes() {
                    value = value.checked_mul(10).and_then(|value| value.checked_add((byte - b'0') as i64)).ok_or(Error::Syntax)?;
                }
                self.push(Value::Integer(if negative { -value } else { value }), key)
            }
            Token::Open(name @ ("dict" | "array")) => {
                let dict = name == "dict";
                let node = self.push(Value::Array { first: NONE, count: 0 }, key)?;
                let mut first = NONE;
                let mut last = NONE;
                let mut count = 0u16;
                loop {
                    let mut child_key = "";
                    let mut token = cursor.next()?;
                    if let Token::Close(closing) = token {
                        if closing == name {
                            break;
                        }
                        return Err(Error::Syntax);
                    }
                    if dict {
                        match token {
                            Token::Open("key") => child_key = cursor.text_until_close("key")?.trim(),
                            _ => return Err(Error::Syntax),
                        }
                        token = cursor.next()?;
                    }
                    let child = self.value(cursor, token, child_key)?;
                    if first == NONE {
                        first = child;
                    } else {
                        self.nodes[last as usize].next = child;
                    }
                    last = child;
                    count += 1;
                }
                self.nodes[node as usize].value =
                    if dict { Value::Dict { first, count } } else { Value::Array { first, count } };
                Ok(node)
            }
            _ => Err(Error::Syntax),
        }
    }

    pub fn root(&self) -> Id {
        Id(0)
    }

    pub fn get(&self, id: Id) -> Value<'a> {
        self.nodes[id.0 as usize].value
    }

    /// The children of a dict or an array, in order, with their keys.
    pub fn children(&self, id: Id) -> impl Iterator<Item = (&'a str, Id)> + '_ {
        let first = match self.get(id) {
            Value::Dict { first, .. } | Value::Array { first, .. } => first,
            _ => NONE,
        };
        let mut next = first;
        core::iter::from_fn(move || {
            if next == NONE {
                return None;
            }
            let node = self.nodes[next as usize];
            let id = Id(next);
            next = node.next;
            Some((node.key, id))
        })
    }

    /// The value under `key` in dict `id`.
    pub fn lookup(&self, id: Id, key: &str) -> Option<Id> {
        match self.get(id) {
            Value::Dict { .. } => self.children(id).find(|(child_key, _)| *child_key == key).map(|(_, id)| id),
            _ => None,
        }
    }

    /// A string value as written, when it holds no entity to decode.
    pub fn string(&self, id: Id) -> Option<&'a str> {
        match self.get(id) {
            Value::String(text) if !text.contains('&') => Some(text),
            _ => None,
        }
    }

    /// A string value with its entities decoded into `buffer`.
    pub fn string_into<'b>(&self, id: Id, buffer: &'b mut [u8]) -> Option<&'b str> {
        let Value::String(text) = self.get(id) else { return None; };
        let mut length = 0;
        let mut rest = text;
        while !rest.is_empty() {
            let (piece, skip): (&str, usize) = if let Some(after) = rest.strip_prefix('&') {
                let end = after.find(';')?;
                let decoded = match &after[..end] {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => return None,
                };
                (decoded, 1 + end + 1)
            } else {
                let end = rest.find('&').unwrap_or(rest.len());
                (&rest[..end], end)
            };
            let end = length + piece.len();
            if end > buffer.len() {
                return None;
            }
            buffer[length..end].copy_from_slice(piece.as_bytes());
            length = end;
            rest = &rest[skip..];
        }
        core::str::from_utf8(&buffer[..length]).ok()
    }

    pub fn integer(&self, id: Id) -> Option<i64> {
        match self.get(id) {
            Value::Integer(value) => Some(value),
            _ => None,
        }
    }

    pub fn boolean(&self, id: Id) -> Option<bool> {
        match self.get(id) {
            Value::Bool(value) => Some(value),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{vec, vec::Vec};

    const DOCK: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//NXU//DTD PLIST 1.0//EN" "http://www.nxu.local/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<!-- the apps, left to right -->
	<key>persistent-apps</key>
	<array>
		<dict>
			<key>path</key>
			<string>/Applications/Voyager.app</string>
		</dict>
		<dict>
			<key>path</key>
			<string>/Applications/Activity Monitor.app</string>
		</dict>
	</array>
	<key>tilesize</key>
	<integer>48</integer>
	<key>show-recents</key>
	<true/>
	<key>empty</key>
	<string/>
</dict>
</plist>
"#;

    #[test]
    fn reads_the_dock_configuration() {
        let plist = Plist::parse(DOCK).unwrap();
        let root = plist.root();
        let apps = plist.lookup(root, "persistent-apps").unwrap();
        let paths: Vec<&str> = plist
            .children(apps)
            .map(|(_, app)| plist.string(plist.lookup(app, "path").unwrap()).unwrap())
            .collect();
        assert_eq!(paths, ["/Applications/Voyager.app", "/Applications/Activity Monitor.app"]);
        assert_eq!(plist.integer(plist.lookup(root, "tilesize").unwrap()), Some(48));
        assert_eq!(plist.boolean(plist.lookup(root, "show-recents").unwrap()), Some(true));
        assert_eq!(plist.string(plist.lookup(root, "empty").unwrap()), Some(""));
        assert!(plist.lookup(root, "missing").is_none());
    }

    #[test]
    fn decodes_entities_on_request() {
        let plist = Plist::parse("<plist><string>Tom &amp; Jerry &lt;3</string></plist>").unwrap();
        assert_eq!(plist.string(plist.root()), None);
        let mut buffer = [0u8; 32];
        assert_eq!(plist.string_into(plist.root(), &mut buffer), Some("Tom & Jerry <3"));
    }

    #[test]
    fn rejects_broken_documents() {
        assert_eq!(Plist::parse("<plist><dict><string>x</string></dict></plist>").err(), Some(Error::Syntax));
        assert_eq!(Plist::parse("<plist><integer>4x</integer></plist>").err(), Some(Error::Syntax));
        assert_eq!(Plist::parse("<plist></plist>").err(), Some(Error::NoRoot));
        assert_eq!(Plist::parse("<plist><array><string>open</array></plist>").err(), Some(Error::Syntax));
    }

    #[test]
    fn negative_and_empty_containers() {
        let plist = Plist::parse("<plist><array><integer>-7</integer><dict/><array></array></array></plist>").unwrap();
        let values: Vec<Value> = plist.children(plist.root()).map(|(_, id)| plist.get(id)).collect();
        assert_eq!(values[0], Value::Integer(-7));
        assert!(matches!(values[1], Value::Dict { count: 0, .. }));
        assert!(matches!(values[2], Value::Array { count: 0, .. }));
    }
}

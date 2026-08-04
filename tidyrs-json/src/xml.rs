//! Minimal, tolerant XML -> `serde_json::Value` converter. We don't rely on
//! quick-xml's serde bridge here because it expects a known target shape;
//! we want the same "anything goes" tree that `serde_json::Value` gives us
//! for JSON input, so both formats can share one flattening pass.

use quick_xml::events::Event;
use quick_xml::reader::Reader;
use serde_json::{Map, Value};

struct Node {
    tag: String,
    attrs: Map<String, Value>,
    children: Vec<Node>,
    text: String,
}

impl Node {
    fn into_value(self) -> Value {
        if self.children.is_empty() && self.attrs.is_empty() {
            let trimmed = self.text.trim();
            return if trimmed.is_empty() {
                Value::Null
            } else {
                Value::String(trimmed.to_string())
            };
        }

        let mut obj = self.attrs;
        let trimmed_text = self.text.trim();
        if !trimmed_text.is_empty() {
            obj.insert("#text".to_string(), Value::String(trimmed_text.to_string()));
        }

        let mut ordered_keys: Vec<String> = Vec::new();
        let mut grouped: std::collections::HashMap<String, Vec<Value>> = std::collections::HashMap::new();
        for child in self.children {
            let tag = child.tag.clone();
            if !grouped.contains_key(&tag) {
                ordered_keys.push(tag.clone());
            }
            grouped.entry(tag).or_default().push(child.into_value());
        }
        let _ = &mut ordered_keys; // silence unused warning if grouped stays empty
        for key in ordered_keys {
            let mut values = grouped.remove(&key).unwrap_or_default();
            let value = if values.len() == 1 {
                values.pop().unwrap()
            } else {
                Value::Array(values)
            };
            obj.insert(key, value);
        }

        Value::Object(obj)
    }
}

pub fn xml_to_value(xml: &str) -> Result<Value, String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut attrs = Map::new();
                for a in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
                    let val = String::from_utf8_lossy(&a.value).to_string();
                    attrs.insert(format!("@{key}"), Value::String(val));
                }
                stack.push(Node { tag, attrs, children: Vec::new(), text: String::new() });
            }
            Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                let mut attrs = Map::new();
                for a in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(a.key.as_ref()).to_string();
                    let val = String::from_utf8_lossy(&a.value).to_string();
                    attrs.insert(format!("@{key}"), Value::String(val));
                }
                let node = Node { tag, attrs, children: Vec::new(), text: String::new() };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                } else {
                    root = Some(node);
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(node) = stack.last_mut() {
                    node.text.push_str(&t.unescape().unwrap_or_default());
                }
            }
            Ok(Event::End(_)) => {
                if let Some(node) = stack.pop() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    } else {
                        root = Some(node);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("XML parse error: {e}")),
            _ => {}
        }
    }

    root.map(|n| n.into_value()).ok_or_else(|| "empty XML document".to_string())
}

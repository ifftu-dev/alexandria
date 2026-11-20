//! A small owned XML tree with namespaces resolved.
//!
//! Manifests are read whole into this tree and then walked. Nothing here
//! fetches or expands anything: a document type declaration is skipped, and
//! an entity it would have defined is an error where it is used.

use quick_xml::escape::resolve_predefined_entity;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;
use quick_xml::XmlVersion;

/// Deepest nesting accepted. Real manifests nest a handful of levels; the cap
/// keeps the recursive walks over the tree off the edge of the stack.
const MAX_DEPTH: usize = 64;

pub(crate) const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Debug, Default)]
pub(crate) struct Element {
    /// Namespace address, when the name is bound to one.
    pub ns: Option<String>,
    /// Name without its prefix.
    pub name: String,
    pub attributes: Vec<Attribute>,
    pub children: Vec<Element>,
    /// The element's own text, child elements left out.
    pub text: String,
}

#[derive(Debug)]
pub(crate) struct Attribute {
    pub ns: Option<String>,
    pub name: String,
    pub value: String,
}

impl Element {
    /// The first child with this name, in any namespace. Namespaces are used
    /// to tell standards apart, not to find elements: packages in the wild
    /// get the addresses wrong often enough that insisting on them would
    /// refuse content every other player accepts.
    pub(crate) fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.name == name)
    }

    pub(crate) fn children_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// An attribute by name, in any namespace or none.
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .map(|attribute| attribute.value.as_str())
    }

    /// The `xml:base` attribute, which is found by namespace: an unrelated
    /// `base` attribute must not be mistaken for it.
    pub(crate) fn xml_base(&self) -> &str {
        self.attributes
            .iter()
            .find(|attribute| {
                attribute.name == "base" && attribute.ns.as_deref() == Some(XML_NAMESPACE)
            })
            .map_or("", |attribute| attribute.value.as_str())
    }

    /// The trimmed text of the first child with this name, if it has any.
    pub(crate) fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name)
            .map(|child| child.text.trim())
            .filter(|text| !text.is_empty())
    }

    /// Whether this element or anything under it uses one of the namespaces.
    pub(crate) fn uses_namespace(&self, namespaces: &[&str]) -> bool {
        let named = |ns: &Option<String>| ns.as_deref().is_some_and(|ns| namespaces.contains(&ns));
        named(&self.ns)
            || self.attributes.iter().any(|attribute| named(&attribute.ns))
            || self
                .children
                .iter()
                .any(|child| child.uses_namespace(namespaces))
    }
}

/// Manifest bytes to text. UTF-8, with or without a byte order mark, is what
/// the standards ask for; ISO-8859-1 is accepted when the document declares
/// it, because manifests written by hand in the early 2000s do.
pub(crate) fn decode(bytes: &[u8]) -> Result<String, String> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(text.to_owned()),
        Err(_) if declares_latin1(bytes) => Ok(bytes.iter().map(|&byte| byte as char).collect()),
        Err(_) => Err("the manifest is not UTF-8 text".to_owned()),
    }
}

fn declares_latin1(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(200)];
    let Some(end) = head.windows(2).position(|pair| pair == b"?>") else {
        return false;
    };
    let declaration = String::from_utf8_lossy(&head[..end]).to_ascii_lowercase();
    declaration.starts_with("<?xml")
        && ["iso-8859-1", "latin1", "windows-1252"]
            .iter()
            .any(|name| declaration.contains(name))
}

/// Parses a document into its root element.
pub(crate) fn parse(text: &str) -> Result<Element, String> {
    let mut reader = NsReader::from_str(text);
    let mut open: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let mut close = |element: Element, open: &mut Vec<Element>| match open.last_mut() {
        Some(parent) => {
            parent.children.push(element);
            Ok(())
        }
        None if root.is_none() => {
            root = Some(element);
            Ok(())
        }
        None => Err("the manifest has more than one root element".to_owned()),
    };

    loop {
        let event = reader
            .read_event()
            .map_err(|error| malformed(&reader, error))?;
        match event {
            Event::Start(start) => {
                if open.len() >= MAX_DEPTH {
                    return Err(format!(
                        "the manifest nests elements more than {MAX_DEPTH} deep"
                    ));
                }
                open.push(element(&reader, &start)?);
            }
            Event::Empty(start) => close(element(&reader, &start)?, &mut open)?,
            Event::End(_) => {
                let element = open
                    .pop()
                    .ok_or_else(|| "the manifest closes an element it never opened".to_owned())?;
                close(element, &mut open)?;
            }
            Event::Text(text) => {
                if let Some(element) = open.last_mut() {
                    let text = text.decode().map_err(|error| malformed(&reader, error))?;
                    element.text.push_str(&text);
                }
            }
            Event::CData(data) => {
                if let Some(element) = open.last_mut() {
                    let data = data.decode().map_err(|error| malformed(&reader, error))?;
                    element.text.push_str(&data);
                }
            }
            Event::GeneralRef(reference) => {
                let Some(element) = open.last_mut() else {
                    continue;
                };
                let resolved = reference
                    .resolve_char_ref()
                    .map_err(|error| malformed(&reader, error))?;
                if let Some(character) = resolved {
                    element.text.push(character);
                    continue;
                }
                let name = reference
                    .decode()
                    .map_err(|error| malformed(&reader, error))?;
                match resolve_predefined_entity(&name) {
                    Some(text) => element.text.push_str(text),
                    None => return Err(format!("the manifest uses an undefined entity &{name};")),
                }
            }
            Event::Eof => break,
            Event::Decl(_) | Event::PI(_) | Event::Comment(_) | Event::DocType(_) => {}
        }
    }
    if !open.is_empty() {
        return Err("the manifest ends before its elements are closed".to_owned());
    }
    root.ok_or_else(|| "the manifest has no root element".to_owned())
}

fn element(reader: &NsReader<&[u8]>, start: &BytesStart<'_>) -> Result<Element, String> {
    let (ns, name) = reader.resolver().resolve_element(start.name());
    let mut element = Element {
        ns: namespace(ns),
        name: text(name.into_inner())?,
        ..Element::default()
    };
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|error| malformed(reader, error))?;
        if attribute.key.as_namespace_binding().is_some() {
            continue;
        }
        let (ns, name) = reader.resolver().resolve_attribute(attribute.key);
        let value = attribute
            .decoded_and_normalized_value(XmlVersion::Implicit1_0, reader.decoder())
            .map_err(|error| malformed(reader, error))?;
        element.attributes.push(Attribute {
            ns: namespace(ns),
            name: text(name.into_inner())?,
            value: value.into_owned(),
        });
    }
    Ok(element)
}

/// An undeclared prefix is treated as no namespace, not as an error: the
/// element or attribute is still found by its name.
fn namespace(resolved: ResolveResult<'_>) -> Option<String> {
    match resolved {
        ResolveResult::Bound(ns) => std::str::from_utf8(ns.into_inner()).ok().map(str::to_owned),
        ResolveResult::Unbound | ResolveResult::Unknown(_) => None,
    }
}

fn text(bytes: &[u8]) -> Result<String, String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| "the manifest has a name that is not UTF-8".to_owned())
}

fn malformed(reader: &NsReader<&[u8]>, error: impl std::fmt::Display) -> String {
    format!(
        "the manifest is not well-formed XML near byte {}: {error}",
        reader.error_position()
    )
}

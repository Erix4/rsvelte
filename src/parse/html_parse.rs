use std::fmt::Debug;

use crate::{EVENTS, parse::rs_parse, utils::*};

pub enum ContentType {
    Text(String, Vec<syn::Expr>),
    Elem(Vec<Element>),
    None,
}

impl Debug for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentType::Text(text, exprs) => {
                let expr_strs = exprs
                    .iter()
                    .map(|e| quote::ToTokens::to_token_stream(e).to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "Text({}, `{}`)", text, expr_strs)
            }
            ContentType::Elem(elems) => {
                write!(f, "Elem({:?})", elems)
            }
            ContentType::None => {
                write!(f, "None")
            }
        }
    }
}

#[derive(Debug)]
pub struct Element {
    pub tag: Tag,
    pub id: u32,
    pub contents: ContentType,
}

impl Element {
    pub fn get_events(&self) -> Vec<(u32, &String, &AttrType)> {
        let all_attrs = self.get_all_attrs();
        all_attrs
            .iter()
            .filter(|(_, name, _)| {
                crate::EVENTS
                    .iter()
                    .any(|(event_name, _, _)| event_name == name)
            })
            .map(|(id, name, attr)| {
                if let AttrType::Str(_) = attr {
                    panic!(
                        "Event attribute '{}' must be a function call or closure, found string",
                        name
                    );
                }
                (*id, *name, *attr)
            })
            .collect()
    }

    fn get_all_attrs(&self) -> Vec<(u32, &String, &AttrType)> {
        let mut attrs = self
            .tag
            .attributes
            .iter()
            .map(|(name, attr)| (self.id, name, attr))
            .collect::<Vec<_>>();
        if let ContentType::Elem(children) = &self.contents {
            for child in children {
                attrs.extend(child.get_all_attrs());
            }
        }
        attrs
    }
}

pub fn read_element_with_tag(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    coord: &mut Coord,
    id_counter: &mut u32,
    tag: Tag,
) -> Element {
    let id = *id_counter;
    *id_counter += 1;

    let contents = if !tag.self_closing {
        read_until(chars, |ch| !ch.is_whitespace(), coord);

        if let Some(&ch) = chars.peek() {
            if ch == '<' {
                let mut child_elems = Vec::new();
                while let Some(&ch) = chars.peek() {
                    if ch != '<' {
                        panic!(
                            "Expected '<' at line {}, col {}, found '{}'",
                            coord.line, coord.col, ch
                        );
                    }
                    let next_tag = read_tag(chars, coord);
                    match next_tag {
                        TagType::Opening(next_tag) => {
                            let child_elem =
                                read_element_with_tag(chars, coord, id_counter, next_tag);
                            child_elems.push(child_elem);
                        }
                        TagType::Closing(name) => {
                            if name == tag.name {
                                break;
                            } else {
                                panic!(
                                    "Mismatched closing tag </{}> at line {}, col {}, expected </{}>",
                                    name, coord.line, coord.col, tag.name
                                );
                            }
                        }
                    }
                    read_until(chars, |ch| !ch.is_whitespace(), coord);
                }
                ContentType::Elem(child_elems)
            } else {
                let text = read_until(chars, |c| c == '<', coord);

                // Detect reactive expressions in text nodes
                let mut exprs = Vec::new();
                let mut processed_text = String::new();
                let mut text_chars = text.chars().peekable();
                while let Some(ch) = text_chars.next() {
                    if ch == '{' {
                        let expr_content = read_until(&mut text_chars, |c| c == '}', coord);
                        expect_next(&mut text_chars, '}', coord);
                        exprs.push(syn::parse_str::<syn::Expr>(&expr_content).unwrap());
                        processed_text.push_str("{}"); // Placeholder for expression
                    } else {
                        processed_text.push(ch);
                    }
                }

                read_closing_tag(chars, coord, &tag.name);
                ContentType::Text(processed_text, exprs)
            }
        } else {
            panic!(
                "Unexpected end of input while reading contents at line {}, col {}",
                coord.line, coord.col
            );
        }
    } else {
        ContentType::None
    };

    let elem_out = Element {
        tag: tag,
        id,
        contents: contents,
    };

    elem_out
}

pub fn read_closing_tag(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    coord: &mut Coord,
    expected_name: &str,
) {
    expect_next(chars, '<', coord);
    expect_next(chars, '/', coord);
    let name = read_until(chars, |ch| ch.is_whitespace() || ch == '>', coord);
    if name != expected_name {
        panic!(
            "Mismatched closing tag </{}> at line {}, col {}, expected </{}>",
            name, coord.line, coord.col, expected_name
        );
    }
    expect_next(chars, '>', coord);
}

#[derive(Clone)]
pub enum AttrType {
    Str(String),
    Call(syn::Ident),
    Expr(syn::Expr),
    Closure(rs_parse::AttrClosure),
}

pub struct Tag {
    pub name: String,
    pub attributes: Vec<(String, AttrType)>,
    self_closing: bool,
}

impl Debug for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let attrs_str = self
            .attributes
            .iter()
            .map(|(name, attr_type)| match attr_type {
                AttrType::Str(value) => format!("{}=\"{}\"", name, value),
                AttrType::Call(call) => {
                    format!("{}={}", name, quote::ToTokens::to_token_stream(call))
                }
                AttrType::Expr(expr) => {
                    format!("{}={}", name, quote::ToTokens::to_token_stream(&expr))
                }
                AttrType::Closure(_) => {
                    format!(
                        "{}=|...| {{ ... }}",
                        name
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        write!(
            f,
            "Tag {{ name: {}, attributes: [{}], self_closing: {} }}",
            self.name, attrs_str, self.self_closing
        )
    }
}

pub enum TagType {
    Opening(Tag),
    Closing(String),
}

pub fn read_tag(chars: &mut std::iter::Peekable<std::str::Chars>, coord: &mut Coord) -> TagType {
    expect_next(chars, '<', coord);
    if chars.peek() == Some(&'/') {
        chars.next();
        let name = read_until(chars, |ch| ch.is_whitespace() || ch == '>', coord);
        expect_next(chars, '>', coord);
        return TagType::Closing(name);
    }

    let name = read_until(
        chars,
        |ch| ch.is_whitespace() || ch == '/' || ch == '>',
        coord,
    );
    let mut attributes = Vec::new();

    loop {
        read_until(chars, |ch| !ch.is_whitespace(), coord);

        if let Some(&ch) = chars.peek() {
            if ch == '/' || ch == '>' {
                break;
            }

            attributes.push(parse_attr(chars, coord));
        } else {
            panic!(
                "Unexpected end of input while reading attributes at line {}, col {}",
                coord.line, coord.col
            );
        }
    }
    let self_closing = if let Some(&ch) = chars.peek() {
        if ch == '/' {
            chars.next();
            update_coord(ch, coord);
            true
        } else {
            false
        }
    } else {
        false
    };
    expect_next(chars, '>', coord);

    TagType::Opening(Tag {
        name,
        attributes,
        self_closing,
    })
}

fn parse_attr(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    coord: &mut Coord,
) -> (String, AttrType) {
    let attr_name = read_until(chars, |ch| ch == '=' || ch.is_whitespace(), coord);
    expect_next(chars, '=', coord);
    let attr_value = if let Some(&ch) = chars.peek() {
        if ch == '"' {
            chars.next();
            let value = read_until(chars, |c| c == '"', coord);
            expect_next(chars, '"', coord);
            AttrType::Str(value)
        } else if ch == '{' {
            chars.next();
            let value = read_attr_expression(chars, coord);
            expect_next(chars, '}', coord);
            rs_parse::parse_attr_expression(&value, EVENTS
                .iter()
                .any(|(event_name, _, _)| event_name == &attr_name)).expect(&format!(
                "Failed to parse attribute expression for attribute '{}' at line {}, col {}", 
                attr_name, coord.line, coord.col))
        } else {
            panic!(
                "Unexpected character '{}' while reading attribute value at line {}, col {}",
                ch, coord.line, coord.col
            );
        }
    } else {
        panic!(
            "Unexpected end of input while reading attribute value at line {}, col {}",
            coord.line, coord.col
        );
    };

    (attr_name, attr_value)
}

fn read_attr_expression(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    coord: &mut Coord,
) -> String {
    let mut result = String::new();
    let mut open_count = 1; // We start after the first '{'
    while let Some(&ch) = chars.peek() {
        if ch == '{' {
            open_count += 1;
        } else if ch == '}' {
            open_count -= 1;
            if open_count == 0 {
                break;
            }
        }
        chars.next();
        update_coord(ch, coord);
        result.push(ch);
    }
    result
}

use std::fmt::Debug;

use crate::{EVENTS, parse::rs_parse, utils::*};

pub struct IfBranch {
    pub condition: syn::Expr,
    pub contents: Vec<Element>,
}

pub enum ContentType {
    Text(String),
    Expr(syn::Expr),
    Tag(Tag, Vec<Element>), // tag and its contents
    If(Vec<IfBranch>, Option<Vec<Element>>), // if branches, else branch
    Each(syn::Expr, String, Vec<Element>), // iterable expression, item name, contents
}

impl Debug for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContentType::Text(text) => write!(f, "Text({:?})", text),
            ContentType::Expr(_) => write!(f, "Expr(?)"),
            ContentType::Tag(tag, contents) => {
                write!(f, "Tag {{ {:?}, contents: {:?} }}", tag, contents)
            }
            ContentType::If(if_branches, else_branch) => {
                let if_str = if_branches
                    .iter()
                    .map(|branch| format!("if (?) {{ {:?} }}", branch.contents))
                    .collect::<Vec<_>>()
                    .join(" else ");
                let else_str = if let Some(else_contents) = else_branch {
                    format!(" else {{ {:?} }}", else_contents)
                } else {
                    String::new()
                };
                write!(f, "If {{ {}{} }}", if_str, else_str)
            }
            ContentType::Each(_, item_name, contents) => {
                write!(
                    f,
                    "Each {{ for {} in ? {{ {:?} }} }}",
                    item_name, contents
                )
            }
        }
    }
}

#[derive(Debug)]
pub struct Element {
    pub id: u32,
    pub content: ContentType,
}

pub fn read_element_with_tag(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    coord: &mut Coord,
    id_counter: &mut u32,
    tag: Tag,
) -> Element {
    let id = *id_counter;
    *id_counter += 1;

    let contents = if tag.self_closing {
        Vec::new()
    } else {
        let (contents, exit_reason) = read_contents(chars, coord, id_counter);
        assert!(
            matches!(exit_reason, ReadContentExitReason::ClosingTag(ref name) if name == &tag.name)
        );
        contents
    };

    let elem_out = Element {
        id,
        content: ContentType::Tag(tag, contents),
    };

    elem_out
}

enum ReadContentExitReason {
    ClosingTag(String), // Found a closing tag, with the tag name
    ElseIf(syn::Expr), // Found an else if branch, with the condition expression
    Else,              // Found an else branch
    IfClose,           // Found the closing tag for an if block
    EachClose,         // Found the closing tag for an each block
    End,               // Reached the end of input
}

fn read_contents(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    coord: &mut Coord,
    id_counter: &mut u32,
) -> (Vec<Element>, ReadContentExitReason) {
    read_until(chars, |ch| !ch.is_whitespace(), coord);

    let id = *id_counter;
    *id_counter += 1;

    let mut elems = Vec::new();

    while let Some(&ch) = chars.peek() {
        match ch {
            '<' => {
                let next_tag = read_tag(chars, coord);
                match next_tag {
                    TagType::Opening(next_tag) => {
                        let elem = read_element_with_tag(
                            chars, coord, id_counter, next_tag,
                        );
                        elems.push(elem);
                    }
                    TagType::Closing(name) => {
                        return (
                            elems,
                            ReadContentExitReason::ClosingTag(name),
                        );
                    }
                }
                read_until(chars, |ch| !ch.is_whitespace(), coord);
            }
            '{' => {
                chars.next();
                update_coord('{', coord);
                let expr_content = read_attr_expression(chars, coord);
                expect_next(chars, '}', coord);

                if expr_content.starts_with("#if") {
                    let condition_str =
                        expr_content.trim_start_matches("#if").trim();
                    let condition =
                        syn::parse_str::<syn::Expr>(condition_str).unwrap();
                    let mut if_branches = Vec::new();

                    // Read contents of the if block
                    let (mut if_contents, mut exit_reason) =
                        read_contents(chars, coord, id_counter);
                    if_branches.push(IfBranch {
                        condition,
                        contents: if_contents,
                    });

                    // Read else if branches
                    while let ReadContentExitReason::ElseIf(else_if_condition) =
                        exit_reason
                    {
                        (if_contents, exit_reason) =
                            read_contents(chars, coord, id_counter);
                        if_branches.push(IfBranch {
                            condition: else_if_condition,
                            contents: if_contents,
                        });
                    }

                    // Read else branch if present
                    let else_branch =
                        if let ReadContentExitReason::Else = exit_reason {
                            let (else_contents, _) =
                                read_contents(chars, coord, id_counter);
                            Some(else_contents)
                        } else {
                            None
                        };
                    elems.push(Element {
                        id,
                        content: ContentType::If(if_branches, else_branch),
                    });
                } else if expr_content.starts_with("#each") {
                    let each_str =
                        expr_content.trim_start_matches("#each").trim();
                    // Expect format: {#each items as item} - each_str should be "items as item"
                    let parts: Vec<&str> =
                        each_str.split_whitespace().collect();
                    if parts.len() != 3 || parts[1] != "as" {
                        panic!(
                            "Invalid #each expression at line {}, col {}: expected format '{{#each items as item}}'",
                            coord.line, coord.col
                        );
                    }
                    let iterable_expr =
                        syn::parse_str::<syn::Expr>(parts[0]).unwrap();
                    let item_name = parts[2].to_string();

                    // Read contents of the each block
                    let (each_contents, exit_reason) =
                        read_contents(chars, coord, id_counter);
                    assert!(matches!(
                        exit_reason,
                        ReadContentExitReason::EachClose
                    ));
                    elems.push(Element {
                        id,
                        content: ContentType::Each(
                            iterable_expr,
                            item_name,
                            each_contents,
                        ),
                    });
                } else if expr_content.starts_with(":else if") {
                    let condition_str =
                        expr_content.trim_start_matches(":else if").trim();
                    let condition =
                        syn::parse_str::<syn::Expr>(condition_str).unwrap();
                    return (elems, ReadContentExitReason::ElseIf(condition));
                } else if expr_content.starts_with(":else") {
                    return (elems, ReadContentExitReason::Else);
                } else if expr_content.starts_with("/if") {
                    return (elems, ReadContentExitReason::IfClose);
                } else if expr_content.starts_with("/each") {
                    return (elems, ReadContentExitReason::EachClose);
                } else {
                    // Just a normal expression in text content
                    let expr =
                        syn::parse_str::<syn::Expr>(&expr_content).unwrap();
                    elems.push(Element {
                        id,
                        content: ContentType::Expr(expr),
                    });
                }
                read_until(chars, |ch| !ch.is_whitespace(), coord);
            }
            _ => {
                // Text content until the next '{' or '<'
                let text = read_until(chars, |c| c == '{' || c == '<', coord);
                elems.push(Element {
                    id,
                    content: ContentType::Text(text),
                });
            }
        }
    }

    (elems, ReadContentExitReason::End)
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
    Bind(syn::Ident),
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
                    format!(
                        "{}={}",
                        name,
                        quote::ToTokens::to_token_stream(call)
                    )
                }
                AttrType::Expr(expr) => {
                    format!(
                        "{}={}",
                        name,
                        quote::ToTokens::to_token_stream(&expr)
                    )
                }
                AttrType::Closure(_) => {
                    format!("{}=|...| {{ ... }}", name)
                }
                AttrType::Bind(var) => {
                    format!(
                        "{}=bind({})",
                        name,
                        quote::ToTokens::to_token_stream(var)
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

pub fn read_tag(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    coord: &mut Coord,
) -> TagType {
    expect_next(chars, '<', coord);
    if chars.peek() == Some(&'/') {
        chars.next();
        let name =
            read_until(chars, |ch| ch.is_whitespace() || ch == '>', coord);
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
    let attr_name =
        read_until(chars, |ch| ch == '=' || ch.is_whitespace(), coord);
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
            let value = value.trim();

            if attr_name.starts_with("bind:") {
                let name = attr_name.trim_start_matches("bind:").to_string();
                let var_name =
                    syn::Ident::new(&name, proc_macro2::Span::call_site());
                return (name, AttrType::Bind(var_name));
            }

            let attr_is_event_type = EVENTS
                .iter()
                .any(|(event_name, _, _)| event_name == &attr_name);

            rs_parse::parse_attr_expression(value, attr_is_event_type).expect(&format!(
                "Failed to parse attribute expression for attribute '{}' at line {}, col {}",
                attr_name, coord.line, coord.col
            ))
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

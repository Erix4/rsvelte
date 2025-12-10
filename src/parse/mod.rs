use std::{fs::File, io::Read};

use crate::utils::*;

pub mod html_parse;
mod rs_parse;

use html_parse::{TagType, read_closing_tag, read_element_with_tag};
pub use rs_parse::{ScriptData, gen_expr, FuncData, ReactiveVar, StateVar};

pub struct ComponentAST {
    pub body: html_parse::Element,
    pub script: Option<rs_parse::ScriptData>,
    pub style: Option<String>,
}

struct ComponentASTBuilder {
    body: Option<html_parse::Element>,
    script: Option<rs_parse::ScriptData>,
    style: Option<String>,
}

impl Into<ComponentAST> for ComponentASTBuilder {
    fn into(self) -> ComponentAST {
        ComponentAST {
            body: self.body.expect("Component must have a body"),
            script: self.script,
            style: self.style,
        }
    }
}

pub fn parse(filepath: &str) -> Result<ComponentAST, CompileError> {
    let main_page_file = File::open(filepath)?;
    let mut reader = std::io::BufReader::new(main_page_file);

    let mut contents = String::new();
    reader.read_to_string(&mut contents)?;

    let mut coord = Coord { line: 1, col: 0 };

    let mut chars = contents.chars().peekable();

    let mut id_counter = 0;
    let mut builder = ComponentASTBuilder {
        body: None,
        script: None,
        style: None,
    };
    while chars.peek().is_some() {
        read_until(&mut chars, |ch| !ch.is_whitespace(), &mut coord);
        if chars.peek().is_none() {
            break;
        }
        let parent_elem = read_parent_elem(&mut chars, &mut coord, &mut id_counter)?;
        match parent_elem {
            ParentElement::Script(script) => {
                builder.script = Some(script);
            }
            ParentElement::Style(style) => {
                builder.style = Some(style);
            }
            ParentElement::Body(elem) => {
                builder.body = Some(elem);
            }
        }
    }

    Ok(builder.into())
}

enum ParentElement {
    Script(rs_parse::ScriptData),
    Style(String), // CSS is copied verbatim
    Body(html_parse::Element),
}

fn read_parent_elem(
    chars: &mut std::iter::Peekable<std::str::Chars>,
    coord: &mut Coord,
    id_counter: &mut u32,
) -> Result<ParentElement, CompileError> {
    let tag = html_parse::read_tag(chars, coord);
    let tag = match tag {
        TagType::Opening(tag) => tag,
        TagType::Closing(name) => {
            return Err(generic_error(&format!(
                "Unexpected closing tag </{}>",
                name
            )));
        }
    };
    if tag.name == "script" {
        let js = read_until_string(chars, "</script>", coord);
        Ok(ParentElement::Script(rs_parse::parse_script(&js)?))
    } else if tag.name == "style" {
        let css = read_until(chars, |ch| ch == '<', coord);
        read_closing_tag(chars, coord, "style");
        Ok(ParentElement::Style(css))
    } else {
        Ok(ParentElement::Body(read_element_with_tag(
            chars, coord, id_counter, tag,
        )))
    }
}

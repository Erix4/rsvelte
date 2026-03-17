use std::{
    env,
    fs::{self, File},
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
};

use crate::{parse::css_parse::CSSRule, utils::*};

pub mod css_parse;
pub mod html_parse;
mod rs_parse;

use html_parse::{TagType, read_closing_tag, read_element_with_tag};
pub use rs_parse::{Prop, ScriptData, StateVar};

pub fn get_all_components(
    path: &str,
) -> Result<Vec<ComponentAST>, CompileError> {
    let path = fs::canonicalize(path)?;

    let mut components = Vec::new();
    let mut paths_to_process = vec![path];
    let mut seen_paths = std::collections::HashSet::new();

    while let Some(current_path) = paths_to_process.pop() {
        if seen_paths.contains(&current_path) {
            continue;
        }
        seen_paths.insert(current_path.clone());

        let component_ast = parse(current_path.clone())?;

        // Add imported components to paths_to_process
        if let Some(script) = &component_ast.script {
            for import in &script.imports {
                // Check if the import path is absolute or relative
                let resolved_path = resolve_path_location(
                    &import.path,
                    current_path.parent().unwrap(),
                )?;
                paths_to_process.push(resolved_path);
            }
        }

        components.push(component_ast);
    }

    Ok(components)
}

pub struct ComponentAST {
    pub id_hash: String, // Unique hash based on source path for this component, used for generating unique identifiers during codegen
    pub source_path: PathBuf,
    pub body: html_parse::Element,
    pub script: Option<rs_parse::ScriptData>,
    pub style: Option<Vec<CSSRule>>,
}

struct ComponentASTBuilder {
    body: Option<html_parse::Element>,
    script: Option<rs_parse::ScriptData>,
    style: Option<Vec<CSSRule>>,
}

impl ComponentASTBuilder {
    fn into(self, source_path: PathBuf) -> Result<ComponentAST, CompileError> {
        // Create unique hash string for this component
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        Hash::hash(&source_path, &mut hasher);
        let id_hash = format!("{:x}", hasher.finish());

        Ok(ComponentAST {
            id_hash,
            source_path,
            body: self.body.ok_or_else(|| {
                generic_error("No HTML body found in component")
            })?,
            script: self.script,
            style: self.style,
        })
    }
}

pub fn parse(filepath: PathBuf) -> Result<ComponentAST, CompileError> {
    let main_page_file = File::open(&filepath)?;
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
        let parent_elem =
            read_parent_elem(&mut chars, &mut coord, &mut id_counter)?;
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

    builder.into(filepath)
}

enum ParentElement {
    Script(rs_parse::ScriptData),
    Style(Vec<CSSRule>),
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
        Ok(ParentElement::Style(css_parse::parse_css(&css)))
    } else {
        Ok(ParentElement::Body(read_element_with_tag(
            chars, coord, id_counter, tag,
        )))
    }
}

fn resolve_path_location(
    path_str: &str,
    current_dir: &Path,
) -> Result<PathBuf, CompileError> {
    log::info!(
        "Resolving path: '{}' relative to '{}'",
        path_str,
        current_dir.display()
    );
    // TODO: add library paths like $lib and $components
    let path = std::path::Path::new(path_str);
    log::info!("Parsed path: '{}'", path.display());
    env::set_current_dir(current_dir)?;
    log::info!(
        "Changed current directory to '{}'",
        env::current_dir()?.display()
    );
    Ok(fs::canonicalize(path)?)
}

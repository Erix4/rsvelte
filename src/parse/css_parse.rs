use core::panic;

pub struct CSSRule {
    pub selector: String,
    pub declarations: Vec<String>,
    pub is_global: bool,
}

pub fn parse_css(css: &str) -> Vec<CSSRule> {
    let mut rules = Vec::new();
    let mut current_rule = None;

    for line in css.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let is_global_rule = line.starts_with(":global(");
        let line = if is_global_rule {
            // Extract the selector from :global(...)
            if let Some(end_idx) = line.find(')') {
                let selector_contents = &line[8..end_idx].trim();
                // Add bracket back at the end
                &format!("{} {{", selector_contents)
            } else {
                panic!("Malformed :global rule: missing closing parenthesis in '{}'", line);
            }
        } else {
            line
        };

        if line.ends_with('{') {
            // Start of a new rule
            let selector = line[..line.len() - 1].trim().to_string();
            current_rule = Some(CSSRule {
                selector,
                declarations: Vec::new(),
                is_global: is_global_rule,
            });
        } else if line == "}" {
            // End of the current rule
            if let Some(rule) = current_rule.take() {
                rules.push(rule);
            }
        } else if let Some(rule) = &mut current_rule {
            // Declaration within the current rule
            rule.declarations.push(line.to_string());
        }
    }

    rules
}
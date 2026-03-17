pub struct CSSRule {
    pub selector: String,
    pub declarations: Vec<String>,
}

pub fn parse_css(css: &str) -> Vec<CSSRule> {
    let mut rules = Vec::new();
    let mut current_rule = None;

    for line in css.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.ends_with('{') {
            // Start of a new rule
            let selector = line[..line.len() - 1].trim().to_string();
            current_rule = Some(CSSRule {
                selector,
                declarations: Vec::new(),
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
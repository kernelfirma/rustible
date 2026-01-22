//! Tag expression parsing and evaluation.
//!
//! Supports complex tag expressions with AND, OR, NOT operators.

use std::fmt;

/// Error type for tag expression parsing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagExpressionError {
    /// Empty expression
    EmptyExpression,
    /// Invalid operator usage
    InvalidOperator(String),
    /// Unbalanced parentheses
    UnbalancedParentheses,
    /// Invalid character in tag name
    InvalidTagName(String),
}

impl fmt::Display for TagExpressionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyExpression => write!(f, "Empty tag expression"),
            Self::InvalidOperator(op) => write!(f, "Invalid operator: {}", op),
            Self::UnbalancedParentheses => write!(f, "Unbalanced parentheses in expression"),
            Self::InvalidTagName(name) => write!(f, "Invalid tag name: {}", name),
        }
    }
}

impl std::error::Error for TagExpressionError {}

/// A parsed tag expression that can be evaluated against task tags
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagExpression {
    /// Single tag to match
    Tag(String),
    /// Negation: matches if tag is NOT present
    Not(Box<TagExpression>),
    /// Conjunction: matches if ALL expressions match
    And(Vec<TagExpression>),
    /// Disjunction: matches if ANY expression matches
    Or(Vec<TagExpression>),
}

impl TagExpression {
    /// Parse a tag expression from a string.
    ///
    /// # Syntax
    ///
    /// - `tag1` - matches tasks with tag1
    /// - `tag1,tag2` - matches tasks with tag1 OR tag2
    /// - `tag1&tag2` or `tag1+tag2` - matches tasks with tag1 AND tag2
    /// - `!tag1` or `not:tag1` - matches tasks WITHOUT tag1
    /// - `(tag1,tag2)&tag3` - matches tasks with (tag1 OR tag2) AND tag3
    ///
    /// # Examples
    ///
    /// ```rust
    /// use rustible::tags::TagExpression;
    ///
    /// let expr = TagExpression::parse("deploy,web").unwrap();
    /// assert!(expr.matches(&["deploy"]));
    /// assert!(expr.matches(&["web"]));
    /// assert!(!expr.matches(&["database"]));
    ///
    /// let expr = TagExpression::parse("deploy&web").unwrap();
    /// assert!(expr.matches(&["deploy", "web"]));
    /// assert!(!expr.matches(&["deploy"]));
    ///
    /// let expr = TagExpression::parse("!debug").unwrap();
    /// assert!(expr.matches(&["deploy"]));
    /// assert!(!expr.matches(&["debug"]));
    /// ```
    pub fn parse(input: &str) -> Result<Self, TagExpressionError> {
        let input = input.trim();

        if input.is_empty() {
            return Err(TagExpressionError::EmptyExpression);
        }

        // Handle parentheses
        if input.starts_with('(') && input.ends_with(')') {
            // Check if these are matching parens
            let inner = &input[1..input.len()-1];
            let mut depth = 0;
            let mut all_inner = true;
            for ch in inner.chars() {
                match ch {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth < 0 {
                            all_inner = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if all_inner && depth == 0 {
                return Self::parse(inner);
            }
        }

        // Split by AND (lowest precedence) - & or +
        let and_parts = Self::split_by_operator(input, '&');
        if and_parts.len() > 1 {
            let exprs: Result<Vec<_>, _> = and_parts.iter().map(|s| Self::parse(s)).collect();
            return Ok(TagExpression::And(exprs?));
        }

        // Try + as alternative AND
        let and_parts = Self::split_by_operator(input, '+');
        if and_parts.len() > 1 {
            let exprs: Result<Vec<_>, _> = and_parts.iter().map(|s| Self::parse(s)).collect();
            return Ok(TagExpression::And(exprs?));
        }

        // Split by OR (higher precedence) - comma
        let or_parts = Self::split_by_operator(input, ',');
        if or_parts.len() > 1 {
            let exprs: Result<Vec<_>, _> = or_parts.iter().map(|s| Self::parse(s)).collect();
            return Ok(TagExpression::Or(exprs?));
        }

        // Handle NOT - ! or not: prefix
        if let Some(rest) = input.strip_prefix('!') {
            let inner = Self::parse(rest.trim())?;
            return Ok(TagExpression::Not(Box::new(inner)));
        }
        if let Some(rest) = input.strip_prefix("not:") {
            let inner = Self::parse(rest.trim())?;
            return Ok(TagExpression::Not(Box::new(inner)));
        }
        if let Some(rest) = input.strip_prefix("not ") {
            let inner = Self::parse(rest.trim())?;
            return Ok(TagExpression::Not(Box::new(inner)));
        }

        // Validate tag name
        let tag = input.trim();
        if tag.is_empty() {
            return Err(TagExpressionError::EmptyExpression);
        }

        // Check for invalid characters in tag name
        if tag.contains(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != '.') {
            return Err(TagExpressionError::InvalidTagName(tag.to_string()));
        }

        Ok(TagExpression::Tag(tag.to_string()))
    }

    /// Split input by operator, respecting parentheses
    fn split_by_operator(input: &str, op: char) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut depth = 0;
        let mut start = 0;

        for (i, ch) in input.char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                c if c == op && depth == 0 => {
                    parts.push(&input[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }

        parts.push(&input[start..]);
        parts
    }

    /// Check if this expression matches the given task tags
    pub fn matches(&self, task_tags: &[impl AsRef<str>]) -> bool {
        match self {
            TagExpression::Tag(tag) => {
                // Handle special tags
                match tag.to_lowercase().as_str() {
                    "all" => true,
                    "tagged" => !task_tags.is_empty(),
                    "untagged" => task_tags.is_empty(),
                    _ => task_tags.iter().any(|t| t.as_ref().eq_ignore_ascii_case(tag)),
                }
            }
            TagExpression::Not(inner) => !inner.matches(task_tags),
            TagExpression::And(exprs) => exprs.iter().all(|e| e.matches(task_tags)),
            TagExpression::Or(exprs) => exprs.iter().any(|e| e.matches(task_tags)),
        }
    }

    /// Get all tag names referenced in this expression (excluding special tags)
    pub fn referenced_tags(&self) -> Vec<&str> {
        match self {
            TagExpression::Tag(tag) => {
                if super::is_special_tag(tag) {
                    vec![]
                } else {
                    vec![tag.as_str()]
                }
            }
            TagExpression::Not(inner) => inner.referenced_tags(),
            TagExpression::And(exprs) | TagExpression::Or(exprs) => {
                exprs.iter().flat_map(|e| e.referenced_tags()).collect()
            }
        }
    }
}

impl fmt::Display for TagExpression {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TagExpression::Tag(tag) => write!(f, "{}", tag),
            TagExpression::Not(inner) => write!(f, "!{}", inner),
            TagExpression::And(exprs) => {
                let parts: Vec<_> = exprs.iter().map(|e| format!("{}", e)).collect();
                write!(f, "({})", parts.join("&"))
            }
            TagExpression::Or(exprs) => {
                let parts: Vec<_> = exprs.iter().map(|e| format!("{}", e)).collect();
                write!(f, "({})", parts.join(","))
            }
        }
    }
}

/// Parse multiple tag expressions from command-line arguments
///
/// Each tag argument can contain comma-separated tags (OR) or complex expressions
pub fn parse_tag_args(args: &[String]) -> Result<Option<TagExpression>, TagExpressionError> {
    if args.is_empty() {
        return Ok(None);
    }

    // If only one arg, parse it directly
    if args.len() == 1 {
        return Ok(Some(TagExpression::parse(&args[0])?));
    }

    // Multiple args are combined with OR
    let exprs: Result<Vec<_>, _> = args.iter().map(|s| TagExpression::parse(s)).collect();
    Ok(Some(TagExpression::Or(exprs?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_tag() {
        let expr = TagExpression::parse("deploy").unwrap();
        assert_eq!(expr, TagExpression::Tag("deploy".to_string()));
    }

    #[test]
    fn test_parse_or_expression() {
        let expr = TagExpression::parse("deploy,web").unwrap();
        assert!(matches!(expr, TagExpression::Or(_)));

        assert!(expr.matches(&["deploy"]));
        assert!(expr.matches(&["web"]));
        assert!(!expr.matches(&["database"]));
    }

    #[test]
    fn test_parse_and_expression() {
        let expr = TagExpression::parse("deploy&web").unwrap();
        assert!(matches!(expr, TagExpression::And(_)));

        assert!(expr.matches(&["deploy", "web"]));
        assert!(!expr.matches(&["deploy"]));
        assert!(!expr.matches(&["web"]));
    }

    #[test]
    fn test_parse_and_with_plus() {
        let expr = TagExpression::parse("deploy+web").unwrap();
        assert!(matches!(expr, TagExpression::And(_)));

        assert!(expr.matches(&["deploy", "web"]));
    }

    #[test]
    fn test_parse_not_expression() {
        let expr = TagExpression::parse("!debug").unwrap();
        assert!(matches!(expr, TagExpression::Not(_)));

        assert!(expr.matches(&["deploy"]));
        assert!(expr.matches(&[] as &[&str]));
        assert!(!expr.matches(&["debug"]));
    }

    #[test]
    fn test_parse_not_with_prefix() {
        let expr = TagExpression::parse("not:debug").unwrap();
        assert!(expr.matches(&["deploy"]));
        assert!(!expr.matches(&["debug"]));

        let expr = TagExpression::parse("not debug").unwrap();
        assert!(expr.matches(&["deploy"]));
        assert!(!expr.matches(&["debug"]));
    }

    #[test]
    fn test_complex_expression() {
        // (deploy OR web) AND (NOT debug)
        let expr = TagExpression::parse("deploy,web&!debug").unwrap();

        assert!(expr.matches(&["deploy"]));
        assert!(expr.matches(&["web"]));
        assert!(!expr.matches(&["deploy", "debug"]));
        assert!(!expr.matches(&["debug"]));
    }

    #[test]
    fn test_parentheses() {
        let expr = TagExpression::parse("(deploy,web)&production").unwrap();

        assert!(expr.matches(&["deploy", "production"]));
        assert!(expr.matches(&["web", "production"]));
        assert!(!expr.matches(&["deploy"]));
        assert!(!expr.matches(&["production"]));
    }

    #[test]
    fn test_special_tag_all() {
        let expr = TagExpression::parse("all").unwrap();

        assert!(expr.matches(&["deploy"]));
        assert!(expr.matches(&[] as &[&str]));
        assert!(expr.matches(&["anything", "else"]));
    }

    #[test]
    fn test_special_tag_tagged() {
        let expr = TagExpression::parse("tagged").unwrap();

        assert!(expr.matches(&["deploy"]));
        assert!(expr.matches(&["any", "tags"]));
        assert!(!expr.matches(&[] as &[&str]));
    }

    #[test]
    fn test_special_tag_untagged() {
        let expr = TagExpression::parse("untagged").unwrap();

        assert!(expr.matches(&[] as &[&str]));
        assert!(!expr.matches(&["deploy"]));
    }

    #[test]
    fn test_case_insensitive_matching() {
        let expr = TagExpression::parse("Deploy").unwrap();

        assert!(expr.matches(&["deploy"]));
        assert!(expr.matches(&["DEPLOY"]));
        assert!(expr.matches(&["Deploy"]));
    }

    #[test]
    fn test_empty_expression_error() {
        assert!(matches!(
            TagExpression::parse(""),
            Err(TagExpressionError::EmptyExpression)
        ));
    }

    #[test]
    fn test_invalid_tag_name() {
        assert!(matches!(
            TagExpression::parse("deploy@web"),
            Err(TagExpressionError::InvalidTagName(_))
        ));
    }

    #[test]
    fn test_referenced_tags() {
        let expr = TagExpression::parse("deploy,web&!debug").unwrap();
        let tags = expr.referenced_tags();

        assert!(tags.contains(&"deploy"));
        assert!(tags.contains(&"web"));
        assert!(tags.contains(&"debug"));
    }

    #[test]
    fn test_referenced_tags_excludes_special() {
        let expr = TagExpression::parse("deploy,all,tagged").unwrap();
        let tags = expr.referenced_tags();

        assert!(tags.contains(&"deploy"));
        assert!(!tags.contains(&"all"));
        assert!(!tags.contains(&"tagged"));
    }

    #[test]
    fn test_display() {
        let expr = TagExpression::parse("deploy").unwrap();
        assert_eq!(format!("{}", expr), "deploy");

        let expr = TagExpression::parse("!debug").unwrap();
        assert_eq!(format!("{}", expr), "!debug");

        let expr = TagExpression::parse("deploy,web").unwrap();
        assert_eq!(format!("{}", expr), "(deploy,web)");

        let expr = TagExpression::parse("deploy&web").unwrap();
        assert_eq!(format!("{}", expr), "(deploy&web)");
    }

    #[test]
    fn test_parse_tag_args_empty() {
        let result = parse_tag_args(&[]).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_tag_args_single() {
        let result = parse_tag_args(&["deploy".to_string()]).unwrap().unwrap();
        assert_eq!(result, TagExpression::Tag("deploy".to_string()));
    }

    #[test]
    fn test_parse_tag_args_multiple() {
        let result = parse_tag_args(&["deploy".to_string(), "web".to_string()]).unwrap().unwrap();
        assert!(matches!(result, TagExpression::Or(_)));
        assert!(result.matches(&["deploy"]));
        assert!(result.matches(&["web"]));
    }
}

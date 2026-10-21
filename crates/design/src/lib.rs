//! Hand edits (story 05): what can be changed on one JSX element of a TSX file, and the exact text
//! patch for a change. One edit changes one place in the source by a fixed rule; anything else
//! (a computed value, a free layout) is refused with the reason, for the web agent to take on.
//!
//! An element is found by where its `<` is, as `@charet/sdk` marks it (`data-src` on host
//! elements, `data-charet-use` on component instances): a 1-based line and a 1-based column
//! counted in UTF-16 code units, as JavaScript counts them.

use std::collections::BTreeMap;

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, Expression, ImportDeclarationSpecifier, JSXAttributeItem,
    JSXAttributeName, JSXAttributeValue, JSXChild, JSXElement, JSXExpression, ObjectExpression,
    ObjectPropertyKind, Program, Statement,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};
use serde::Serialize;

/// Where an element starts: 1-based line, 1-based UTF-16 column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub line: u32,
    pub column: u32,
}

impl Place {
    /// Splits `path:line:col` (a `data-src` value) into the path and the place.
    pub fn parse_location(value: &str) -> Option<(&str, Place)> {
        let (rest, column) = value.rsplit_once(':')?;
        let (path, line) = rest.rsplit_once(':')?;
        let place = Place {
            line: line.parse().ok()?,
            column: column.parse().ok()?,
        };
        (!path.is_empty() && place.line > 0 && place.column > 0).then_some((path, place))
    }
}

/// A value in the source: written out (and so editable), computed, or not there.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "value", rename_all = "camelCase")]
pub enum Value {
    Absent,
    Literal(String),
    Computed,
}

/// What a hand edit can do to one element.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Element {
    /// `h1`, `Button`, `ui.Card`.
    pub name: String,
    /// A component (capitalised or dotted name) rather than a host element.
    pub component: bool,
    /// Its one text child, trimmed. Computed when the text comes from an expression; absent when
    /// it has no text, or other elements beside it.
    pub text: Value,
    pub class_name: Value,
    /// Attributes written on it (`variant="outline"`); spread ones are left out.
    pub props: BTreeMap<String, Value>,
    /// It sits inside a list (`.map(...)`): a change applies to every item.
    pub in_list: bool,
    /// Which way it can swap places with a sibling element.
    pub moves: Moves,
}

/// Whether an element can swap with the sibling before (`up`) or after (`down`) it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
pub struct Moves {
    pub up: bool,
    pub down: bool,
}

/// One change to an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Edit {
    /// Replace its text (it must have a literal one).
    Text(String),
    /// Set `className` to this literal (empty removes it); a computed one is refused.
    ClassName(String),
    /// Set a string attribute (`None` removes it); a computed one is refused.
    Prop { name: String, value: Option<String> },
    /// Swap with the previous (`-1`) or next (`1`) sibling element.
    Move(i8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesignError {
    /// The file does not parse.
    Syntax(String),
    /// No element starts there.
    NotFound,
    /// The change cannot be made by a hand edit; the reason says why.
    Refused(String),
}

impl std::fmt::Display for DesignError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Syntax(message) => write!(f, "the file does not parse: {message}"),
            Self::NotFound => f.write_str("no element starts at that place (the code moved?)"),
            Self::Refused(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for DesignError {}

fn refused<T>(reason: &str) -> Result<T, DesignError> {
    Err(DesignError::Refused(reason.to_owned()))
}

fn source_type(path: &str) -> SourceType {
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "jsx" || extension == "js" {
        SourceType::jsx()
    } else {
        SourceType::tsx()
    }
}

fn parse<'a>(
    allocator: &'a Allocator,
    path: &str,
    source: &'a str,
) -> Result<Program<'a>, DesignError> {
    let parsed = Parser::new(allocator, source, source_type(path)).parse();
    if let Some(error) = parsed.diagnostics.first() {
        return Err(DesignError::Syntax(error.to_string()));
    }
    Ok(parsed.program)
}

/// Whether `source` still parses (after an edit).
pub fn check_syntax(path: &str, source: &str) -> Result<(), DesignError> {
    let allocator = Allocator::default();
    parse(&allocator, path, source).map(|_| ())
}

/// Byte offset of a place; `None` when the place is not in the file.
fn offset_of(source: &str, place: Place) -> Option<u32> {
    let line = usize::try_from(place.line).ok()?;
    let start = if line == 1 {
        0
    } else {
        source.match_indices('\n').nth(line - 2)?.0 + 1
    };
    let mut units = place.column - 1;
    let mut at = start;
    for c in source[start..].chars() {
        if units == 0 {
            break;
        }
        if c == '\n' {
            return None;
        }
        units = units.checked_sub(u32::try_from(c.len_utf16()).ok()?)?;
        at += c.len_utf8();
    }
    if units != 0 {
        return None;
    }
    u32::try_from(at).ok()
}

fn text(source: &str, span: Span) -> &str {
    &source[span.start as usize..span.end as usize]
}

/// Where the element sits among its siblings.
#[derive(Debug, Clone, Default)]
struct Siblings {
    spans: Vec<Span>,
    index: usize,
}

/// Called once with the element, its siblings and whether it sits in a list.
type Found<'f> = dyn FnMut(&JSXElement<'_>, Option<Siblings>, bool) + 'f;

/// Walks to the element starting at `at` and hands it, with its siblings, to `found`.
struct Finder<'f> {
    at: u32,
    /// Sibling element spans per open parent; `None` when a parent's children are not plain
    /// elements and whitespace (an expression among them could reorder them).
    groups: Vec<Option<Vec<Span>>>,
    lists: u32,
    found: Option<&'f mut Found<'f>>,
}

impl<'a> Visit<'a> for Finder<'_> {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        let list = it
            .callee
            .get_member_expr()
            .and_then(oxc_ast::ast::MemberExpression::static_property_name)
            .is_some_and(|name| matches!(name, "map" | "flatMap"));
        self.lists += u32::from(list);
        walk::walk_call_expression(self, it);
        self.lists -= u32::from(list);
    }

    fn visit_jsx_element(&mut self, it: &JSXElement<'a>) {
        if it.span.start == self.at
            && let Some(found) = self.found.take()
        {
            let siblings = self.groups.last().cloned().flatten().and_then(|spans| {
                let index = spans.iter().position(|s| *s == it.span)?;
                Some(Siblings { spans, index })
            });
            found(it, siblings, self.lists > 0);
            return;
        }
        let plain = it.children.iter().all(|child| match child {
            JSXChild::Element(_) => true,
            JSXChild::Text(text) => text.value.trim().is_empty(),
            _ => false,
        });
        let group = plain.then(|| {
            it.children
                .iter()
                .filter_map(|child| match child {
                    JSXChild::Element(e) => Some(e.span),
                    _ => None,
                })
                .collect()
        });
        self.groups.push(group);
        walk::walk_jsx_element(self, it);
        self.groups.pop();
    }
}

/// Runs `f` on the element at `place`.
fn at_element<T>(
    path: &str,
    source: &str,
    place: Place,
    f: impl FnOnce(&JSXElement<'_>, Option<Siblings>, bool) -> Result<T, DesignError>,
) -> Result<T, DesignError> {
    let allocator = Allocator::default();
    let program = parse(&allocator, path, source)?;
    let at = offset_of(source, place).ok_or(DesignError::NotFound)?;
    let mut f = Some(f);
    let mut out = None;
    let mut found = |element: &JSXElement<'_>, siblings: Option<Siblings>, in_list: bool| {
        if let Some(f) = f.take() {
            out = Some(f(element, siblings, in_list));
        }
    };
    let mut finder = Finder {
        at,
        groups: Vec::new(),
        lists: 0,
        found: Some(&mut found),
    };
    finder.visit_program(&program);
    out.unwrap_or(Err(DesignError::NotFound))
}

fn attribute_name(name: &JSXAttributeName<'_>) -> String {
    match name {
        JSXAttributeName::Identifier(id) => id.name.to_string(),
        JSXAttributeName::NamespacedName(n) => format!("{}:{}", n.namespace.name, n.name.name),
    }
}

/// A string written out: `"x"`, `{"x"}` or `` {`x`} `` without `${}`.
fn literal_value(value: Option<&JSXAttributeValue<'_>>) -> Value {
    match value {
        Some(JSXAttributeValue::StringLiteral(s)) => Value::Literal(s.value.to_string()),
        Some(JSXAttributeValue::ExpressionContainer(c)) => match &c.expression {
            JSXExpression::StringLiteral(s) => Value::Literal(s.value.to_string()),
            JSXExpression::TemplateLiteral(t) if t.expressions.is_empty() => t
                .quasis
                .first()
                .and_then(|q| q.value.cooked.as_ref())
                .map_or(Value::Computed, |c| Value::Literal(c.to_string())),
            _ => Value::Computed,
        },
        _ => Value::Computed,
    }
}

/// The element's one text, and the span to replace when it changes.
fn text_of(element: &JSXElement<'_>, source: &str) -> (Value, Option<Span>) {
    let mut found: Option<(Value, Span)> = None;
    for child in &element.children {
        let (value, span) = match child {
            JSXChild::Text(t) if t.value.trim().is_empty() => continue,
            JSXChild::Text(t) => {
                let raw = text(source, t.span);
                let lead = raw.len() - raw.trim_start().len();
                let trail = raw.len() - raw.trim_end().len();
                let span = Span::new(
                    t.span.start + u32::try_from(lead).unwrap_or(0),
                    t.span.end - u32::try_from(trail).unwrap_or(0),
                );
                (Value::Literal(collapse(&t.value)), span)
            }
            JSXChild::ExpressionContainer(c) => match &c.expression {
                JSXExpression::EmptyExpression(_) => continue,
                JSXExpression::StringLiteral(s) => (Value::Literal(s.value.to_string()), c.span),
                _ => return (Value::Computed, None),
            },
            // Text beside other elements (`Hi <b>you</b>`) is several places, not one.
            _ => return (Value::Absent, None),
        };
        if found.is_some() {
            return (Value::Absent, None);
        }
        found = Some((value, span));
    }
    match found {
        Some((value, span)) => (value, Some(span)),
        None => (Value::Absent, None),
    }
}

/// JSX text as it shows: runs of whitespace with a line break become one space.
fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn name_of(element: &JSXElement<'_>, source: &str) -> String {
    text(source, element.opening_element.name.span()).to_owned()
}

fn is_component(name: &str) -> bool {
    name.contains('.') || name.starts_with(|c: char| c.is_ascii_uppercase())
}

/// What can be changed on the element at `place` in `source` (a file at `path`).
pub fn inspect(path: &str, source: &str, place: Place) -> Result<Element, DesignError> {
    at_element(path, source, place, |element, siblings, in_list| {
        let name = name_of(element, source);
        let mut props = BTreeMap::new();
        let mut class_name = Value::Absent;
        for item in &element.opening_element.attributes {
            let JSXAttributeItem::Attribute(attribute) = item else {
                continue;
            };
            let key = attribute_name(&attribute.name);
            let value = literal_value(attribute.value.as_ref());
            if key == "className" {
                class_name = value;
            } else if !key.starts_with("data-") && key != "key" && key != "ref" {
                props.insert(key, value);
            }
        }
        let (text, _) = text_of(element, source);
        let moves = siblings.as_ref().map_or(Moves::default(), |s| Moves {
            up: s.index > 0,
            down: s.index + 1 < s.spans.len(),
        });
        Ok(Element {
            component: is_component(&name),
            name,
            text,
            class_name,
            props,
            in_list,
            moves,
        })
    })
}

/// `text` as JSX children: plain when it can be, else a string expression.
fn jsx_text(text: &str) -> String {
    if text.is_empty() || text.contains(['{', '}', '<', '>', '&']) || text.trim() != text {
        format!("{{{}}}", js_string(text))
    } else {
        text.to_owned()
    }
}

fn js_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An attribute value as written: `"x"`, or `{"x"}` when it holds a quote.
fn attribute_text(value: &str) -> String {
    if value.contains('"') || value.contains('\n') {
        format!("{{{}}}", js_string(value))
    } else {
        format!("\"{value}\"")
    }
}

/// Replacements, applied last first so earlier offsets stay right.
fn apply_patches(source: &str, mut patches: Vec<(Span, String)>) -> String {
    patches.sort_by_key(|(span, _)| std::cmp::Reverse(span.start));
    let mut out = source.to_owned();
    for (span, with) in patches {
        out.replace_range(span.start as usize..span.end as usize, &with);
    }
    out
}

/// Sets, replaces or removes the attribute `key`.
fn set_attribute(
    element: &JSXElement<'_>,
    source: &str,
    key: &str,
    value: Option<&str>,
) -> Result<Vec<(Span, String)>, DesignError> {
    let opening = &element.opening_element;
    let existing = opening.attributes.iter().find_map(|item| match item {
        JSXAttributeItem::Attribute(a) if attribute_name(&a.name) == key => Some(a),
        _ => None,
    });
    match (existing, value) {
        (Some(attribute), _) if literal_value(attribute.value.as_ref()) == Value::Computed => {
            refused(&format!(
                "`{key}` is computed here; ask the agent to change it"
            ))
        }
        (Some(attribute), Some(value)) => {
            let span = attribute
                .value
                .as_ref()
                .map_or(attribute.span, GetSpan::span);
            let with = if attribute.value.is_some() {
                attribute_text(value)
            } else {
                format!("{key}={}", attribute_text(value))
            };
            Ok(vec![(span, with)])
        }
        (Some(attribute), None) => {
            // Take the whitespace before it too.
            let before = &source[..attribute.span.start as usize];
            let gap = before.len() - before.trim_end().len();
            let start = attribute.span.start - u32::try_from(gap).unwrap_or(0);
            Ok(vec![(Span::new(start, attribute.span.end), String::new())])
        }
        (None, Some(value)) => {
            let after = opening
                .type_arguments
                .as_ref()
                .map_or(opening.name.span().end, |t| t.span.end);
            Ok(vec![(
                Span::new(after, after),
                format!(" {key}={}", attribute_text(value)),
            )])
        }
        (None, None) => Ok(Vec::new()),
    }
}

/// A file after an edit, and where the edited element now starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied {
    pub source: String,
    pub place: Place,
}

/// The place of byte offset `at`.
fn place_at(source: &str, at: u32) -> Place {
    let prefix = &source[..at as usize];
    let last = prefix.rfind('\n').map_or(0, |i| i + 1);
    Place {
        line: u32::try_from(prefix.matches('\n').count()).unwrap_or(0) + 1,
        column: u32::try_from(prefix[last..].encode_utf16().count()).unwrap_or(0) + 1,
    }
}

/// Applies `edit` to the element at `place` in `source`.
pub fn apply(path: &str, source: &str, place: Place, edit: &Edit) -> Result<Applied, DesignError> {
    let mut start = None;
    let patches = at_element(path, source, place, |element, siblings, _| match edit {
        Edit::Text(new) => match text_of(element, source) {
            (Value::Literal(_), Some(span)) => Ok(vec![(span, jsx_text(new))]),
            (Value::Computed, _) => {
                refused("the text is computed here; ask the agent to change it")
            }
            _ => refused("this element has no text of its own to change"),
        },
        Edit::ClassName(new) => {
            let classes = new.split_whitespace().collect::<Vec<_>>().join(" ");
            set_attribute(
                element,
                source,
                "className",
                (!classes.is_empty()).then_some(classes.as_str()),
            )
        }
        Edit::Prop { name, value } => {
            if name == "className" || name == "key" || name == "ref" || name.starts_with("data-") {
                return refused("that attribute is not a hand edit");
            }
            if !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return refused("not an attribute name");
            }
            set_attribute(element, source, name, value.as_deref())
        }
        Edit::Move(by) => {
            let Some(siblings) = siblings else {
                return refused("its place among the others is computed; ask the agent to move it");
            };
            let other = match by {
                -1 if siblings.index > 0 => siblings.index - 1,
                1 if siblings.index + 1 < siblings.spans.len() => siblings.index + 1,
                _ => return refused("it cannot move further that way"),
            };
            let here = siblings.spans[siblings.index];
            let there = siblings.spans[other];
            // Up: it starts where the other one did. Down: after the other one and the gap.
            start = Some(if other < siblings.index {
                there.start
            } else {
                here.start + (there.end - there.start) + (there.start - here.end)
            });
            Ok(vec![
                (here, text(source, there).to_owned()),
                (there, text(source, here).to_owned()),
            ])
        }
    })?;
    let changed = apply_patches(source, patches);
    check_syntax(path, &changed)?;
    let place = start.map_or(place, |at| place_at(&changed, at));
    Ok(Applied {
        source: changed,
        place,
    })
}

/// One `cva` variant: its options and the default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Variant {
    pub options: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
}

struct Cva(BTreeMap<String, Variant>);

fn key_name<'b>(kind: &'b ObjectPropertyKind<'b>) -> Option<(String, &'b Expression<'b>)> {
    let ObjectPropertyKind::ObjectProperty(p) = kind else {
        return None;
    };
    Some((p.key.static_name()?.to_string(), &p.value))
}

fn object<'b>(expression: &'b Expression<'b>) -> Option<&'b ObjectExpression<'b>> {
    match expression {
        Expression::ObjectExpression(o) => Some(o),
        _ => None,
    }
}

impl<'a> Visit<'a> for Cva {
    fn visit_call_expression(&mut self, it: &CallExpression<'a>) {
        if it.callee.is_specific_id("cva")
            && let Some(Argument::ObjectExpression(config)) = it.arguments.get(1)
        {
            let mut defaults = BTreeMap::new();
            for property in &config.properties {
                if let Some((name, value)) = key_name(property)
                    && name == "defaultVariants"
                    && let Some(o) = object(value)
                {
                    for p in &o.properties {
                        if let Some((k, Expression::StringLiteral(s))) = key_name(p) {
                            defaults.insert(k, s.value.to_string());
                        }
                    }
                }
            }
            for property in &config.properties {
                if let Some((name, value)) = key_name(property)
                    && name == "variants"
                    && let Some(o) = object(value)
                {
                    for p in &o.properties {
                        if let Some((variant, Expression::ObjectExpression(options))) = key_name(p)
                        {
                            let entry = self.0.entry(variant.clone()).or_insert(Variant {
                                options: Vec::new(),
                                default: defaults.get(&variant).cloned(),
                            });
                            for option in &options.properties {
                                if let Some((o, _)) = key_name(option)
                                    && !entry.options.contains(&o)
                                {
                                    entry.options.push(o);
                                }
                            }
                        }
                    }
                }
            }
        }
        walk::walk_call_expression(self, it);
    }
}

/// The `cva` variants a file defines (shadcn's `variant`, `size`, ...), with their options.
pub fn variants(path: &str, source: &str) -> Result<BTreeMap<String, Variant>, DesignError> {
    let allocator = Allocator::default();
    let program = parse(&allocator, path, source)?;
    let mut cva = Cva(BTreeMap::new());
    cva.visit_program(&program);
    Ok(cva.0)
}

/// Where each imported name comes from: local name → module specifier.
pub fn imports(path: &str, source: &str) -> Result<BTreeMap<String, String>, DesignError> {
    let allocator = Allocator::default();
    let program = parse(&allocator, path, source)?;
    let mut out = BTreeMap::new();
    for statement in &program.body {
        let Statement::ImportDeclaration(import) = statement else {
            continue;
        };
        for specifier in import.specifiers.iter().flatten() {
            let local = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(s) => &s.local,
                ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => &s.local,
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => &s.local,
            };
            out.insert(local.name.to_string(), import.source.value.to_string());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests;

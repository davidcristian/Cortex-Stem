//! Generates the settings reference, `docs/SETTINGS.md`, from the doc
//! comments and serde attributes of the settings types in `src/settings.rs`
//! (and of `LaunchTarget` in `src/model.rs`), and from the values of
//! `Settings::default()`.
//!
//! `settings_reference_is_up_to_date` fails when the file differs from what
//! it would generate. With `STEM_UPDATE_SETTINGS_REFERENCE=1` it rewrites the
//! file instead. Step 3c of `scripts/check-all.*` and of CI runs it that way,
//! then fails when `git status` shows that the file changed.
//!
//! The parser reads rustfmt-formatted source line by line. It reads only the
//! shapes and serde attributes that the settings types use, and panics on
//! any other shape in a type reachable from `Settings`. A new shape
//! therefore needs a deliberate change here instead of producing a wrong
//! page. The other tests check the page's claims (ranges, allowed values,
//! required keys) against the real deserialisation.

use std::fmt::Write as _;
use std::path::PathBuf;

use serde_json::{Map, Value};
use stem_core::Settings;

/// The files the settings types live in, compiled into this test so that a
/// change to them rebuilds and reruns it.
const SOURCES: [(&str, &str); 2] = [
    (
        "crates/stem-core/src/settings.rs",
        include_str!("../src/settings.rs"),
    ),
    (
        "crates/stem-core/src/model.rs",
        include_str!("../src/model.rs"),
    ),
];

/// The type whose JSON form the page describes.
const ROOT: &str = "Settings";
/// The page, relative to the repository root.
const OUTPUT: &str = "docs/SETTINGS.md";
/// Set to `1` to rewrite [`OUTPUT`] instead of comparing it.
const UPDATE_VAR: &str = "STEM_UPDATE_SETTINGS_REFERENCE";

const REGENERATE: &str =
    "STEM_UPDATE_SETTINGS_REFERENCE=1 cargo test -p stem-core --test settings_reference";

// ---------------------------------------------------------------------------
// Parsing the Rust source.
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Field {
    name: String,
    doc: String,
    serde: Vec<String>,
    ty: String,
}

#[derive(Debug)]
enum VariantShape {
    Unit,
    Struct(Vec<Field>),
    /// Tuple variants, which no settings type uses.
    Other,
}

#[derive(Debug)]
struct Variant {
    name: String,
    doc: String,
    serde: Vec<String>,
    shape: VariantShape,
}

#[derive(Debug)]
enum Shape {
    Struct(Vec<Field>),
    Enum(Vec<Variant>),
    /// `pub struct Name(pub Inner);`
    Newtype(String),
}

#[derive(Debug)]
struct Item {
    name: String,
    source: &'static str,
    doc: String,
    serde: Vec<String>,
    shape: Shape,
    /// Lines of the body the parser could not read; an error only if the
    /// item is reached from [`ROOT`].
    problems: Vec<String>,
}

/// Doc comment lines and attributes waiting for the declaration they belong to.
#[derive(Default)]
struct Pending {
    doc: Vec<String>,
    attrs: Vec<String>,
}

impl Pending {
    /// Consumes `lines[*i]` (and the rest of a multi-line attribute) when it is
    /// a doc comment or an attribute.
    fn absorb(&mut self, lines: &[&str], i: &mut usize) -> bool {
        let line = lines[*i].trim();
        if let Some(text) = line.strip_prefix("///") {
            if !text.starts_with('/') {
                self.doc
                    .push(text.strip_prefix(' ').unwrap_or(text).to_owned());
                *i += 1;
                return true;
            }
        }
        if line.starts_with("#[") {
            let mut attr = String::new();
            loop {
                attr.push_str(lines[*i].trim());
                *i += 1;
                if brackets_balanced(&attr) || *i >= lines.len() {
                    break;
                }
                attr.push(' ');
            }
            self.attrs.push(attr);
            return true;
        }
        false
    }

    /// The collected doc comment (one line) and serde entries; resets.
    fn take(&mut self) -> (String, Vec<String>) {
        let doc = self
            .doc
            .iter()
            .map(|line| line.trim())
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let serde = self.attrs.iter().flat_map(|a| serde_entries(a)).collect();
        self.doc.clear();
        self.attrs.clear();
        (doc, serde)
    }
}

fn parse_items() -> Vec<Item> {
    let mut items = Vec::new();
    for (source, text) in SOURCES {
        let lines: Vec<&str> = text.lines().collect();
        let mut pending = Pending::default();
        let mut i = 0;
        while i < lines.len() {
            if pending.absorb(&lines, &mut i) {
                continue;
            }
            let line = lines[i];
            i += 1;
            // Items are declared at column 0; anything indented (impl blocks,
            // the test module) is not an item of interest.
            let header = line
                .strip_prefix("pub struct ")
                .map(|rest| (false, rest))
                .or_else(|| line.strip_prefix("pub enum ").map(|rest| (true, rest)));
            let Some((is_enum, rest)) = header else {
                pending.take();
                continue;
            };
            let (doc, serde) = pending.take();
            let name = ident(rest);
            let after = rest[name.len()..].trim();
            let mut problems = Vec::new();
            let shape = if after == "{" {
                let start = i;
                while i < lines.len() && lines[i] != "}" {
                    i += 1;
                }
                let body = &lines[start..i];
                i += 1;
                if is_enum {
                    Shape::Enum(parse_variants(body, &mut problems))
                } else {
                    Shape::Struct(parse_fields(body, &mut problems))
                }
            } else if let Some(inner) = after
                .strip_prefix("(pub ")
                .and_then(|r| r.strip_suffix(");"))
                .filter(|_| !is_enum)
            {
                Shape::Newtype(inner.trim().to_owned())
            } else {
                continue;
            };
            items.push(Item {
                name: name.to_owned(),
                source,
                doc,
                serde,
                shape,
                problems,
            });
        }
    }
    items
}

fn parse_fields(body: &[&str], problems: &mut Vec<String>) -> Vec<Field> {
    let mut fields = Vec::new();
    let mut pending = Pending::default();
    let mut i = 0;
    while i < body.len() {
        if pending.absorb(body, &mut i) {
            continue;
        }
        let line = body[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let (doc, serde) = pending.take();
        match field_decl(line) {
            Some((name, ty)) => fields.push(Field {
                name,
                doc,
                serde,
                ty,
            }),
            None => problems.push(line.to_owned()),
        }
    }
    fields
}

/// `pub name: Type,` (any visibility) as `(name, Type)`.
fn field_decl(line: &str) -> Option<(String, String)> {
    let decl = match line.strip_prefix("pub(") {
        Some(rest) => rest.split_once(')')?.1.trim_start(),
        None => line.strip_prefix("pub ").unwrap_or(line),
    };
    let (name, ty) = decl.split_once(':')?;
    let ty = ty.trim().trim_end_matches(',').trim();
    let name = name.trim();
    (!ty.is_empty() && !name.is_empty() && ident(name) == name)
        .then(|| (name.to_owned(), ty.to_owned()))
}

fn parse_variants(body: &[&str], problems: &mut Vec<String>) -> Vec<Variant> {
    let mut variants = Vec::new();
    let mut pending = Pending::default();
    let mut i = 0;
    while i < body.len() {
        if pending.absorb(body, &mut i) {
            continue;
        }
        let line = body[i].trim();
        i += 1;
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let (doc, serde) = pending.take();
        let name = ident(line);
        if name.is_empty() {
            problems.push(line.to_owned());
            continue;
        }
        let rest = line[name.len()..].trim();
        let shape = if rest.is_empty() || rest == "," || rest.starts_with('=') {
            VariantShape::Unit
        } else if rest == "{" {
            let start = i;
            while i < body.len() && !matches!(body[i].trim(), "}," | "}") {
                i += 1;
            }
            let fields = parse_fields(&body[start..i], problems);
            i += 1;
            VariantShape::Struct(fields)
        } else if let Some(inner) = rest
            .strip_prefix('{')
            .and_then(|r| r.trim_end_matches(',').strip_suffix('}'))
        {
            let mut fields = Vec::new();
            for decl in split_top_level(inner).into_iter().filter(|d| !d.is_empty()) {
                match field_decl(decl) {
                    Some((name, ty)) => fields.push(Field {
                        name,
                        doc: String::new(),
                        serde: Vec::new(),
                        ty,
                    }),
                    None => problems.push(line.to_owned()),
                }
            }
            VariantShape::Struct(fields)
        } else {
            if rest == "(" {
                while i < body.len() && !body[i].trim().starts_with(')') {
                    i += 1;
                }
                i += 1;
            }
            VariantShape::Other
        };
        variants.push(Variant {
            name: name.to_owned(),
            doc,
            serde,
            shape,
        });
    }
    variants
}

/// The identifier at the start of `s`.
fn ident(s: &str) -> &str {
    let end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    &s[..end]
}

/// Whether every `[` in `s` is closed, ignoring string literals.
fn brackets_balanced(s: &str) -> bool {
    let (mut depth, mut in_str, mut escaped) = (0i32, false, false);
    for c in s.chars() {
        if in_str {
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
    }
    depth == 0
}

/// Splits at the commas outside brackets and string literals; trims parts.
fn split_top_level(s: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut depth, mut in_str, mut start) = (0i32, false, 0);
    for (i, c) in s.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '(' | '[' | '{' | '<' if !in_str => depth += 1,
            ')' | ']' | '}' | '>' if !in_str => depth -= 1,
            ',' if !in_str && depth == 0 => {
                parts.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].trim());
    parts
}

/// The entries of a `#[serde(...)]` attribute; nothing for other attributes.
fn serde_entries(attr: &str) -> Vec<String> {
    attr.strip_prefix("#[serde(")
        .and_then(|a| a.strip_suffix(")]"))
        .map(|inner| {
            split_top_level(inner)
                .into_iter()
                .filter(|e| !e.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn serde_key(entry: &str) -> &str {
    entry.split('=').next().unwrap_or(entry).trim()
}

fn serde_value<'e>(serde: &'e [String], key: &str) -> Option<&'e str> {
    serde
        .iter()
        .find(|e| serde_key(e) == key)
        .and_then(|e| e.split_once('='))
        .map(|(_, v)| v.trim().trim_matches('"'))
}

fn has_serde(serde: &[String], key: &str) -> bool {
    serde.iter().any(|e| serde_key(e) == key)
}

/// Panics on a serde attribute entry the generator does not model.
fn check_serde(serde: &[String], allowed: &[&str], what: &str) {
    for entry in serde {
        assert!(
            allowed.contains(&serde_key(entry)),
            "{what}: #[serde({entry})] is not modelled by \
             crates/stem-core/tests/settings_reference.rs; teach it what the \
             attribute does to the JSON before relying on the reference"
        );
    }
}

/// Whether a container's `rename_all` asks for camelCase (the only rule used).
fn camel_case(serde: &[String], what: &str) -> bool {
    match serde_value(serde, "rename_all") {
        None => false,
        Some("camelCase") => true,
        Some(other) => panic!("{what}: rename_all = \"{other}\" is not modelled"),
    }
}

fn camel_from_snake(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.extend(c.to_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

fn camel_from_pascal(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}

fn field_key(field: &Field, camel: bool) -> String {
    if camel {
        camel_from_snake(&field.name)
    } else {
        field.name.clone()
    }
}

fn variant_value(variant: &Variant, camel: bool) -> String {
    if camel {
        camel_from_pascal(&variant.name)
    } else {
        variant.name.clone()
    }
}

/// `Outer<Inner>` → `Inner`.
fn generic<'t>(ty: &'t str, outer: &str) -> Option<&'t str> {
    ty.strip_prefix(outer)?.strip_prefix('<')?.strip_suffix('>')
}

fn find<'i>(items: &'i [Item], name: &str, used_at: &str) -> &'i Item {
    let item = items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| {
            panic!(
                "{used_at}: no rule for the type `{name}`: it is not a primitive the \
                 generator knows, nor a `pub struct`/`pub enum` in {}",
                SOURCES.map(|(path, _)| path).join(" or ")
            )
        });
    assert!(
        item.problems.is_empty(),
        "{}: the parser could not read these lines of `{name}`: {:?}",
        item.source,
        item.problems
    );
    item
}

// ---------------------------------------------------------------------------
// Doc comment text.
// ---------------------------------------------------------------------------

/// Turns intra-doc links (`` [`Name`] ``) into plain code spans.
fn strip_doc_links(doc: &str) -> String {
    let mut out = String::with_capacity(doc.len());
    let mut rest = doc;
    while let Some(start) = rest.find("[`") {
        let Some(len) = rest[start + 2..].find("`]") else {
            break;
        };
        let end = start + 2 + len + 2;
        if matches!(rest[end..].chars().next(), Some('(' | '[')) {
            // A real Markdown link; keep it.
            out.push_str(&rest[..end]);
        } else {
            out.push_str(&rest[..start]);
            out.push_str(&rest[start + 1..end - 1]);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || c == '.') && s.parse::<f64>().is_ok()
}

/// A documented inclusive range such as `(24..=128)`, removed from the text.
fn extract_range(doc: &str) -> (Option<(String, String)>, String) {
    let found = doc.find("..=").and_then(|op| {
        let open = doc[..op].rfind('(')?;
        let close = op + doc[op..].find(')')?;
        let (lo, hi) = (&doc[open + 1..op], &doc[op + 3..close]);
        (is_number(lo) && is_number(hi)).then_some((open, close, lo, hi))
    });
    match found {
        Some((open, close, lo, hi)) => {
            let mut text = doc[..open].trim_end().to_owned();
            text.push_str(&doc[close + 1..]);
            (Some((lo.to_owned(), hi.to_owned())), text)
        }
        None => (None, doc.to_owned()),
    }
}

// ---------------------------------------------------------------------------
// The reference.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Seg {
    Key(String),
    /// Each element of an array.
    Each,
}

#[derive(Debug, Clone, PartialEq, Default)]
struct KeyPath(Vec<Seg>);

impl KeyPath {
    fn key(&self, key: &str) -> Self {
        let mut segs = self.0.clone();
        segs.push(Seg::Key(key.to_owned()));
        Self(segs)
    }

    fn each(&self) -> Self {
        let mut segs = self.0.clone();
        segs.push(Seg::Each);
        Self(segs)
    }

    /// The keys from the root, when the path crosses no array.
    fn keys(&self) -> Option<Vec<&str>> {
        self.0
            .iter()
            .map(|seg| match seg {
                Seg::Key(key) => Some(key.as_str()),
                Seg::Each => None,
            })
            .collect()
    }

    fn display(&self) -> String {
        let mut out = String::new();
        for seg in &self.0 {
            match seg {
                Seg::Key(key) => {
                    if !out.is_empty() {
                        out.push('.');
                    }
                    out.push_str(key);
                }
                Seg::Each => out.push_str("[]"),
            }
        }
        out
    }

    fn title(&self) -> String {
        if self.0.is_empty() {
            "Top level".to_owned()
        } else {
            format!("`{}`", self.display())
        }
    }

    fn link(&self) -> String {
        let title = self.title();
        format!("[{title}](#{})", anchor(&title))
    }
}

/// GitHub's heading anchor: lower case, punctuation dropped, spaces to `-`.
fn anchor(title: &str) -> String {
    title
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => c.to_lowercase().next(),
            _ => None,
        })
        .collect()
}

#[derive(Debug)]
struct Row {
    key: String,
    ty: String,
    default: String,
    description: String,
}

#[derive(Debug)]
struct Section {
    title: String,
    intro: String,
    rows: Vec<Row>,
}

/// A documented range, for the test that checks it against validation.
#[derive(Debug)]
struct RangeClaim {
    path: KeyPath,
    lo: String,
    hi: String,
    integer: bool,
}

/// The documented values of an enum-typed key.
#[derive(Debug)]
struct ValuesClaim {
    path: KeyPath,
    values: Vec<String>,
}

#[derive(Debug, Default)]
struct Reference {
    sections: Vec<Section>,
    sources: Vec<&'static str>,
    ranges: Vec<RangeClaim>,
    values: Vec<ValuesClaim>,
}

/// A type the generator describes in a section of its own.
struct Child {
    name: String,
    path: KeyPath,
    /// The object's default keys, when the parent's default has the object.
    defaults: Option<Map<String, Value>>,
}

struct Generator<'i> {
    items: &'i [Item],
    reference: Reference,
}

/// The JSON form of `Settings::default()`. Serialised to text first so that
/// floats keep their short `f32` form (`0.72`, not `0.7200000286102295`).
fn default_json() -> Map<String, Value> {
    let text = serde_json::to_string(&Settings::default()).expect("serialise defaults");
    match serde_json::from_str(&text).expect("parse defaults") {
        Value::Object(map) => map,
        other => panic!("Settings::default() is not a JSON object: {other}"),
    }
}

fn generate(items: &[Item]) -> Reference {
    let mut generator = Generator {
        items,
        reference: Reference::default(),
    };
    generator.child(Child {
        name: ROOT.to_owned(),
        path: KeyPath::default(),
        defaults: Some(default_json()),
    });
    generator.reference
}

impl Generator<'_> {
    fn child(&mut self, child: Child) {
        let item = find(self.items, &child.name, &child.path.title());
        self.visit(item);
        match &item.shape {
            Shape::Struct(fields) => self.struct_section(item, fields, &child),
            Shape::Enum(variants) => self.tagged_section(item, variants, &child.path),
            Shape::Newtype(_) => unreachable!("newtypes are described inline"),
        }
    }

    /// Records the file an item came from, for the page's list of sources.
    fn visit(&mut self, item: &Item) {
        if !self.reference.sources.contains(&item.source) {
            self.reference.sources.push(item.source);
        }
    }

    fn struct_section(&mut self, item: &Item, fields: &[Field], child: &Child) {
        let what = format!("{}::{}", item.source, item.name);
        check_serde(&item.serde, &["rename_all", "default"], &what);
        let camel = camel_case(&item.serde, &what);
        if child.defaults.is_none() {
            assert!(
                !has_serde(&item.serde, "default"),
                "{what}: an array element with #[serde(default)] needs an instance \
                 to read its defaults from; the generator has none"
            );
        }
        let mut rows = Vec::new();
        let mut keys = Vec::new();
        let mut children = Vec::new();
        for field in fields {
            let what = format!("{what}::{}", field.name);
            check_serde(&field.serde, &["default"], &what);
            let key = field_key(field, camel);
            keys.push(key.clone());
            let path = child.path.key(&key);
            let mut values = Vec::new();
            let mut ty = self.type_text(&field.ty, &path, &mut children, &mut values);
            let (range, doc) = extract_range(&strip_doc_links(&field.doc));
            if let Some((lo, hi)) = range {
                let integer = ty == "integer";
                assert!(
                    integer || ty == "number",
                    "{what}: a documented range on a {ty}"
                );
                write!(ty, ", {lo} to {hi}").expect("write to a String");
                self.reference.ranges.push(RangeClaim {
                    path: path.clone(),
                    lo,
                    hi,
                    integer,
                });
            }
            if !values.is_empty() {
                self.reference.values.push(ValuesClaim {
                    path: path.clone(),
                    values: values.iter().map(|(value, _)| value.clone()).collect(),
                });
            }
            let default = match &child.defaults {
                Some(defaults) => {
                    let value = defaults.get(&key).unwrap_or_else(|| {
                        panic!(
                            "{what}: Settings::default() has no key `{}`; the \
                             parser and serde disagree",
                            path.display()
                        )
                    });
                    if let Some(last) = children.last_mut().filter(|c| c.path == path) {
                        last.defaults = Some(match value {
                            Value::Object(map) => map.clone(),
                            other => panic!("{what}: default {other} is not an object"),
                        });
                        "per key".to_owned()
                    } else {
                        code(value)
                    }
                }
                None => element_default(field, &what),
            };
            rows.push(Row {
                key: path.display(),
                ty,
                default,
                description: with_values(doc, &values),
            });
        }
        if let Some(defaults) = &child.defaults {
            for key in defaults.keys() {
                assert!(
                    keys.contains(key),
                    "{what}: Settings::default() has the key `{key}`, which the \
                     parser did not find; the parser and serde disagree"
                );
            }
        }
        self.reference.sections.push(Section {
            title: child.path.title(),
            intro: strip_doc_links(&item.doc),
            rows,
        });
        for child in children {
            self.child(child);
        }
    }

    /// An internally tagged enum: one row for the tag, one per variant field.
    fn tagged_section(&mut self, item: &Item, variants: &[Variant], path: &KeyPath) {
        let what = format!("{}::{}", item.source, item.name);
        let tag = serde_value(&item.serde, "tag")
            .unwrap_or_else(|| panic!("{what}: only internally tagged enums get a section"));
        let camel = camel_case(&item.serde, &what);
        let kinds: Vec<(String, String)> = variants
            .iter()
            .map(|v| {
                (
                    json_string(&variant_value(v, camel)),
                    strip_doc_links(&v.doc),
                )
            })
            .collect();
        let tag_path = path.key(tag);
        self.reference.values.push(ValuesClaim {
            path: tag_path.clone(),
            values: kinds.iter().map(|(value, _)| value.clone()).collect(),
        });
        let mut rows = vec![Row {
            key: tag_path.display(),
            ty: one_of(&kinds),
            default: "required".to_owned(),
            description: with_values("Sets which of the other keys apply.".to_owned(), &kinds),
        }];
        // Variant fields, merged when several variants share an identical key.
        let mut fields: Vec<(Row, Vec<String>)> = Vec::new();
        let mut children = Vec::new();
        for (variant, (kind, _)) in variants.iter().zip(&kinds) {
            let what = format!("{what}::{}", variant.name);
            check_serde(&variant.serde, &["rename_all"], &what);
            let variant_camel = camel_case(&variant.serde, &what);
            let variant_fields = match &variant.shape {
                VariantShape::Unit => continue,
                VariantShape::Struct(fields) => fields,
                VariantShape::Other => panic!("{what}: tuple variants are not modelled"),
            };
            for field in variant_fields {
                let what = format!("{what}::{}", field.name);
                check_serde(&field.serde, &["default"], &what);
                let key_path = path.key(&field_key(field, variant_camel));
                let mut values = Vec::new();
                let ty = self.type_text(&field.ty, &key_path, &mut children, &mut values);
                let (range, doc) = extract_range(&strip_doc_links(&field.doc));
                assert!(
                    range.is_none(),
                    "{what}: ranges inside arrays are not checked"
                );
                let row = Row {
                    key: key_path.display(),
                    ty,
                    default: element_default(field, &what),
                    description: with_values(doc, &values),
                };
                match fields.iter_mut().find(|(seen, _)| {
                    (&seen.key, &seen.ty, &seen.default, &seen.description)
                        == (&row.key, &row.ty, &row.default, &row.description)
                }) {
                    Some((_, used_by)) => used_by.push(kind.clone()),
                    None => fields.push((row, vec![kind.clone()])),
                }
            }
        }
        for (mut row, used_by) in fields {
            let only = format!(
                "Only when `{tag}` is {}.",
                join_or(&used_by.iter().map(|k| format!("`{k}`")).collect::<Vec<_>>())
            );
            row.description = if row.description.is_empty() {
                only
            } else {
                format!("{} {only}", row.description)
            };
            rows.push(row);
        }
        self.reference.sections.push(Section {
            title: path.title(),
            intro: strip_doc_links(&item.doc),
            rows,
        });
        for child in children {
            self.child(child);
        }
    }

    /// The "Type" column for `ty`; records the types that get a section of
    /// their own in `children`, and an enum's values in `values`.
    fn type_text(
        &mut self,
        ty: &str,
        path: &KeyPath,
        children: &mut Vec<Child>,
        values: &mut Vec<(String, String)>,
    ) -> String {
        match ty {
            "bool" => return "boolean".to_owned(),
            "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize" => {
                return "integer".to_owned();
            }
            "f32" | "f64" => return "number".to_owned(),
            "String" | "PathBuf" => return "string".to_owned(),
            _ => {}
        }
        if let Some(inner) = generic(ty, "Option") {
            return format!(
                "{} or `null`",
                self.type_text(inner, path, children, values)
            );
        }
        if let Some(inner) = generic(ty, "Vec") {
            let element = self.type_text(inner, &path.each(), children, values);
            return match element.split_once(',') {
                Some((first, rest)) if !first.contains(' ') => format!("array of {first}s,{rest}"),
                _ if !element.contains(' ') => format!("array of {element}s"),
                _ => format!("array; each entry: {element}"),
            };
        }
        let item = find(self.items, ty, &path.title());
        self.visit(item);
        let what = format!("{}::{}", item.source, item.name);
        match &item.shape {
            Shape::Newtype(inner) => {
                check_serde(&item.serde, &["transparent"], &what);
                assert!(
                    has_serde(&item.serde, "transparent"),
                    "{what}: only #[serde(transparent)] newtypes are modelled"
                );
                self.type_text(inner, path, children, values)
            }
            Shape::Struct(_) => {
                children.push(Child {
                    name: item.name.clone(),
                    path: path.clone(),
                    defaults: None,
                });
                format!("object, see {}", path.link())
            }
            Shape::Enum(variants) => {
                check_serde(&item.serde, &["rename_all", "tag"], &what);
                if has_serde(&item.serde, "tag") {
                    children.push(Child {
                        name: item.name.clone(),
                        path: path.clone(),
                        defaults: None,
                    });
                    return format!("object, see {}", path.link());
                }
                let camel = camel_case(&item.serde, &what);
                for variant in variants {
                    assert!(
                        matches!(variant.shape, VariantShape::Unit),
                        "{what}::{}: an untagged enum with data is not modelled",
                        variant.name
                    );
                    check_serde(&variant.serde, &[], &what);
                }
                *values = variants
                    .iter()
                    .map(|v| {
                        (
                            json_string(&variant_value(v, camel)),
                            strip_doc_links(&v.doc),
                        )
                    })
                    .collect();
                one_of(values)
            }
        }
    }
}

/// The "Default" column of a field that has no default instance (an array
/// element): what serde fills in when the key is missing, or "required".
fn element_default(field: &Field, what: &str) -> String {
    if generic(&field.ty, "Option").is_some() {
        return "`null`".to_owned();
    }
    if !has_serde(&field.serde, "default") {
        return "required".to_owned();
    }
    if generic(&field.ty, "Vec").is_some() {
        return "`[]`".to_owned();
    }
    match field.ty.as_str() {
        "String" | "PathBuf" => "`\"\"`".to_owned(),
        "bool" => "`false`".to_owned(),
        _ => panic!("{what}: no rule for the serde default of {}", field.ty),
    }
}

fn json_string(s: &str) -> String {
    Value::String(s.to_owned()).to_string()
}

fn code(value: &Value) -> String {
    format!("`{value}`")
}

fn join_or(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

fn one_of(values: &[(String, String)]) -> String {
    join_or(
        &values
            .iter()
            .map(|(value, _)| format!("`{value}`"))
            .collect::<Vec<_>>(),
    )
}

/// The description followed by one line per allowed value.
fn with_values(description: String, values: &[(String, String)]) -> String {
    let mut out = description;
    for (value, doc) in values {
        if !out.is_empty() {
            out.push_str("<br>");
        }
        if doc.is_empty() {
            write!(out, "`{value}`").expect("write to a String");
        } else {
            write!(out, "`{value}`: {doc}").expect("write to a String");
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Markdown.
// ---------------------------------------------------------------------------

fn render(reference: &Reference) -> String {
    let mut out = String::new();
    out.push_str(
        "<!-- Generated by crates/stem-core/tests/settings_reference.rs. Do not edit this file by hand. -->\n\
         \n\
         # Settings reference\n\
         \n\
         Every key of Stem's settings file, `settings.json`, with its type, default and\n\
         meaning. The page is generated from the doc comments, serde attributes and\n\
         defaults of the settings types in:\n\
         \n",
    );
    for path in &reference.sources {
        writeln!(out, "- [`{path}`](../{path})").expect("write to a String");
    }
    write!(
        out,
        "\n\
         To change the page, change the source, then regenerate the page and commit\n\
         it:\n\
         \n\
         ```sh\n\
         {REGENERATE}\n\
         ```\n\
         \n\
         In PowerShell, set the variable with `$env:{UPDATE_VAR} = '1'`\n\
         first and remove it afterwards with `Remove-Item Env:{UPDATE_VAR}`.\n\
         The check scripts (`scripts/check-all.sh` and `scripts/check-all.ps1`) and\n\
         CI fail while the page is out of date.\n\
         \n\
         The [user guide](user-guide/settings-window.md) says where the file lives, how the\n\
         settings window names each setting, and how the dock behaves.\n\
         \n\
         A dotted key such as `magnification.maxScale` is the `maxScale` key inside the\n\
         `magnification` object, and `pinned[]` stands for each entry of the `pinned`\n\
         array.\n\
         \n\
         A key missing from the file takes its default, a key that this page does not\n\
         list is ignored, and a number outside its documented range is clamped to the\n\
         nearest end of it. The whole file is unreadable when it is not valid JSON, or\n\
         when it has one of these values:\n\
         \n\
         - a value of the wrong JSON type, such as `\"48\"` for a number, `\"true\"` for a\n\
         \x20 boolean or `null` for an object;\n\
         - a string that is not one of the key's listed values;\n\
         - for an integer key, a negative number, a number with a decimal point (even\n\
         \x20 `48.0`) or one above `4294967295`;\n\
         - a `pinned[]` or `hiddenApps[]` entry without one of its required keys;\n\
         - a `version` higher than the one this build of Stem writes.\n\
         \n\
         Stem then keeps the file as `settings.json.broken` and starts with the\n\
         defaults; see the [user guide](user-guide/settings-window.md).\n\
         \n"
    )
    .expect("write to a String");
    for section in &reference.sections {
        writeln!(out, "- [{}](#{})", section.title, anchor(&section.title))
            .expect("write to a String");
    }
    for section in &reference.sections {
        write!(out, "\n## {}\n\n", section.title).expect("write to a String");
        if !section.intro.is_empty() {
            write!(out, "{}\n\n", section.intro).expect("write to a String");
        }
        out.push_str(&table(&section.rows));
    }
    out
}

/// A Markdown table with every column padded to its widest cell, the way
/// Prettier formats the hand-written docs.
fn table(rows: &[Row]) -> String {
    let header = ["Key", "Type", "Default", "Description"].map(str::to_owned);
    let cells: Vec<[String; 4]> = rows
        .iter()
        .map(|row| {
            [
                format!("`{}`", row.key),
                row.ty.clone(),
                row.default.clone(),
                row.description.clone(),
            ]
            .map(|cell| cell.replace('|', "\\|"))
        })
        .collect();
    let mut widths = header.clone().map(|h| h.chars().count());
    for row in &cells {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |cells: &[String; 4]| {
        let padded: Vec<String> = cells
            .iter()
            .zip(widths)
            .map(|(cell, width)| {
                let pad = width - cell.chars().count();
                format!("{cell}{}", " ".repeat(pad))
            })
            .collect();
        format!("| {} |\n", padded.join(" | "))
    };
    let mut out = line(&header);
    let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    writeln!(out, "| {} |", rule.join(" | ")).expect("write to a String");
    for row in &cells {
        out.push_str(&line(row));
    }
    out
}

fn output_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(OUTPUT)
}

fn first_difference(current: &str, generated: &str) -> String {
    let mut current_lines = current.lines();
    let mut generated_lines = generated.lines();
    let mut number = 1;
    loop {
        match (current_lines.next(), generated_lines.next()) {
            (Some(a), Some(b)) if a == b => number += 1,
            (Some(a), Some(b)) => {
                // An excerpt of each line around the first character that differs.
                let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
                let column = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
                let start = column.saturating_sub(30);
                let excerpt = |s: &[char]| s.iter().skip(start).take(80).collect::<String>();
                return format!(
                    "line {number}, column {}: the file has `{}`, the source gives `{}`",
                    column + 1,
                    excerpt(&a),
                    excerpt(&b)
                );
            }
            (Some(_), None) => return format!("the file has extra lines from line {number}"),
            (None, Some(_)) => return format!("the file ends before line {number}"),
            (None, None) => return "only line endings or the final newline differ".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests.
// ---------------------------------------------------------------------------

#[test]
fn settings_reference_is_up_to_date() {
    let generated = render(&generate(&parse_items()));
    let path = output_path();
    let current = std::fs::read_to_string(&path)
        .ok()
        .map(|text| text.replace("\r\n", "\n"));
    if current.as_deref() == Some(generated.as_str()) {
        return;
    }
    if std::env::var(UPDATE_VAR).is_ok_and(|value| value == "1") {
        std::fs::write(&path, &generated)
            .unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
        return;
    }
    let detail = match &current {
        None => format!("{OUTPUT} does not exist"),
        Some(current) => first_difference(current, &generated),
    };
    panic!(
        "{OUTPUT} is out of date with the settings source ({detail}). Regenerate \
         it and commit the result:\n  {REGENERATE}\nIn PowerShell: \
         $env:{UPDATE_VAR} = '1'; cargo test -p stem-core --test settings_reference; \
         Remove-Item Env:{UPDATE_VAR}"
    );
}

#[test]
fn the_reference_is_deterministic() {
    let items = parse_items();
    assert_eq!(render(&generate(&items)), render(&generate(&items)));
}

/// `{"a": {"b": value}}` for the path `a.b`.
fn document(keys: &[&str], value: Value) -> Value {
    keys.iter().rev().fold(value, |inner, key| {
        Value::Object(Map::from_iter([((*key).to_owned(), inner)]))
    })
}

fn lookup<'v>(value: &'v Value, keys: &[&str]) -> &'v Value {
    keys.iter().fold(value, |v, key| &v[*key])
}

/// Loads `doc` the way the app loads `settings.json` and returns its JSON form.
fn load(doc: &Value) -> Result<Value, String> {
    Settings::from_json(&doc.to_string())
        .map(|settings| serde_json::to_value(&settings).expect("serialise settings"))
        .map_err(|e| e.to_string())
}

#[test]
fn documented_ranges_are_what_validation_clamps_to() {
    let reference = generate(&parse_items());
    for claim in &reference.ranges {
        let keys = claim
            .path
            .keys()
            .expect("ranges are checked outside arrays");
        let lo: f64 = claim.lo.parse().expect("range start");
        let hi: f64 = claim.hi.parse().expect("range end");
        let name = claim.path.display();
        let mut probes = vec![(lo, lo), (hi, hi), (hi + 1.0, hi), (hi + 1_000.0, hi)];
        if claim.integer && lo < 1.0 {
            // Below 0 an integer key is not clamped: the page's introduction
            // calls a negative integer unreadable.
            let written = lo as i64 - 1;
            assert!(
                load(&document(&keys, Value::from(written))).is_err(),
                "{name}: the page says a negative integer makes the file \
                 unreadable, but {written} loads"
            );
        } else {
            probes.push((lo - 1.0, lo));
        }
        for (written, expected) in probes {
            let number = if claim.integer {
                Value::from(written as i64)
            } else {
                Value::from(written)
            };
            let loaded = load(&document(&keys, number))
                .unwrap_or_else(|e| panic!("{name} = {written} does not load: {e}"));
            let got = lookup(&loaded, &keys)
                .as_f64()
                .unwrap_or_else(|| panic!("{name} is not a number after loading"));
            assert!(
                (got - expected).abs() < 1e-6,
                "{name}: documented as {} to {}, but {written} loads as {got}",
                claim.lo,
                claim.hi
            );
        }
        let default = lookup(
            &serde_json::to_value(Settings::default()).expect("json"),
            &keys,
        )
        .as_f64()
        .expect("numeric default");
        assert!(
            (lo..=hi).contains(&default),
            "{name}: default {default} outside its range"
        );
    }
}

/// The values the page's introduction calls unreadable, for a key whose
/// default is `default`; `listed` when the page lists the key's values.
fn unreadable_values(default: &Value, listed: bool) -> Vec<Value> {
    let mut values = vec![match default {
        // `null` is a key of an optional type, which takes more than one.
        Value::Null => return Vec::new(),
        Value::Bool(_) => Value::from("true"),
        Value::Number(_) => Value::from("48"),
        Value::String(_) => Value::from(48),
        Value::Array(_) => Value::Object(Map::new()),
        Value::Object(_) => Value::Null,
    }];
    if let Some(integer) = default.as_u64() {
        values.extend([
            Value::from(-1),
            Value::from(u64::from(u32::MAX) + 1),
            Value::from(integer as f64),
            Value::from(integer as f64 + 0.5),
        ]);
    }
    if listed {
        values.push(Value::from("notAListedValue"));
    }
    values
}

#[test]
fn values_the_page_calls_unreadable_do_not_load() {
    let reference = generate(&parse_items());
    let defaults = serde_json::to_value(Settings::default()).expect("json");
    let mut paths = Vec::new();
    collect_paths(&defaults, &mut Vec::new(), &mut paths);
    let mut checked = 0;
    for path in paths {
        let keys: Vec<&str> = path.iter().map(String::as_str).collect();
        let listed = reference
            .values
            .iter()
            .any(|claim| claim.path.keys().as_deref() == Some(keys.as_slice()));
        for value in unreadable_values(lookup(&defaults, &keys), listed) {
            let doc = document(&keys, value);
            assert!(
                load(&doc).is_err(),
                "{}: the page calls this value unreadable, but it loads: {doc}",
                path.join(".")
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "no values were checked");
}

#[test]
fn documented_values_are_accepted_and_kept() {
    let reference = generate(&parse_items());
    for claim in reference.values.iter().filter(|c| c.path.keys().is_some()) {
        let keys = claim.path.keys().expect("filtered");
        for value in &claim.values {
            let value: Value = serde_json::from_str(value).expect("a JSON string");
            let loaded = load(&document(&keys, value.clone())).unwrap_or_else(|e| {
                panic!("{} = {value} does not load: {e}", claim.path.display())
            });
            assert_eq!(lookup(&loaded, &keys), &value, "{}", claim.path.display());
        }
    }
}

/// A value of `ty` with only the keys the reference calls required; for an
/// internally tagged enum, of its variant number `variant`.
fn minimal(items: &[Item], ty: &str, variant: usize) -> Value {
    if generic(ty, "Option").is_some() {
        return Value::Null;
    }
    if generic(ty, "Vec").is_some() {
        return Value::Array(Vec::new());
    }
    match ty {
        "bool" => return Value::Bool(false),
        "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize" => {
            return Value::from(1);
        }
        "f32" | "f64" => return Value::from(1.0),
        "String" | "PathBuf" => return Value::from("C:\\x"),
        _ => {}
    }
    let item = find(items, ty, "minimal");
    let required = |fields: &[Field], camel: bool| -> Map<String, Value> {
        fields
            .iter()
            .filter(|f| element_default(f, &f.name) == "required")
            .map(|f| (field_key(f, camel), minimal(items, &f.ty, 0)))
            .collect()
    };
    match &item.shape {
        Shape::Newtype(inner) => minimal(items, inner, variant),
        Shape::Struct(fields) => Value::Object(required(fields, camel_case(&item.serde, ty))),
        Shape::Enum(variants) => {
            let camel = camel_case(&item.serde, ty);
            let chosen = &variants[variant];
            match serde_value(&item.serde, "tag") {
                None => Value::from(variant_value(chosen, camel)),
                Some(tag) => {
                    let mut object = match &chosen.shape {
                        VariantShape::Struct(fields) => {
                            required(fields, camel_case(&chosen.serde, ty))
                        }
                        _ => Map::new(),
                    };
                    object.insert(tag.to_owned(), Value::from(variant_value(chosen, camel)));
                    Value::Object(object)
                }
            }
        }
    }
}

#[test]
fn required_keys_of_array_entries_are_required_and_the_rest_optional() {
    let items = parse_items();
    let root = find(&items, ROOT, ROOT);
    let Shape::Struct(root_fields) = &root.shape else {
        panic!("{ROOT} is not a struct");
    };
    let root_camel = camel_case(&root.serde, ROOT);
    let mut checked = 0;
    for array in root_fields {
        let Some(element) = generic(&array.ty, "Vec") else {
            continue;
        };
        let element_item = find(&items, element, element);
        let Shape::Struct(fields) = &element_item.shape else {
            continue;
        };
        let array_key = field_key(array, root_camel);
        let element_camel = camel_case(&element_item.serde, element);
        // The minimal entry, plus one per variant of every internally tagged
        // enum the entry holds.
        let tagged: Vec<(&Field, usize)> = fields
            .iter()
            .filter_map(|f| {
                let item = items.iter().find(|item| item.name == f.ty)?;
                match &item.shape {
                    Shape::Enum(variants) if serde_value(&item.serde, "tag").is_some() => {
                        Some((f, variants.len()))
                    }
                    _ => None,
                }
            })
            .collect();
        let mut entries = vec![minimal(&items, element, 0)];
        for (field, count) in &tagged {
            let key = field_key(field, element_camel);
            for variant in 0..*count {
                let mut entry = minimal(&items, element, 0);
                entry[&key] = minimal(&items, &field.ty, variant);
                entries.push(entry);
            }
        }
        for entry in entries {
            let doc = document(&[&array_key], Value::Array(vec![entry.clone()]));
            let loaded = load(&doc).unwrap_or_else(|e| {
                panic!(
                    "{array_key}: an entry with only its required keys does not load: {e}\n{doc}"
                )
            });
            assert_eq!(
                loaded[&array_key].as_array().map(Vec::len),
                Some(1),
                "{array_key}: the minimal entry was dropped: {doc}"
            );
            // Dropping any required key, at any depth, must fail the load.
            let mut paths = Vec::new();
            collect_paths(&entry, &mut Vec::new(), &mut paths);
            for path in paths {
                let mut broken = entry.clone();
                remove(&mut broken, &path);
                let doc = document(&[&array_key], Value::Array(vec![broken]));
                assert!(
                    load(&doc).is_err(),
                    "{array_key}[].{}: documented as required, but an entry without it loads",
                    path.join(".")
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 1, "no array entries were checked");
}

/// The key paths of every object member in `value`.
fn collect_paths(value: &Value, prefix: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
    if let Value::Object(map) = value {
        for (key, inner) in map {
            prefix.push(key.clone());
            out.push(prefix.clone());
            collect_paths(inner, prefix, out);
            prefix.pop();
        }
    }
}

fn remove(value: &mut Value, path: &[String]) {
    if let [init @ .., last] = path {
        let parent = init.iter().fold(value, |v, key| &mut v[key.as_str()]);
        if let Value::Object(map) = parent {
            map.remove(last);
        }
    }
}

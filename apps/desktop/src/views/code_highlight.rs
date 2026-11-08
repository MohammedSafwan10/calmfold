//! Syntax colors for fenced code blocks in notes.
//!
//! syntect + two-face (bat's ~200 grammars), the same engine and versions
//! gpui-kit's `syntect` feature uses. TextMate scopes are mapped onto the
//! highlight names gpui-kit themes define (`keyword`, `string`, …), so code
//! colors follow the app theme like every other color.
//!
//! The editor asks for every code block on every frame, so results are cached
//! by (language, code). Only the names are cached; styles are resolved per call
//! from the current theme, so switching themes recolors without re-parsing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};

use gpui_kit::HighlightStyle;
use gpui_kit::base::input::HighlightStyleResolver;
use gpui_kit::component::highlighter::HighlightTheme;
use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Deserialized once (tens of ms), on the first code block.
static SYNTAXES: LazyLock<SyntaxSet> = LazyLock::new(two_face::syntax::extra_newlines);

/// Blocks longer than this aren't colored (a pasted log shouldn't stall a frame).
const MAX_BLOCK_BYTES: usize = 64 * 1024;
/// Cached blocks before the cache starts over.
const MAX_CACHED: usize = 512;

type Names = Rc<Vec<(Range<usize>, &'static str)>>;

/// Shared by the editor's highlighter callback; the theme is swapped on change.
pub struct CodeHighlighter {
    theme: RefCell<Arc<HighlightTheme>>,
    cache: RefCell<HashMap<u64, Names>>,
}

impl CodeHighlighter {
    pub fn new(theme: Arc<HighlightTheme>) -> Rc<Self> {
        Rc::new(Self {
            theme: RefCell::new(theme),
            cache: RefCell::new(HashMap::new()),
        })
    }

    pub fn set_theme(&self, theme: Arc<HighlightTheme>) {
        *self.theme.borrow_mut() = theme;
    }

    /// Sorted, non-overlapping styled ranges for `code` in `language`.
    pub fn highlight(&self, language: &str, code: &str) -> Vec<(Range<usize>, HighlightStyle)> {
        let Some(names) = self.names(language, code) else {
            return Vec::new();
        };
        let theme = self.theme.borrow();
        names
            .iter()
            .filter_map(|(r, name)| theme.style(name).map(|s| (r.clone(), s)))
            .collect()
    }

    fn names(&self, language: &str, code: &str) -> Option<Names> {
        if code.len() > MAX_BLOCK_BYTES {
            return None;
        }
        let key = {
            use std::hash::{BuildHasher as _, Hash as _, Hasher as _};
            // One random seed per process: the cache never outlives it.
            let mut h = SEED.build_hasher();
            language.hash(&mut h);
            code.hash(&mut h);
            h.finish()
        };
        if let Some(hit) = self.cache.borrow().get(&key) {
            return Some(hit.clone());
        }
        let syntax = find_syntax(language)?;
        let names: Names = Rc::new(parse(syntax, code));
        let mut cache = self.cache.borrow_mut();
        if cache.len() >= MAX_CACHED {
            cache.clear();
        }
        cache.insert(key, names.clone());
        Some(names)
    }
}

static SEED: LazyLock<std::collections::hash_map::RandomState> =
    LazyLock::new(std::collections::hash_map::RandomState::new);

/// Languages offered in the code block's language picker.
pub const COMMON_LANGUAGES: &[&str] = &[
    "bash",
    "c",
    "cpp",
    "csharp",
    "css",
    "diff",
    "go",
    "html",
    "java",
    "javascript",
    "json",
    "kotlin",
    "lua",
    "markdown",
    "php",
    "python",
    "ruby",
    "rust",
    "sql",
    "swift",
    "toml",
    "typescript",
    "yaml",
];

fn find_syntax(language: &str) -> Option<&'static SyntaxReference> {
    let token = match language.trim().to_ascii_lowercase().as_str() {
        "" | "text" | "plain" | "txt" => return None,
        "sh" | "shell" | "zsh" => "bash".to_owned(),
        "c++" => "cpp".to_owned(),
        "c#" | "csharp" => "cs".to_owned(),
        "ts" => "typescript".to_owned(),
        "js" => "javascript".to_owned(),
        "py" => "python".to_owned(),
        "rs" => "rust".to_owned(),
        "yml" => "yaml".to_owned(),
        other => other.to_owned(),
    };
    SYNTAXES
        .find_syntax_by_token(&token)
        .or_else(|| SYNTAXES.find_syntax_by_token(language))
        .filter(|s| s.name != "Plain Text")
}

fn parse(syntax: &SyntaxReference, text: &str) -> Vec<(Range<usize>, &'static str)> {
    let mut parser = ParseState::new(syntax);
    let mut scopes = ScopeStack::new();
    let mut memo = HashMap::<Scope, Option<&'static str>>::new();
    let mut out: Vec<(Range<usize>, &'static str)> = Vec::new();
    let mut push = |range: Range<usize>,
                    scopes: &ScopeStack,
                    out: &mut Vec<(Range<usize>, &'static str)>| {
        if range.is_empty() {
            return;
        }
        // The innermost scope that names something wins.
        let name = scopes
            .scopes
            .iter()
            .rev()
            .find_map(|s| *memo.entry(*s).or_insert_with(|| semantic_name(*s)));
        if let Some(name) = name {
            match out.last_mut() {
                Some((last, n)) if last.end == range.start && *n == name => last.end = range.end,
                _ => out.push((range, name)),
            }
        }
    };
    let mut offset = 0;
    for line in LinesWithEndings::from(text) {
        // A line syntect can't parse keeps its starting scopes for the next.
        if let Ok(ops) = parser.parse_line(line, &SYNTAXES) {
            let mut cursor = 0;
            for (index, op) in ops {
                push(offset + cursor..offset + index, &scopes, &mut out);
                _ = scopes.apply(&op);
                cursor = index;
            }
            push(offset + cursor..offset + line.len(), &scopes, &mut out);
        }
        offset += line.len();
    }
    out
}

/// The theme highlight name for a TextMate scope, or `None` to let an outer scope decide.
fn semantic_name(scope: Scope) -> Option<&'static str> {
    let scope = scope.build_string();
    let is = |p: &str| scope == p || scope.strip_prefix(p).is_some_and(|r| r.starts_with('.'));
    Some(
        if is("punctuation.definition.string")
            || is("punctuation.definition.comment")
            || is("meta")
            || is("source")
            || is("text")
            || is("invalid")
        {
            return None;
        } else if is("comment") {
            "comment"
        } else if is("constant.character.escape") {
            "string.escape"
        } else if is("string.regexp") {
            "string.regex"
        } else if is("string") {
            "string"
        } else if is("constant.numeric") {
            "number"
        } else if is("constant.language.boolean") {
            "boolean"
        } else if is("constant") || is("support.constant") {
            "constant"
        } else if is("keyword.operator") {
            "operator"
        } else if is("keyword") || is("storage") {
            "keyword"
        } else if is("entity.name.function") || is("support.function") || is("variable.function") {
            "function"
        } else if is("entity.name") || is("support.type") || is("support.class") {
            "type"
        } else if is("entity.other.attribute-name") {
            "attribute"
        } else if is("variable.language") {
            "variable.special"
        } else if is("variable.other.member") || is("meta.mapping.key") {
            "property"
        } else if is("variable") {
            "variable"
        } else if is("punctuation") {
            "punctuation"
        } else {
            return None;
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(lang: &str, code: &str) -> Vec<(&'static str, String)> {
        let h = CodeHighlighter::new(HighlightTheme::default_dark());
        h.names(lang, code)
            .map(|n| {
                n.iter()
                    .map(|(r, n)| (*n, code[r.clone()].to_owned()))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn colors_common_languages_and_aliases() {
        let rust = names("rust", "fn main() { let x = 42; }\n");
        assert!(rust.contains(&("keyword", "fn".into())), "{rust:?}");
        assert!(rust.contains(&("number", "42".into())), "{rust:?}");
        assert!(
            !names("py", "def f():\n    return 'x'\n").is_empty(),
            "alias py"
        );
        assert!(
            !names("ts", "const a: number = 1;\n").is_empty(),
            "alias ts"
        );
        assert!(
            names("text", "fn main() {}").is_empty(),
            "plain text stays plain"
        );
        assert!(names("nonsense-lang", "x").is_empty());
    }

    #[test]
    fn styles_come_from_the_theme_and_huge_blocks_are_skipped() {
        let h = CodeHighlighter::new(HighlightTheme::default_dark());
        let styled = h.highlight("rust", "let s = \"hi\";\n");
        assert!(styled.iter().all(|(_, s)| s.color.is_some()));
        assert!(!styled.is_empty());
        let huge = "x\n".repeat(MAX_BLOCK_BYTES);
        assert!(h.highlight("rust", &huge).is_empty());
    }
}

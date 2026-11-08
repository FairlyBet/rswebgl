//! `#include` for GLSL ES (which has none): shader code assembled from named
//! chunks, with compile errors pointing at the chunk and line they come from.
//!
//! ```text
//! #include "color.glsl"     // or <color.glsl>
//! ```
//!
//! Each chunk is pasted once per shader (a second `#include` of the same name
//! is dropped, so chunks needn't guard themselves), chunks may include
//! others, and every file gets its own GLSL source-string number through
//! `#line <line> <source>`: a compile log's `ERROR: 3:12:` names file 3, line
//! 12, which [`ShaderSource::describe_log`] (and the program's error report)
//! turns into `ERROR: color.glsl:12:`.

use crate::error::{Error, err};

/// Assembled shader code plus the names of its source strings: index 0 is
/// the main file, then each chunk in the order first included.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ShaderSource {
    pub code: String,
    pub files: Vec<String>,
}

impl ShaderSource {
    /// Plain code with no chunks (one source string, `name`).
    pub fn plain(name: &str, code: &str) -> Self {
        Self {
            code: code.to_string(),
            files: vec![name.to_string()],
        }
    }

    /// Expands `#include` lines in `code` (named `name` in error logs),
    /// looking chunks up with `chunk(name)`. `prefix` (`#version`, defines)
    /// goes first, unnumbered, then the `preludes` chunks (as if included
    /// at the top); `code` starts at line 1 of source 0.
    pub fn assemble<'a>(
        prefix: &str,
        preludes: &[&str],
        name: &str,
        code: &str,
        chunk: impl Fn(&str) -> Option<&'a str>,
    ) -> Result<Self, Error> {
        let mut out = Self {
            code: String::with_capacity(prefix.len() + code.len() * 2),
            files: vec![name.to_string()],
        };
        out.code.push_str(prefix);
        if !prefix.is_empty() && !prefix.ends_with('\n') {
            out.code.push('\n');
        }
        if !preludes.is_empty() {
            let top: String = preludes
                .iter()
                .map(|p| format!("#include \"{p}\"\n"))
                .collect();
            out.expand(&top, 0, &chunk, 0)?;
        }
        out.code.push_str("#line 1 0\n");
        out.expand(code, 0, &chunk, 0)?;
        Ok(out)
    }

    fn expand<'a>(
        &mut self,
        code: &str,
        id: usize,
        chunk: &impl Fn(&str) -> Option<&'a str>,
        depth: usize,
    ) -> Result<(), Error> {
        for (i, line) in code.lines().enumerate() {
            let Some(name) = include_target(line) else {
                self.code.push_str(line);
                self.code.push('\n');
                continue;
            };
            let at = || format!("{}:{}", self.files[id], i + 1);
            let name = name.map_err(|e| err!("{}: {e}", at()))?;
            if !self.files.iter().any(|f| f == name) {
                if depth >= 32 {
                    return Err(err!("{}: #include nested too deeply", at()));
                }
                let text =
                    chunk(name).ok_or_else(|| err!("{}: no shader chunk \"{name}\"", at()))?;
                let child = self.files.len();
                self.files.push(name.to_string());
                self.code.push_str(&format!("#line 1 {child}\n"));
                self.expand(text, child, chunk, depth + 1)?;
            }
            // Back in this file: the next line is i + 2 (1-based).
            self.code.push_str(&format!("#line {} {id}\n", i + 2));
        }
        Ok(())
    }

    /// A compile log with source numbers replaced by file names
    /// (`ERROR: 2:14:` → `ERROR: brdf.glsl:14:`).
    pub fn describe_log(&self, log: &str) -> String {
        describe_log(log, &self.files)
    }
}

// The target of an `#include` line: None if the line isn't one, Err if it is
// but malformed.
fn include_target(line: &str) -> Option<Result<&str, &'static str>> {
    let rest = line.trim_start().strip_prefix('#')?.trim_start();
    let rest = rest.strip_prefix("include")?.trim();
    let (open, close) = match rest.chars().next() {
        Some('"') => ('"', '"'),
        Some('<') => ('<', '>'),
        _ => return Some(Err("#include needs \"name\" or <name>")),
    };
    let inner = &rest[open.len_utf8()..];
    Some(match inner.find(close) {
        Some(end) if end > 0 => Ok(&inner[..end]),
        _ => Err("#include: unterminated name"),
    })
}

/// `ShaderSource::describe_log` for a bare file table.
pub fn describe_log(log: &str, files: &[String]) -> String {
    if files.len() < 2 {
        return log.to_string();
    }
    let mut out = String::with_capacity(log.len());
    for line in log.lines() {
        let mut rewritten = None;
        for tag in ["ERROR: ", "WARNING: "] {
            if let Some(rest) = line.strip_prefix(tag)
                && let Some((num, tail)) = rest.split_once(':')
                && let Ok(n) = num.trim().parse::<usize>()
                && let Some(name) = files.get(n)
            {
                rewritten = Some(format!("{tag}{name}:{tail}"));
            }
        }
        out.push_str(rewritten.as_deref().unwrap_or(line));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(name: &str) -> Option<&'static str> {
        match name {
            "a.glsl" => Some("float a() { return 1.0; }"),
            "b.glsl" => Some("#include \"a.glsl\"\nfloat b() { return a(); }"),
            "loop.glsl" => Some("#include \"loop.glsl\"\nint x;"),
            _ => None,
        }
    }

    #[test]
    fn includes_once_with_line_numbers() {
        let s = ShaderSource::assemble(
            "#version 300 es\n",
            &[],
            "main",
            "#include \"b.glsl\"\n#include <a.glsl>\nvoid main() {}",
            chunks,
        )
        .unwrap();
        assert_eq!(s.files, ["main", "b.glsl", "a.glsl"]);
        // A prelude comes before line 1 of the main file.
        let p = ShaderSource::assemble("", &["a.glsl"], "main", "x", chunks).unwrap();
        assert_eq!(
            p.code,
            "#line 1 1\nfloat a() { return 1.0; }\n#line 2 0\n#line 1 0\nx\n"
        );
        assert_eq!(
            s.code,
            "#version 300 es\n#line 1 0\n\
             #line 1 1\n\
             #line 1 2\nfloat a() { return 1.0; }\n#line 2 1\n\
             float b() { return a(); }\n\
             #line 2 0\n\
             #line 3 0\n\
             void main() {}\n"
        );
    }

    #[test]
    fn errors_name_the_file_and_line() {
        let e = ShaderSource::assemble("", &[], "main", "x\n#include \"nope.glsl\"", chunks)
            .unwrap_err();
        assert!(
            e.to_string()
                .contains("main:2: no shader chunk \"nope.glsl\""),
            "{e}"
        );
        let e = ShaderSource::assemble("", &[], "main", "#include nope", chunks).unwrap_err();
        assert!(e.to_string().contains("main:1:"), "{e}");
        // A chunk including itself is included once, not forever.
        assert!(ShaderSource::assemble("", &[], "main", "#include \"loop.glsl\"", chunks).is_ok());
    }

    #[test]
    fn logs_get_file_names() {
        let files = vec!["main".to_string(), "brdf.glsl".to_string()];
        assert_eq!(
            describe_log(
                "ERROR: 1:14: 'x' : undeclared\nERROR: 0:3: y\nother",
                &files
            ),
            "ERROR: brdf.glsl:14: 'x' : undeclared\nERROR: main:3: y\nother\n"
        );
    }
}

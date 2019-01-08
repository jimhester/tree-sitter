use super::error::Result;
use ansi_term::Colour;
use regex::bytes::{Regex as ByteRegex, RegexBuilder as ByteRegexBuilder};
use regex::Regex;
use std::char;
use std::fs;
use std::io;
use std::path::Path;
use std::str;
use tree_sitter::{Language, Parser};

lazy_static! {
    static ref HEADER_REGEX: ByteRegex = ByteRegexBuilder::new(r"^===+\r?\n([^=]*)\r?\n===+\r?\n")
        .multi_line(true)
        .build()
        .unwrap();
    static ref DIVIDER_REGEX: ByteRegex = ByteRegexBuilder::new(r"^---+\r?\n")
        .multi_line(true)
        .build()
        .unwrap();
    static ref WHITESPACE_REGEX: Regex = Regex::new(r"\s+").unwrap();
}

#[derive(Debug, PartialEq, Eq)]
pub enum TestEntry {
    Group {
        name: String,
        children: Vec<TestEntry>,
    },
    Example {
        name: String,
        input: Vec<u8>,
        output: String,
    },
}

pub fn run_tests_at_path(language: Language, path: &Path) -> Result<()> {
    let test_entry = parse_tests(path)?;
    let mut parser = Parser::new();
    parser.set_language(language)?;

    let mut failures = Vec::new();
    if let TestEntry::Group { children, .. } = test_entry {
        for child in children {
            run_tests(&mut parser, child, 0, &mut failures)?;
        }
    }

    if failures.len() > 0 {
        println!("");

        if failures.len() == 1 {
            println!("1 failure:")
        } else {
            println!("{} failures:", failures.len())
        }

        for (name, actual, expected) in failures {
            println!("\n  {}:", name);
            println!("    Expected: {}", expected);
            println!("    Actual: {}", actual);
        }
    }

    Ok(())
}

fn run_tests(
    parser: &mut Parser,
    test_entry: TestEntry,
    mut indent_level: i32,
    failures: &mut Vec<(String, String, String)>,
) -> Result<()> {
    for _ in 0..indent_level {
        print!("  ");
    }
    match test_entry {
        TestEntry::Example {
            name,
            input,
            output,
        } => {
            let tree = parser
                .parse_utf8(&mut |byte_offset, _| &input[byte_offset..], None)
                .unwrap();
            let actual = tree.root_node().to_sexp();
            if actual == output {
                println!("✓ {}", Colour::Green.paint(&name));
            } else {
                println!("✗ {}", Colour::Red.paint(&name));
                failures.push((name, actual, output));
            }
        }
        TestEntry::Group { name, children } => {
            println!("{}:", name);
            indent_level += 1;
            for child in children {
                run_tests(parser, child, indent_level, failures)?;
            }
        }
    }
    Ok(())
}

pub fn parse_tests(path: &Path) -> io::Result<TestEntry> {
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    if path.is_dir() {
        let mut children = Vec::new();
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            children.push(parse_tests(&entry.path())?);
        }
        Ok(TestEntry::Group { name, children })
    } else {
        let content = fs::read_to_string(path)?;
        Ok(parse_test_content(name, content))
    }
}

fn parse_test_content(name: String, content: String) -> TestEntry {
    let mut children = Vec::new();
    let bytes = content.as_bytes();
    let mut previous_name = String::new();
    let mut previous_header_end = 0;
    for header_match in HEADER_REGEX
        .find_iter(&bytes)
        .map(|m| (m.start(), m.end()))
        .chain(Some((bytes.len(), bytes.len())))
    {
        let (header_start, header_end) = header_match;
        if previous_header_end > 0 {
            if let Some(divider_match) =
                DIVIDER_REGEX.find(&bytes[previous_header_end..header_start])
            {
                let (divider_start, divider_end) = (
                    previous_header_end + divider_match.start(),
                    previous_header_end + divider_match.end(),
                );
                if let Ok(output) = str::from_utf8(&bytes[divider_end..header_start]) {
                    let input = bytes[previous_header_end..divider_start].to_vec();
                    let output = WHITESPACE_REGEX.replace_all(output.trim(), " ").to_string();
                    children.push(TestEntry::Example {
                        name: previous_name,
                        input,
                        output,
                    });
                }
            }
        }
        previous_name = String::from_utf8_lossy(&bytes[header_start..header_end])
            .trim_matches(|c| char::is_whitespace(c) || c == '=')
            .to_string();
        previous_header_end = header_end;
    }
    TestEntry::Group { name, children }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_test_content() {
        let entry = parse_test_content(
            "the-filename".to_string(),
            r#"
===============
The first test
===============

a b c

---

(a
    (b c))

================
The second test
================
d
---
(d)
        "#
            .trim()
            .to_string(),
        );

        assert_eq!(
            entry,
            TestEntry::Group {
                name: "the-filename".to_string(),
                children: vec![
                    TestEntry::Example {
                        name: "The first test".to_string(),
                        input: "\na b c\n\n".as_bytes().to_vec(),
                        output: "(a (b c))".to_string(),
                    },
                    TestEntry::Example {
                        name: "The second test".to_string(),
                        input: "d\n".as_bytes().to_vec(),
                        output: "(d)".to_string(),
                    },
                ]
            }
        );
    }
}

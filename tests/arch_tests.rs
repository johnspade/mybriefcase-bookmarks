//! Architectural fitness test for `mybriefcase-bookmarks-core`.
//!
//! Enforces the module dependency graph by reading source files, extracting
//! non-test `use crate::` statements, and asserting against a declared
//! allowed-dependencies map. The map below IS the architecture reference.

use std::collections::HashMap;
use std::path::Path;

/// Declared allowed intra-crate dependencies for each core module.
/// A module listed with an empty slice is a leaf (no intra-crate imports allowed).
fn allowed_deps() -> HashMap<&'static str, Vec<&'static str>> {
    HashMap::from([
        ("model", vec![]),
        ("schema", vec![]),
        ("ops", vec!["schema", "import"]),
        ("repo", vec![]),
        ("history", vec![]),
        ("export", vec!["model"]),
        ("import", vec![]),
        ("identity", vec![]),
        ("watcher", vec![]),
        ("lib", vec![]),
    ])
}

/// Extracts non-test `use crate::` imports from a source file.
/// Skips any imports inside `#[cfg(test)]` modules.
fn extract_crate_imports(source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut in_cfg_test = false;
    let mut brace_depth: i32 = 0;
    let mut cfg_test_start_depth: i32 = 0;

    for line in source.lines() {
        let trimmed = line.trim();

        // Detect #[cfg(test)] on the next item
        if trimmed == "#[cfg(test)]" {
            in_cfg_test = true;
            cfg_test_start_depth = brace_depth;
            continue;
        }

        // Track brace depth
        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => {
                    brace_depth -= 1;
                    if in_cfg_test && brace_depth <= cfg_test_start_depth {
                        in_cfg_test = false;
                    }
                }
                _ => {}
            }
        }

        if in_cfg_test {
            continue;
        }

        // Extract `use crate::X` or inline `crate::X::Y`
        if trimmed.starts_with("use crate::") {
            if let Some(module) = extract_module_from_use(trimmed) {
                imports.push(module);
            }
        } else if !trimmed.starts_with("//") && !trimmed.starts_with("use ") {
            // Check for inline `crate::module` references
            for module in extract_inline_crate_refs(trimmed) {
                imports.push(module);
            }
        }
    }

    imports.sort();
    imports.dedup();
    imports
}

fn extract_module_from_use(line: &str) -> Option<String> {
    // "use crate::schema;" or "use crate::schema::CHILDREN;"
    let after_crate = line.strip_prefix("use crate::")?;
    let module = after_crate.split([':', ';', '{']).next()?;
    let module = module.trim();
    if module.is_empty() {
        return None;
    }
    Some(module.to_string())
}

fn extract_inline_crate_refs(line: &str) -> Vec<String> {
    let mut modules = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = line[search_from..].find("crate::") {
        let abs_pos = search_from + pos;
        let after = &line[abs_pos + 7..];
        if let Some(module) = after.split([':', ' ', ')', ',']).next() {
            let module = module.trim();
            if !module.is_empty() {
                modules.push(module.to_string());
            }
        }
        search_from = abs_pos + 7;
    }
    modules
}

#[test]
#[cfg_attr(miri, ignore)]
fn core_module_dependency_graph() {
    let core_src = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates/core/src");
    let allowed = allowed_deps();
    let mut violations = Vec::new();

    for (module, allowed_imports) in &allowed {
        let filename = format!("{module}.rs");
        let filepath = core_src.join(&filename);
        let source = std::fs::read_to_string(&filepath)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", filepath.display()));

        let actual_imports = extract_crate_imports(&source);

        for import in &actual_imports {
            if !allowed_imports.contains(&import.as_str()) {
                violations.push(format!(
                    "{filename}: imports `crate::{import}` which is not in its allowed dependencies {allowed_imports:?}"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Architecture violations detected:\n{}",
        violations.join("\n")
    );
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn extracts_use_crate_import() {
        let source = "use crate::schema;\nuse crate::model::Bookmark;";
        let imports = extract_crate_imports(source);
        assert_eq!(imports, vec!["model", "schema"]);
    }

    #[test]
    fn skips_cfg_test_imports() {
        let source = r"
use crate::schema;

#[cfg(test)]
mod tests {
    use crate::model::BookmarkStore;
}
";
        let imports = extract_crate_imports(source);
        assert_eq!(imports, vec!["schema"]);
    }

    #[test]
    fn extracts_inline_crate_refs() {
        let source = r"
fn foo(items: &[crate::import::ImportedItem]) {}
";
        let imports = extract_crate_imports(source);
        assert_eq!(imports, vec!["import"]);
    }

    #[test]
    fn ignores_comments() {
        let source = "// use crate::model;\nuse crate::schema;";
        let imports = extract_crate_imports(source);
        assert_eq!(imports, vec!["schema"]);
    }
}

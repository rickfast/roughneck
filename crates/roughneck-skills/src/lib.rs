use roughneck_core::{Result, RoughneckError, SkillsConfig};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub system_instructions: String,
}

#[derive(Debug, Deserialize)]
struct TomlSkillFile {
    name: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    system_instructions: String,
}

#[derive(Debug, Deserialize)]
struct MarkdownSkillFrontmatter {
    name: String,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Default)]
pub struct SkillsRegistry {
    skills: HashMap<String, SkillDefinition>,
}

impl SkillsRegistry {
    /// Loads skills from the configured registry paths.
    ///
    /// # Errors
    ///
    /// Returns an error if a discovered skill file is invalid or if duplicate skill names are found.
    pub fn load(paths: &[PathBuf]) -> Result<Self> {
        let mut skills = HashMap::new();

        for root in paths {
            if !root.exists() {
                continue;
            }
            for entry in walkdir::WalkDir::new(root)
                .follow_links(false)
                .into_iter()
                .filter_map(std::result::Result::ok)
                .filter(|entry| entry.file_type().is_file())
            {
                if !is_skill_file(entry.path()) {
                    continue;
                }

                let skill = load_skill_file(entry.path())?;
                if skills.contains_key(&skill.name) {
                    return Err(RoughneckError::InvalidInput(format!(
                        "duplicate skill '{}': {}",
                        skill.name,
                        entry.path().display()
                    )));
                }
                skills.insert(skill.name.clone(), skill);
            }
        }

        Ok(Self { skills })
    }

    /// Resolves the enabled skill set by name.
    ///
    /// # Errors
    ///
    /// Returns an error if any requested skill name is not present in the registry.
    pub fn enabled(&self, names: &[String]) -> Result<Vec<SkillDefinition>> {
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let skill = self
                .skills
                .get(name)
                .ok_or_else(|| RoughneckError::NotFound(format!("unknown skill: {name}")))?;
            out.push(skill.clone());
        }
        Ok(out)
    }

    #[must_use]
    /// Builds the prompt section for the enabled skills.
    pub fn prompt_section(skills: &[SkillDefinition]) -> String {
        if skills.is_empty() {
            return String::new();
        }

        let mut output = String::from("\n\n## Enabled Skills\n");
        for skill in skills {
            output.push_str("- ");
            output.push_str(&skill.name);
            output.push_str(": ");
            output.push_str(&skill.description);
            output.push('\n');
            if !skill.system_instructions.trim().is_empty() {
                output.push_str(skill.system_instructions.trim());
                output.push('\n');
            }
        }

        output
    }

    /// Loads the configured skills and resolves the enabled subset.
    ///
    /// # Errors
    ///
    /// Returns an error if skill files cannot be loaded or if any enabled skill is unknown.
    pub fn from_config(config: &SkillsConfig) -> Result<Vec<SkillDefinition>> {
        let registry = Self::load(&config.registry_paths)?;
        registry.enabled(&config.enabled_skills)
    }
}

fn load_skill_file(path: &Path) -> Result<SkillDefinition> {
    let raw = std::fs::read_to_string(path)?;

    if is_toml_skill_file(path) {
        let parsed: TomlSkillFile = toml::from_str(&raw).map_err(|err| {
            RoughneckError::InvalidInput(format!("invalid skill file {}: {err}", path.display()))
        })?;
        return build_skill_definition(
            parsed.name,
            parsed.description,
            &parsed.system_instructions,
            path,
        );
    }

    if is_markdown_skill_file(path) {
        let (frontmatter, body) = split_markdown_frontmatter(&raw, path)?;
        let parsed: MarkdownSkillFrontmatter =
            serde_yaml::from_str(&frontmatter).map_err(|err| {
                RoughneckError::InvalidInput(format!(
                    "invalid markdown skill frontmatter {}: {err}",
                    path.display()
                ))
            })?;
        return build_skill_definition(parsed.name, parsed.description, &body, path);
    }

    Err(RoughneckError::InvalidInput(format!(
        "unsupported skill file: {}",
        path.display()
    )))
}

fn build_skill_definition(
    name: String,
    description: String,
    system_instructions: &str,
    path: &Path,
) -> Result<SkillDefinition> {
    if name.trim().is_empty() {
        return Err(RoughneckError::InvalidInput(format!(
            "skill file {} is missing a name",
            path.display()
        )));
    }

    Ok(SkillDefinition {
        name,
        description,
        system_instructions: system_instructions.trim().to_string(),
    })
}

fn split_markdown_frontmatter(raw: &str, path: &Path) -> Result<(String, String)> {
    let normalized = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let mut lines = normalized.lines();
    let Some(first) = lines.next() else {
        return Err(RoughneckError::InvalidInput(format!(
            "markdown skill {} is empty",
            path.display()
        )));
    };

    if first.trim() != "---" {
        return Err(RoughneckError::InvalidInput(format!(
            "markdown skill {} must start with YAML frontmatter",
            path.display()
        )));
    }

    let mut frontmatter = Vec::new();
    let mut body = Vec::new();
    let mut in_frontmatter = true;

    for line in lines {
        if in_frontmatter && line.trim() == "---" {
            in_frontmatter = false;
            continue;
        }

        if in_frontmatter {
            frontmatter.push(line);
        } else {
            body.push(line);
        }
    }

    if in_frontmatter {
        return Err(RoughneckError::InvalidInput(format!(
            "markdown skill {} is missing a closing YAML frontmatter delimiter",
            path.display()
        )));
    }

    Ok((frontmatter.join("\n"), body.join("\n")))
}

fn is_skill_file(path: &Path) -> bool {
    is_toml_skill_file(path) || is_markdown_skill_file(path)
}

fn is_toml_skill_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".skill.toml"))
}

fn is_markdown_skill_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == "SKILL.md" || name.ends_with(".skill.md"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn make_temp_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("roughneck-skills-{name}-{unique}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn prompt_section_contains_skill_content() {
        let prompt = SkillsRegistry::prompt_section(&[SkillDefinition {
            name: "rust-best-practices".to_string(),
            description: "Rust guidance".to_string(),
            system_instructions: "Use Result instead of panic".to_string(),
        }]);

        assert!(prompt.contains("Enabled Skills"));
        assert!(prompt.contains("rust-best-practices"));
    }

    #[test]
    fn load_supports_skill_markdown() {
        let root = make_temp_dir("markdown");
        let skill_dir = root.join("rust-best-practices");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            r"---
name: rust-best-practices
description: Idiomatic Rust patterns
metadata:
  short-description: Rust guidance
---

# Rust Best Practices

- Prefer `Result` over `panic!`
- Borrow instead of cloning when possible
",
        )
        .unwrap();

        let registry = SkillsRegistry::load(std::slice::from_ref(&root)).unwrap();
        let enabled = registry
            .enabled(&["rust-best-practices".to_string()])
            .unwrap();

        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].name, "rust-best-practices");
        assert_eq!(enabled[0].description, "Idiomatic Rust patterns");
        assert!(
            enabled[0]
                .system_instructions
                .contains("# Rust Best Practices")
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn markdown_skill_requires_yaml_frontmatter() {
        let root = make_temp_dir("invalid-markdown");
        let skill_dir = root.join("broken");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(skill_dir.join("SKILL.md"), "# Missing frontmatter\n").unwrap();

        let err = SkillsRegistry::load(std::slice::from_ref(&root)).unwrap_err();
        assert!(err.to_string().contains("must start with YAML frontmatter"));

        fs::remove_dir_all(root).unwrap();
    }
}

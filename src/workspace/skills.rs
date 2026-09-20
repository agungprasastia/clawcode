use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillSource {
    Project,
    Global,
    Agents,
    OpenCode,
    Claude,
}

impl SkillSource {
    pub fn label(&self) -> &'static str {
        match self {
            SkillSource::Project => "project",
            SkillSource::Global => "global",
            SkillSource::Agents => "agents",
            SkillSource::OpenCode => "opencode",
            SkillSource::Claude => "claude",
        }
    }
}

impl std::fmt::Display for SkillSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillItem {
    pub name: String,
    pub description: String,
    pub location: PathBuf,
    pub instructions: String,
    pub source: SkillSource,
}

pub struct SkillStore {
    skills: HashMap<String, SkillItem>,
}

impl SkillStore {
    pub fn load(project_root: &Path) -> Self {
        Self::load_with_custom_globals(project_root, &global_skills_dirs())
    }

    pub fn load_with_custom_globals(
        project_root: &Path,
        global_dirs: &[(PathBuf, SkillSource)],
    ) -> Self {
        let mut skills = HashMap::new();

        for (global_dir, source) in global_dirs {
            scan_dir_for_skills(global_dir, source.clone(), &mut skills, 0);
        }

        let abs_root;
        let root = if project_root.is_relative() {
            if let Ok(cur) = std::env::current_dir() {
                abs_root = cur.join(project_root);
                &abs_root
            } else {
                project_root
            }
        } else {
            project_root
        };

        let mut chain = Vec::new();
        let mut curr = Some(root);
        while let Some(p) = curr {
            chain.push(p);
            curr = p.parent();
        }
        chain.reverse();

        for current in chain {
            scan_dir_for_skills(
                &current.join(".agents").join("skills"),
                SkillSource::Agents,
                &mut skills,
                0,
            );
            scan_dir_for_skills(
                &current.join(".agent").join("skills"),
                SkillSource::Agents,
                &mut skills,
                0,
            );
            scan_dir_for_skills(
                &current.join(".claude").join("skills"),
                SkillSource::Claude,
                &mut skills,
                0,
            );
            scan_dir_for_skills(
                &current.join(".clawcode").join("skills"),
                SkillSource::Project,
                &mut skills,
                0,
            );
            scan_dir_for_skills(
                &current.join(".opencode").join("skills"),
                SkillSource::OpenCode,
                &mut skills,
                0,
            );
            scan_dir_for_skills(
                &current.join("skills"),
                SkillSource::Project,
                &mut skills,
                0,
            );
        }

        Self { skills }
    }

    pub fn get(&self, name: &str) -> Option<&SkillItem> {
        self.skills.get(name)
    }

    pub fn all(&self) -> Vec<&SkillItem> {
        let mut items: Vec<&SkillItem> = self.skills.values().collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        items
    }

    pub fn is_empty(&self) -> bool {
        self.skills.is_empty()
    }

    pub fn len(&self) -> usize {
        self.skills.len()
    }
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

fn global_skills_dirs() -> Vec<(PathBuf, SkillSource)> {
    let mut dirs = Vec::new();
    let mut push_dir = |p: PathBuf, source: SkillSource| {
        if !dirs.iter().any(|(existing, _)| existing == &p) {
            dirs.push((p, source));
        }
    };

    if let Some(home) = home_dir() {
        push_dir(home.join(".agents").join("skills"), SkillSource::Agents);
        push_dir(home.join(".agent").join("skills"), SkillSource::Agents);
        push_dir(home.join(".claude").join("skills"), SkillSource::Claude);
        push_dir(home.join(".clawcode").join("skills"), SkillSource::Global);
        push_dir(home.join(".opencode").join("skills"), SkillSource::OpenCode);
        push_dir(
            home.join(".config").join("clawcode").join("skills"),
            SkillSource::Global,
        );
        push_dir(
            home.join(".config").join("opencode").join("skills"),
            SkillSource::OpenCode,
        );
    }
    if let Some(val) = std::env::var_os("XDG_CONFIG_HOME")
        && !val.is_empty()
    {
        let base = PathBuf::from(val);
        push_dir(base.join("clawcode").join("skills"), SkillSource::Global);
        push_dir(base.join("opencode").join("skills"), SkillSource::OpenCode);
        push_dir(base.join("agents").join("skills"), SkillSource::Agents);
    }
    if let Some(val) = std::env::var_os("APPDATA")
        && !val.is_empty()
    {
        let base = PathBuf::from(val);
        push_dir(base.join("clawcode").join("skills"), SkillSource::Global);
        push_dir(base.join("opencode").join("skills"), SkillSource::OpenCode);
        push_dir(base.join("agents").join("skills"), SkillSource::Agents);
    }
    dirs
}

fn scan_dir_for_skills(
    dir: &Path,
    source: SkillSource,
    skills: &mut HashMap<String, SkillItem>,
    depth: usize,
) {
    if depth > 8 || !dir.is_dir() {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };

        if path.is_dir() {
            if file_name.starts_with('.') || file_name == "target" || file_name == "node_modules" {
                continue;
            }
            let mut found_skill = false;
            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    let sub_path = sub_entry.path();
                    if sub_path.is_file()
                        && sub_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .map(|n| n.eq_ignore_ascii_case("skill.md"))
                            .unwrap_or(false)
                    {
                        if let Some(item) = parse_skill_file(&sub_path, source.clone(), file_name) {
                            skills.insert(item.name.clone(), item);
                        }
                        found_skill = true;
                        break;
                    }
                }
            }
            if !found_skill {
                scan_dir_for_skills(&path, source.clone(), skills, depth + 1);
            }
        } else if path.is_file() {
            if file_name.eq_ignore_ascii_case("skill.md") {
                let default_name = path
                    .parent()
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str())
                    .unwrap_or("skill");
                if let Some(item) = parse_skill_file(&path, source.clone(), default_name) {
                    skills.insert(item.name.clone(), item);
                }
            } else if path
                .extension()
                .map(|e| e.to_string_lossy().eq_ignore_ascii_case("md"))
                .unwrap_or(false)
            {
                let default_name = path.file_stem().and_then(|n| n.to_str()).unwrap_or("skill");
                if let Some(item) = parse_skill_file(&path, source.clone(), default_name) {
                    skills.insert(item.name.clone(), item);
                }
            }
        }
    }
}

fn parse_skill_file(path: &Path, source: SkillSource, default_name: &str) -> Option<SkillItem> {
    let content = std::fs::read_to_string(path).ok()?;
    let (frontmatter, body) = split_frontmatter(&content);

    let mut name = default_name.to_string();
    let mut description = String::new();

    if let Some(fm) = frontmatter {
        for line in fm.lines() {
            let trimmed = line.trim();
            if let Some(val) = strip_yaml_field(trimmed, "description") {
                description = val.to_string();
            } else if let Some(val) = strip_yaml_field(trimmed, "name") {
                name = val.to_string();
            }
        }
    }

    if description.is_empty() {
        description = extract_markdown_description(body);
    }

    let instructions = if frontmatter.is_some() {
        body.trim().to_string()
    } else {
        content.trim().to_string()
    };

    Some(SkillItem {
        name,
        description,
        location: path.to_path_buf(),
        instructions,
        source,
    })
}

fn split_frontmatter(content: &str) -> (Option<&str>, &str) {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return (None, content);
    }
    let after_dashes = &trimmed[3..];
    for (i, _) in after_dashes.match_indices("---") {
        let before = &after_dashes[..i];
        let after = &after_dashes[i + 3..];
        if before.ends_with('\n') || before.ends_with("\r\n") || before.is_empty() {
            let fm = before.trim();
            let body = after.trim_start_matches('\r').trim_start_matches('\n');
            return (Some(fm), body);
        }
    }
    (None, content)
}

fn strip_yaml_field<'a>(line: &'a str, field: &str) -> Option<&'a str> {
    let prefix = format!("{field}:");
    if line.to_ascii_lowercase().starts_with(&prefix) {
        let val = line[prefix.len()..].trim();
        let stripped = val
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .or_else(|| val.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
            .unwrap_or(val);
        Some(stripped)
    } else {
        None
    }
}

fn extract_markdown_description(markdown: &str) -> String {
    let mut lines = markdown.lines();
    let mut found_title = false;
    for line in lines.by_ref() {
        let trimmed = line.trim();
        if trimmed.starts_with("# ") {
            found_title = true;
            break;
        }
    }

    let mut paragraph = Vec::new();
    if found_title {
        for line in lines {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                if !paragraph.is_empty() {
                    break;
                }
            } else if trimmed.starts_with('#') {
                break;
            } else {
                paragraph.push(trimmed);
            }
        }
    } else {
        for line in markdown.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                if !paragraph.is_empty() {
                    break;
                }
            } else {
                paragraph.push(trimmed);
            }
        }
    }

    paragraph.join(" ")
}

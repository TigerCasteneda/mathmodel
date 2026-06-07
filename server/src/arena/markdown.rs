use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArenaFrontmatter {
    pub card_type: String,
    pub title: String,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArenaParsedCard {
    pub frontmatter: ArenaFrontmatter,
    pub body: String,
    pub links: Vec<String>,
}

pub fn parse_arena_markdown(content: &str, fallback_title: &str) -> ArenaParsedCard {
    let (frontmatter_text, body) = split_frontmatter(content);
    let frontmatter = parse_frontmatter(frontmatter_text, fallback_title);
    ArenaParsedCard {
        frontmatter,
        body: body.to_string(),
        links: parse_wiki_links(body),
    }
}

pub fn render_arena_markdown(card_type: &str, title: &str, tags: &[String], body: &str) -> String {
    let tags = tags.join(", ");
    format!(
        "---\ntype: {card_type}\ntitle: {title}\ntags: [{tags}]\naliases: []\nstatus: draft\n---\n\n{body}\n"
    )
}

fn split_frontmatter(content: &str) -> (&str, &str) {
    let trimmed = content
        .strip_prefix("---\n")
        .or_else(|| content.strip_prefix("---\r\n"));
    let Some(after_start) = trimmed else {
        return ("", content);
    };
    if let Some(index) = after_start.find("\n---\n") {
        return (&after_start[..index], &after_start[index + 5..]);
    }
    if let Some(index) = after_start.find("\r\n---\r\n") {
        return (&after_start[..index], &after_start[index + 7..]);
    }
    ("", content)
}

fn parse_frontmatter(frontmatter: &str, fallback_title: &str) -> ArenaFrontmatter {
    let mut card_type = "note".to_string();
    let mut title = fallback_title.to_string();
    let mut tags = Vec::new();
    let mut aliases = Vec::new();
    let mut status = "draft".to_string();

    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "type" => card_type = value.to_string(),
            "title" => title = value.to_string(),
            "tags" => tags = parse_inline_list(value),
            "aliases" => aliases = parse_inline_list(value),
            "status" => status = value.to_string(),
            _ => {}
        }
    }

    ArenaFrontmatter {
        card_type,
        title,
        tags,
        aliases,
        status,
    }
}

fn parse_inline_list(value: &str) -> Vec<String> {
    value
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').trim_matches('\''))
        .filter(|item| !item.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn parse_wiki_links(content: &str) -> Vec<String> {
    let mut links = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("[[") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("]]") else {
            break;
        };
        let link = after_start[..end]
            .split('|')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        if !link.is_empty() && !links.contains(&link) {
            links.push(link);
        }
        rest = &after_start[end + 2..];
    }
    links
}

#[cfg(test)]
mod tests {
    use super::{parse_arena_markdown, render_arena_markdown};

    #[test]
    fn parses_frontmatter_tags_and_wiki_links() {
        let parsed = parse_arena_markdown(
            r#"---
type: formula
title: Traffic Flow
tags: [traffic, pde]
aliases: [LWR]
status: draft
---
# Traffic Flow

Uses [[Continuity Equation]] and [[Density]].
"#,
            "fallback",
        );

        assert_eq!(parsed.frontmatter.card_type, "formula");
        assert_eq!(parsed.frontmatter.title, "Traffic Flow");
        assert_eq!(parsed.frontmatter.tags, vec!["traffic", "pde"]);
        assert_eq!(parsed.frontmatter.aliases, vec!["LWR"]);
        assert_eq!(parsed.frontmatter.status, "draft");
        assert_eq!(parsed.links, vec!["Continuity Equation", "Density"]);
        assert!(parsed.body.contains("# Traffic Flow"));
    }

    #[test]
    fn renders_obsidian_style_markdown_card() {
        let rendered = render_arena_markdown(
            "finding",
            "Bottleneck Insight",
            &["traffic".to_string(), "queueing".to_string()],
            "Queue length grows near [[Capacity Drop]].",
        );

        assert!(rendered.contains("type: finding"));
        assert!(rendered.contains("title: Bottleneck Insight"));
        assert!(rendered.contains("tags: [traffic, queueing]"));
        assert!(rendered.contains("[[Capacity Drop]]"));
    }
}

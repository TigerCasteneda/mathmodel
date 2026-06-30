use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct ArenaCard {
    pub file_id: String,
    pub title: String,
    pub card_type: String,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub status: String,
    pub links: Vec<String>,
    pub backlinks: Vec<String>,
    pub unresolved_links: Vec<String>,
    pub content: String,
    pub updated_at: i64,
    /// `user_id` of the user who originally created this card. `None` for
    /// legacy rows inserted before the authorship migration.
    pub created_by: Option<String>,
    /// `user_id` of the user who most recently saved this card. `None` for
    /// legacy rows. Updated on every successful `update_card` /
    /// `append_log`; the corresponding `updated_at` ticks alongside it.
    pub last_edited_by: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ArenaIndex {
    pub cards: Vec<ArenaCard>,
    pub unresolved_links: Vec<String>,
}

pub fn build_arena_index(mut cards: Vec<ArenaCard>) -> ArenaIndex {
    let mut unresolved_links = Vec::new();
    for index in 0..cards.len() {
        let links = cards[index].links.clone();
        let mut unresolved = Vec::new();
        for link in links {
            let target = cards
                .iter()
                .find(|card| card.title == link || card.aliases.iter().any(|alias| alias == &link));

            if target.is_none() {
                if !unresolved.contains(&link) {
                    unresolved.push(link.clone());
                }
                if !unresolved_links.contains(&link) {
                    unresolved_links.push(link);
                }
            }
        }
        cards[index].unresolved_links = unresolved;
    }

    for source_index in 0..cards.len() {
        let source_title = cards[source_index].title.clone();
        let links = cards[source_index].links.clone();
        for link in links {
            if let Some(target_index) = cards.iter().position(|card| {
                card.title == link || card.aliases.iter().any(|alias| alias == &link)
            }) {
                if target_index != source_index
                    && !cards[target_index].backlinks.contains(&source_title)
                {
                    cards[target_index].backlinks.push(source_title.clone());
                }
            }
        }
    }

    ArenaIndex {
        cards,
        unresolved_links,
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateArenaCardRequest {
    pub card_type: String,
    pub title: String,
    pub tags: Option<Vec<String>>,
    pub body: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateArenaCardRequest {
    pub content: String,
    pub expected_updated_at: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct AppendArenaLogRequest {
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct AppendArenaLogResponse {
    pub file_id: String,
    pub content: String,
    pub updated_at: i64,
}

#[cfg(test)]
mod tests {
    use super::{build_arena_index, ArenaCard};

    fn card(file_id: &str, title: &str, aliases: Vec<&str>, links: Vec<&str>) -> ArenaCard {
        ArenaCard {
            file_id: file_id.to_string(),
            title: title.to_string(),
            card_type: "finding".to_string(),
            tags: Vec::new(),
            aliases: aliases.into_iter().map(ToOwned::to_owned).collect(),
            status: "draft".to_string(),
            links: links.into_iter().map(ToOwned::to_owned).collect(),
            backlinks: Vec::new(),
            unresolved_links: Vec::new(),
            content: String::new(),
            updated_at: 0,
            created_by: None,
            last_edited_by: None,
        }
    }

    #[test]
    fn builds_backlinks_and_unresolved_links() {
        let index = build_arena_index(vec![
            card("a", "Continuity Equation", vec!["mass balance"], vec![]),
            card(
                "b",
                "Traffic Model",
                vec![],
                vec!["mass balance", "Missing Parameter"],
            ),
        ]);

        let continuity = index
            .cards
            .iter()
            .find(|card| card.title == "Continuity Equation")
            .unwrap();
        let traffic = index
            .cards
            .iter()
            .find(|card| card.title == "Traffic Model")
            .unwrap();

        assert_eq!(continuity.backlinks, vec!["Traffic Model"]);
        assert_eq!(traffic.unresolved_links, vec!["Missing Parameter"]);
        assert_eq!(index.unresolved_links, vec!["Missing Parameter"]);
    }
}

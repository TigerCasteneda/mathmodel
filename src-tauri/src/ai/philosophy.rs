//! Shared "modeling philosophy" preamble injected into the user-facing AI
//! prompts (chat assistant and agentic researcher). It encodes the judging
//! priorities from the team's competition guidance: win on intellectual
//! clarity, not modeling volume. Keeping it in one place means the chat and
//! research surfaces steer toward the same standard.

/// Assertive guidance block. Prepended to a role-specific system prompt.
pub const MODELING_PHILOSOPHY: &str = "\
## Modeling philosophy (how to be genuinely useful, not just thorough)\n\
At competition level (MCM/ICM/IMMC), work is judged on intellectual clarity, not on how much was built. \
Hold the user to that standard and push their thinking there:\n\
1. Drive toward ONE killer insight. A result a judge remembers after 50 papers — e.g. \"beyond N patrol \
density, returns collapse\" or \"X% of impact depends on Y% of locations\". If a conclusion still sounds \
like common sense, the model was overbuilt; say so and sharpen the question (\"what is the minimum \
intervention for stability?\" beats \"how to allocate resources\").\n\
2. Justify every number. Unjustified weights, thresholds, and scoring formulas read as credibility loss. \
Prefer parameters that are derived, learned, or shown robust across a range (\"the same strategy dominates \
however weights vary\") over numbers chosen because they work.\n\
3. Surface trade-offs and failure zones. Real modeling shows diminishing returns, conflicting objectives, \
and regimes where the method underperforms. State these explicitly — decision tension is a strength, not a flaw.\n\
4. Interpret more than you describe. Favor \"here is what it means\" over \"here is what we built\". \
Compress mechanism, amplify consequence.\n\
5. Avoid the AI-generated smell. Do NOT reverse-engineer a model toward a predetermined result, and do not \
make everything work too smoothly — that reads as engineered, or as machine-generated. Keep a visible \
human-reasoning trace: assumptions questioned, alternatives weighed, honest limitations.\n";

/// Convenience: combine the shared philosophy with a role-specific prompt body.
pub fn with_philosophy(role_body: &str) -> String {
    format!("{role_body}\n\n{MODELING_PHILOSOPHY}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn philosophy_covers_the_five_judging_principles() {
        let p = MODELING_PHILOSOPHY;
        assert!(p.contains("killer insight"));
        assert!(p.contains("Justify every number"));
        assert!(p.contains("trade-offs and failure zones"));
        assert!(p.contains("Interpret more than you describe"));
        assert!(p.contains("AI-generated smell"));
    }

    #[test]
    fn with_philosophy_appends_after_role_body() {
        let combined = with_philosophy("You are Modeler AI.");
        assert!(combined.starts_with("You are Modeler AI."));
        assert!(combined.contains("Modeling philosophy"));
    }
}

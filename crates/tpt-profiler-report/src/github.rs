//! GitHub Actions annotations (spec §39): one workflow command per
//! opportunity so findings appear inline on the pull request diff and in
//! the job summary.
//!
//! Every opportunity is emitted as a `notice`; the CI policy layer decides
//! what fails a build (spec §39). Annotations attach to a file only when
//! the opportunity carries a source location. Locations are relative to
//! the owning crate, so they line up with repository paths when the crate
//! sits at the repository root.

use std::fmt::Write as _;

use tpt_profiler_model::{FindingKind, Opportunity};

/// Renders one `::notice` workflow command per opportunity, newline
/// terminated. An empty list yields an empty string.
#[must_use]
pub fn render_github_annotations(opportunities: &[Opportunity]) -> String {
    let mut out = String::new();
    for opportunity in opportunities {
        let location = opportunity
            .evidence
            .iter()
            .find_map(|evidence| evidence.location.as_ref());

        let mut properties = format!("title={}", escape_property(&opportunity.id.to_string()));
        if let Some(location) = location {
            let _ = write!(
                properties,
                ",file={},line={}",
                escape_property(&location.file.replace('\\', "/")),
                location.line_start.max(1),
            );
            if location.line_end > location.line_start {
                let _ = write!(properties, ",endLine={}", location.line_end);
            }
        }

        let _ = writeln!(
            out,
            "::notice {properties}::{}",
            escape_data(&message(opportunity))
        );
    }
    out
}

fn message(opportunity: &Opportunity) -> String {
    let basis = match opportunity.kind {
        FindingKind::Deterministic => "static (deterministic)",
        FindingKind::Statistical => "measured (statistical)",
    };
    let priority = opportunity
        .priority
        .map_or_else(|| String::from("n/a"), |priority| priority.to_string());
    format!(
        "{} — {} (priority {priority}, {basis})",
        opportunity.description, opportunity.recommendation.action
    )
}

/// Escapes a workflow-command message body (`%`, CR, LF).
fn escape_data(text: &str) -> String {
    text.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Escapes a workflow-command property value (body escapes plus `:` and
/// `,`, which delimit properties).
fn escape_property(text: &str) -> String {
    escape_data(text).replace(':', "%3A").replace(',', "%2C")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_message_body() {
        assert_eq!(escape_data("100% done\r\nnext"), "100%25 done%0D%0Anext");
    }

    #[test]
    fn escapes_property_delimiters() {
        assert_eq!(escape_property("a:b,c"), "a%3Ab%2Cc");
    }

    #[test]
    fn empty_run_emits_nothing() {
        assert_eq!(render_github_annotations(&[]), "");
    }
}

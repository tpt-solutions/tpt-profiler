//! HTML output (spec §42): a single self-contained page with a ranked
//! table and per-opportunity detail, distinguishing measured facts from
//! static findings (spec §29). The page has no external resources, scripts
//! or network references, so it can be opened offline or archived as-is
//! (spec §70).

use std::fmt::Write as _;

use tpt_profiler_model::{FindingKind, Opportunity};

const STYLE: &str = "\
body{font-family:system-ui,sans-serif;margin:2rem;color:#1d1d1f;background:#fff}\
h1{font-size:1.5rem}h2{font-size:1.1rem;margin-top:2rem}\
table{border-collapse:collapse;width:100%}\
th,td{text-align:left;padding:.4rem .6rem;border-bottom:1px solid #ddd;vertical-align:top}\
th{background:#f5f5f7}code{font-family:ui-monospace,monospace;font-size:.9em}\
.measured{color:#0a6b3a}.static{color:#7a5a00}\
@media (prefers-color-scheme:dark){\
body{color:#e5e5ea;background:#1c1c1e}th{background:#2c2c2e}\
th,td{border-color:#3a3a3c}.measured{color:#6fd39a}.static{color:#f0c35b}}";

/// Renders opportunities as one self-contained HTML document.
#[must_use]
pub fn render_html(repository: &str, opportunities: &[Opportunity]) -> String {
    let title = escape(&format!("tpt-profiler report — {repository}"));
    let mut out = String::new();
    let _ = writeln!(out, "<!doctype html>");
    let _ = writeln!(out, "<html lang=\"en\">");
    let _ = writeln!(out, "<head>");
    let _ = writeln!(out, "<meta charset=\"utf-8\">");
    let _ = writeln!(out, "<title>{title}</title>");
    let _ = writeln!(out, "<style>{STYLE}</style>");
    let _ = writeln!(out, "</head>");
    let _ = writeln!(out, "<body>");
    let _ = writeln!(out, "<h1>{title}</h1>");

    if opportunities.is_empty() {
        let _ = writeln!(out, "<p>No opportunities detected.</p>");
    } else {
        write_table(&mut out, opportunities);
        for opportunity in opportunities {
            write_detail(&mut out, opportunity);
        }
    }

    let _ = writeln!(out, "</body>");
    let _ = writeln!(out, "</html>");
    out
}

fn write_table(out: &mut String, opportunities: &[Opportunity]) {
    let _ = writeln!(out, "<table>");
    let _ = writeln!(
        out,
        "<thead><tr><th>#</th><th>ID</th><th>Category</th><th>Component</th>\
         <th>Impact</th><th>Confidence</th><th>Effort</th><th>Priority</th><th>Basis</th></tr></thead>"
    );
    let _ = writeln!(out, "<tbody>");
    for (index, opportunity) in opportunities.iter().enumerate() {
        let priority = opportunity
            .priority
            .map_or_else(|| String::from("n/a"), |priority| priority.to_string());
        let (basis_label, basis_class) = basis(opportunity.kind);
        let _ = writeln!(
            out,
            "<tr><td>{}</td><td><code>{}</code></td><td>{}</td><td><code>{}</code></td>\
             <td>{}/10</td><td>{}</td><td>{}</td><td>{}</td><td class=\"{}\">{}</td></tr>",
            index + 1,
            escape(&opportunity.id.to_string()),
            escape(&opportunity.category.to_string()),
            escape(&opportunity.component),
            opportunity.impact.value(),
            escape(&opportunity.confidence.to_string()),
            escape(&opportunity.effort.to_string()),
            escape(&priority),
            basis_class,
            basis_label,
        );
    }
    let _ = writeln!(out, "</tbody>");
    let _ = writeln!(out, "</table>");
}

fn write_detail(out: &mut String, opportunity: &Opportunity) {
    let _ = writeln!(
        out,
        "<h2><code>{}</code> — {}</h2>",
        escape(&opportunity.id.to_string()),
        escape(&opportunity.description)
    );
    let _ = writeln!(out, "<ul>");
    let _ = writeln!(
        out,
        "<li><strong>Interpretation:</strong> {} ({})</li>",
        escape(&opportunity.interpretation.statement),
        escape(&opportunity.interpretation.confidence.to_string())
    );
    let _ = writeln!(
        out,
        "<li><strong>Recommendation:</strong> {} — {}</li>",
        escape(&opportunity.recommendation.action),
        escape(&opportunity.recommendation.rationale)
    );
    let _ = writeln!(
        out,
        "<li><strong>Risk:</strong> overall {}/10</li>",
        opportunity.risk.overall.value()
    );
    if !opportunity.evidence.is_empty() {
        let _ = writeln!(out, "<li><strong>Evidence:</strong><ul>");
        for evidence in &opportunity.evidence {
            let location = evidence
                .location
                .as_ref()
                .map_or_else(String::new, |location| {
                    format!(" (<code>{}</code>)", escape(&location.to_string()))
                });
            let _ = writeln!(
                out,
                "<li>{:?}{}: {}</li>",
                evidence.kind,
                location,
                escape(&evidence.description)
            );
        }
        let _ = writeln!(out, "</ul></li>");
    }
    let _ = writeln!(out, "</ul>");
}

fn basis(kind: FindingKind) -> (&'static str, &'static str) {
    match kind {
        FindingKind::Deterministic => ("static (deterministic)", "static"),
        FindingKind::Statistical => ("measured (statistical)", "measured"),
    }
}

/// Escapes text for HTML element content and double-quoted attributes.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_neutralises_markup() {
        assert_eq!(
            escape(r#"<script>"a" & 'b'</script>"#),
            "&lt;script&gt;&quot;a&quot; &amp; &#39;b&#39;&lt;/script&gt;"
        );
    }

    #[test]
    fn empty_run_renders_a_valid_page_without_a_table() {
        let html = render_html("tpt-cadence", &[]);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.contains("No opportunities detected."));
        assert!(!html.contains("<table>"));
    }

    #[test]
    fn page_is_self_contained() {
        let html = render_html("repo", &[]);
        assert!(!html.contains("<script"));
        assert!(!html.contains("http://"));
        assert!(!html.contains("https://"));
        assert!(!html.contains("<link"));
    }

    #[test]
    fn repository_name_is_escaped_in_title_and_heading() {
        let html = render_html("<img src=x>", &[]);
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;img src=x&gt;"));
    }
}

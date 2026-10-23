//! Terminal rendering of opportunities (spec §42, §77).
//!
//! The ranked list answers "what should I do next": highest priority
//! first, each entry showing the scored dimensions and — per spec §29 —
//! whether the finding is *deterministic* (static) or *statistical*
//! (measured). Colour is the caller's decision (NO_COLOR / tty
//! detection at the CLI layer); this renderer only emits plain text or
//! simple ANSI when asked.

use std::string::String;

use tpt_profiler_model::Opportunity;

/// Terminal rendering options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalOptions {
    /// Emit ANSI colour codes ( honoured only when the caller determined
    /// colour is safe: tty + NO_COLOR unset).
    pub colour: bool,
    /// Terminal width for wrapping/truncating (width-aware, spec §42).
    pub width: usize,
}

impl Default for TerminalOptions {
    fn default() -> Self {
        Self {
            colour: false,
            width: 100,
        }
    }
}

/// Renders the ranked opportunity report (spec §77 format).
#[must_use]
pub fn render_opportunities(
    repository: &str,
    opportunities: &[Opportunity],
    options: &TerminalOptions,
) -> String {
    let mut out = String::new();
    out.push_str("TPT PROFILER\n");
    out.push_str("============\n\n");
    out.push_str("Repository: ");
    out.push_str(repository);
    out.push_str("\n\n");

    if opportunities.is_empty() {
        out.push_str("No opportunities detected.\n");
        return out;
    }

    for (index, opportunity) in opportunities.iter().enumerate() {
        let rank = index + 1;
        let priority = opportunity
            .priority
            .map_or_else(|| String::from("n/a"), |priority| priority.to_string());
        out.push_str(&heading(
            &format!(
                "#{rank} {} — {} [priority {}]",
                category_label(opportunity),
                opportunity.component,
                priority
            ),
            options,
        ));
        out.push_str(&item(
            &format!("Function/Component: {}", opportunity.component),
            options,
        ));
        out.push_str(&item(
            &format!("Impact: {}/10", opportunity.impact.value()),
            options,
        ));
        out.push_str(&item(
            &format!("Confidence: {}", opportunity.confidence),
            options,
        ));
        out.push_str(&item(&format!("Effort: {}", opportunity.effort), options));
        out.push_str(&item(
            &format!(
                "Basis: {}",
                match opportunity.kind {
                    tpt_profiler_model::FindingKind::Deterministic => {
                        "deterministic (static analysis)"
                    }
                    tpt_profiler_model::FindingKind::Statistical => {
                        "statistical (measured)"
                    }
                }
            ),
            options,
        ));
        out.push_str(&item(
            &wrap(&opportunity.description, options.width.saturating_sub(4)),
            options,
        ));
        out.push('\n');
    }
    out
}

fn category_label(opportunity: &Opportunity) -> &'static str {
    opportunity.category.as_slug()
}

fn heading(line: &str, options: &TerminalOptions) -> String {
    if options.colour {
        format!("\x1b[1;33m{line}\x1b[0m\n")
    } else {
        format!("{line}\n")
    }
}

fn item(line: &str, options: &TerminalOptions) -> String {
    let _ = options;
    format!("    {line}\n")
}

/// Greedy word wrap; falls back to hard character cuts for words longer
/// than the width (width-aware output, spec §42).
#[must_use]
pub fn wrap(text: &str, width: usize) -> String {
    if width == 0 {
        return String::from(text);
    }
    let mut wrapped = String::new();
    for (index, line) in text.lines().enumerate() {
        if index > 0 {
            wrapped.push('\n');
        }
        let mut current = 0_usize;
        for word in line.split_whitespace() {
            let word_length = word.chars().count();
            if current == 0 {
                current = word_length.min(width);
                wrapped.push_str(&word.chars().take(width).collect::<String>());
            } else if current + 1 + word_length <= width {
                wrapped.push(' ');
                wrapped.push_str(word);
                current += 1 + word_length;
            } else {
                wrapped.push('\n');
                wrapped.push_str(&word.chars().take(width).collect::<String>());
                current = word_length.min(width);
            }
        }
    }
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;
    use tpt_profiler_model::{
        Category, Confidence, Effort, Evidence, FindingKind, Impact, Interpretation,
        LifecycleState, Opportunity, OpportunityId, Priority, Recommendation, Risk, RiskDimensions,
        RiskScore,
    };

    fn sample(kind: FindingKind) -> Opportunity {
        Opportunity {
            id: OpportunityId::generate("SIMD", "mdct::transform", "vectorisable loop"),
            repository: String::from("tpt-cadence"),
            component: String::from("mdct::transform"),
            category: Category::Simd,
            description: String::from("independent scalar arithmetic over contiguous input"),
            evidence: vec![Evidence::static_pattern(
                "element-wise arithmetic",
                tpt_profiler_model::SourceLocation::at_line("src/mdct.rs", 42),
            )],
            impact: Impact::new(9).unwrap(),
            confidence: Confidence::High,
            effort: Effort::Medium,
            risk: Risk {
                overall: RiskScore::new(3).unwrap(),
                dimensions: RiskDimensions::default(),
            },
            generality: tpt_profiler_model::Generality::new(8).unwrap(),
            priority: Priority::new(85).ok(),
            interpretation: Interpretation {
                statement: String::from("hot numerical kernel"),
                confidence: Confidence::High,
            },
            recommendation: Recommendation {
                action: String::from("investigate vectorisation"),
                rationale: String::from("independent fixed-width arithmetic"),
            },
            state: LifecycleState::Open,
            kind,
            relationships: Vec::new(),
        }
    }

    #[test]
    fn golden_terminal_output() {
        let text = render_opportunities(
            "tpt-cadence",
            &[sample(FindingKind::Deterministic)],
            &TerminalOptions::default(),
        );
        let expected = concat!(
            "TPT PROFILER\n",
            "============\n",
            "\n",
            "Repository: tpt-cadence\n",
            "\n",
            "#1 SIMD — mdct::transform [priority 85/100]\n",
            "    Function/Component: mdct::transform\n",
            "    Impact: 9/10\n",
            "    Confidence: 8/10 (high)\n",
            "    Effort: 5/10 (medium)\n",
            "    Basis: deterministic (static analysis)\n",
            "    independent scalar arithmetic over contiguous input\n",
            "\n",
        );
        assert_eq!(text, expected);
    }

    #[test]
    fn empty_list_is_a_stated_result() {
        let text = render_opportunities("repo", &[], &TerminalOptions::default());
        assert!(text.contains("No opportunities detected."));
    }

    #[test]
    fn statistical_basis_is_labelled_measured() {
        let text = render_opportunities(
            "repo",
            &[sample(FindingKind::Statistical)],
            &TerminalOptions::default(),
        );
        assert!(text.contains("statistical (measured)"), "{text}");
    }

    #[test]
    fn colour_wraps_heading_in_ansi() {
        let text = render_opportunities(
            "repo",
            &[sample(FindingKind::Deterministic)],
            &TerminalOptions {
                colour: true,
                width: 100,
            },
        );
        assert!(text.contains("\x1b[1;33m#1"), "{text}");
    }

    #[test]
    fn wrap_respects_width() {
        let wrapped = wrap("alpha beta gamma delta", 11);
        assert_eq!(wrapped, "alpha beta\ngamma delta");
        assert_eq!(wrap("aaaaaaaaaaaa", 5).chars().count(), 5);
        assert_eq!(wrap("untouched", 0), "untouched");
    }
}

//! Schema compatibility tests (spec §69): the emitted v1 JSON and JSONL
//! shapes are frozen in `tests/fixtures/`. A change that alters a field,
//! its type, or its order fails here and must come with a schema version
//! bump, so downstream TPT tools never see silent drift.

use tpt_profiler_model::{
    Category, Confidence, Effort, FindingKind, Generality, Impact, Interpretation, LifecycleState,
    Opportunity, OpportunityId, Priority, Recommendation, Risk, RiskDimensions, RiskScore,
};
use tpt_profiler_report::{render_opportunities_json, render_opportunities_jsonl};

const JSON_GOLDEN: &str = include_str!("fixtures/opportunities-v1.json");
const JSONL_GOLDEN: &str = include_str!("fixtures/opportunities-v1.jsonl");

fn fixed_opportunity() -> Opportunity {
    Opportunity {
        id: OpportunityId::generate("SIMD", "mdct::transform", "vectorisable loop"),
        repository: String::from("tpt-cadence"),
        component: String::from("mdct::transform"),
        category: Category::Simd,
        description: String::from("vectorisable loop"),
        evidence: Vec::new(),
        impact: Impact::new(9).expect("valid impact"),
        confidence: Confidence::High,
        effort: Effort::Medium,
        risk: Risk {
            overall: RiskScore::new(3).expect("valid risk"),
            dimensions: RiskDimensions::default(),
        },
        generality: Generality::new(8).expect("valid generality"),
        priority: Priority::new(85).ok(),
        interpretation: Interpretation {
            statement: String::from("hot kernel"),
            confidence: Confidence::High,
        },
        recommendation: Recommendation {
            action: String::from("vectorise"),
            rationale: String::from("independent arithmetic"),
        },
        state: LifecycleState::Open,
        kind: FindingKind::Deterministic,
        relationships: Vec::new(),
    }
}

#[test]
fn json_envelope_matches_frozen_v1_shape() {
    let actual =
        render_opportunities_json("tpt-profiler test", "tpt-cadence", &[fixed_opportunity()])
            .expect("encode");
    assert_eq!(actual.trim_end(), JSON_GOLDEN.trim_end());
}

#[test]
fn jsonl_lines_match_frozen_v1_shape() {
    let actual = render_opportunities_jsonl("tpt-cadence", &[fixed_opportunity()]).expect("encode");
    assert_eq!(actual, JSONL_GOLDEN);
}

#[test]
fn frozen_v1_json_still_decodes_as_a_generic_document() {
    let value: serde_json::Value = serde_json::from_str(JSON_GOLDEN).expect("golden is JSON");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["generated_by"], "tpt-profiler test");
    let entry = &value["data"]["opportunities"][0];
    for field in [
        "id",
        "category",
        "component",
        "impact",
        "confidence",
        "effort",
        "risk",
        "generality",
        "priority",
    ] {
        assert!(entry.get(field).is_some(), "missing v1 field `{field}`");
    }
}

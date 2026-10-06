//! Cross-module integration tests for the data model (spec §6, §24, §25,
//! §69, §75): a complete profile run with opportunities must round-trip
//! through serde unchanged, and deterministic IDs must be reproducible.

#![cfg(feature = "serde")]

use tpt_profiler_model::{
    Category, Confidence, Effort, Environment, Evidence, EvidenceKind, FindingKind, Generality,
    Impact, Interpretation, LifecycleState, Measurement, MeasurementSource, Opportunity,
    OpportunityId, Priority, ProfileRun, Recommendation, Relationship, RelationshipKind, Risk,
    RiskDimensions, RiskScore, SCHEMA_VERSION, SchemaEnvelope, SourceLocation, Unit, Workload,
};

fn sample_opportunity() -> Opportunity {
    let id = OpportunityId::generate("SIMD", "mdct::transform", "independent scalar arithmetic");
    Opportunity {
        id,
        repository: String::from("tpt-cadence"),
        component: String::from("mdct::transform"),
        category: Category::Simd,
        description: String::from("93% independent scalar arithmetic over contiguous input"),
        evidence: vec![
            Evidence::static_pattern(
                "fixed-width numeric loop without loop-carried dependency",
                SourceLocation::at_line("src/mdct.rs", 142).with_unsafe_region(),
            ),
            Evidence::measured(
                EvidenceKind::RuntimeProfile,
                "41% of CPU time attributed to transform_block",
                Measurement::runtime("cpu_time", 1.482, Unit::Seconds, MeasurementSource::Perf, 0),
            ),
        ],
        impact: Impact::new(9).unwrap(),
        confidence: Confidence::High,
        effort: Effort::Medium,
        risk: Risk {
            overall: RiskScore::new(3).unwrap(),
            dimensions: RiskDimensions {
                numerical: Some(RiskScore::new(6).unwrap()),
                ..RiskDimensions::default()
            },
        },
        generality: Generality::new(8).unwrap(),
        priority: Some(Priority::new(86).unwrap()),
        interpretation: Interpretation {
            statement: String::from("hot numerical kernel, vectorisable"),
            confidence: Confidence::High,
        },
        recommendation: Recommendation {
            action: String::from("investigate vectorisation"),
            rationale: String::from("independent fixed-width arithmetic over contiguous data"),
        },
        state: LifecycleState::Open,
        kind: FindingKind::Deterministic,
        relationships: vec![],
    }
}

#[test]
fn profile_run_round_trips_through_json() {
    let mut run = ProfileRun::new("run-0001", 1_760_000_000, Environment::default());
    run.environment.cpu_cores = Some(16);
    run.environment
        .opt_flags
        .push(String::from("-C target-cpu=native"));
    run.measurements.push(Measurement::runtime(
        "wall_time",
        30.5,
        Unit::Seconds,
        MeasurementSource::WallClock,
        1_760_000_001,
    ));
    run.opportunities.push(sample_opportunity());
    run.opportunities[0].relationships.push(Relationship {
        target: OpportunityId::generate("DATA_STRUCTURE", "mdct", "contiguity"),
        kind: RelationshipKind::Requires,
    });

    let envelope = SchemaEnvelope::new("tpt-profiler test", &run);
    let json = serde_json::to_string(&envelope).expect("encode");
    assert!(json.contains(&format!("\"schema_version\":{SCHEMA_VERSION}")));

    let decoded: SchemaEnvelope<ProfileRun> = serde_json::from_str(&json).expect("decode");
    assert_eq!(decoded.validate(), Ok(SCHEMA_VERSION));
    assert_eq!(decoded.data, run);
}

#[test]
fn deterministic_ids_survive_reconstruction() {
    let a = OpportunityId::generate("SIMD", "mdct::transform", "independent scalar arithmetic");
    let b = OpportunityId::generate("SIMD", "mdct::transform", "independent scalar arithmetic");
    assert_eq!(a, b);
    assert!(a.as_str().starts_with("SIMD-"));
}

#[test]
fn workloads_compose_into_projects() {
    let workload = Workload::named("decode_1080p");
    assert_eq!(workload.name, "decode_1080p");
}

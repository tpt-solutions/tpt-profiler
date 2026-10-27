//! Regression corpus tests (spec §76): every `// expect: <detector>`
//! marker in `tests/corpus/hot_loop.rs` must produce a finding from that
//! detector, negative markers must not fire, and the whole corpus must
//! analyse with no parse errors, in deterministic order.

use std::collections::BTreeMap;
use std::path::Path;
use tpt_profiler_static::StaticEngine;

fn corpus_file(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/corpus")
        .join(name)
}

#[test]
fn every_expectation_in_the_corpus_is_met() {
    let source = std::fs::read_to_string(corpus_file("hot_loop.rs")).expect("read corpus");
    let engine = StaticEngine::with_default_analyzers();
    let (findings, parse_errors) = engine.analyze_str("hot_loop.rs", &source);
    assert!(parse_errors.is_empty(), "{parse_errors:?}");

    let mut seen: BTreeMap<&'static str, Vec<u32>> = BTreeMap::new();
    for finding in &findings {
        seen.entry(finding.detector)
            .or_default()
            .push(finding.location.line_start);
    }

    let mut expected = 0;
    for (line_no, line) in source.lines().enumerate() {
        let Some(marker) = line.trim().strip_prefix("// expect: ") else {
            continue;
        };
        let detector = marker.split(['(', ' ']).next().expect("marker");
        expected += 1;
        let lines = seen.get(detector).unwrap_or_else(|| {
            panic!(
                "{detector} finding missing for expectation on line {}",
                line_no + 1
            )
        });
        // The finding must appear on the marker line, or the line
        // before/after (loop-header findings land on the `for` line).
        let target = u32::try_from(line_no + 1).unwrap_or(u32::MAX);
        assert!(
            lines.contains(&(target - 1))
                || lines.contains(&target)
                || lines.contains(&(target + 1)),
            "{detector} expected near line {target}, found at {lines:?}"
        );
    }
    assert!(expected >= 6, "corpus must carry meaningful expectations");

    // Negative markers: Arc::clone line and the suppressed clone must
    // produce no clone finding.
    let clone_lines = seen.get("clone").expect("clone findings exist");
    assert!(
        !clone_lines.iter().any(|line| *line >= 47 && *line <= 49),
        "suppressed clone leaked: {clone_lines:?}"
    );
}

#[test]
fn clean_corpus_yields_no_findings() {
    let source = std::fs::read_to_string(corpus_file("clean.rs")).expect("read corpus");
    let engine = StaticEngine::with_default_analyzers();
    let (findings, parse_errors) = engine.analyze_str("clean.rs", &source);
    assert_eq!(parse_errors.len(), 0);
    assert!(
        findings.is_empty(),
        "false positives on clean code: {findings:?}"
    );
}

#[test]
fn path_analysis_is_deterministic_and_complete() {
    let engine = StaticEngine::with_default_analyzers();
    let corpus_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let first = engine.analyze_path(&corpus_dir).expect("analyse");
    let second = engine.analyze_path(&corpus_dir).expect("analyse");

    assert_eq!(first.files_analyzed, 2, "both corpus files analysed");
    assert_eq!(first.parse_errors.len(), 0);
    assert_eq!(first.findings, second.findings, "deterministic (spec §75)");
    assert_ne!(first.findings.len(), 0);

    // Deterministic order: sorted by (file, line).
    for pair in first.findings.windows(2) {
        let a = (&pair[0].location.file, pair[0].location.line_start);
        let b = (&pair[1].location.file, pair[1].location.line_start);
        assert!(a <= b, "findings out of order: {a:?} > {b:?}");
    }
}

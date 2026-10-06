//! Regression corpus: known-detectable opportunities that must stay
//! detectable (spec §76). Each pattern carries a `// expect: <detector>`
//! comment; `tests/regression.rs` asserts every expectation is met.

pub struct BigFrame {
    pub samples: Vec<f64>,
}

pub fn transform_block(out: &mut [f64], left: &[f64], right: &[f64], frames: &[BigFrame]) {
    let mut cache: Vec<BigFrame> = Vec::new();
    for i in 0..out.len() {
        // expect: simd (element-wise arithmetic)
        out[i] = left[i] * right[i] + 0.5;
    }
    for frame in frames {
        // expect: clone (deep copy inside loop)
        cache.push(frame.clone());
        // expect: allocation (collect inside loop)
        let loud: Vec<f64> = frame.samples.iter().copied().collect();
        // expect: datastructure (membership lookup inside loop)
        if loud.contains(&1.0) {
            // expect: loops (call does not use the loop variable)
            let gain = compute_gain();
            normalize(&mut cache, gain);
        }
    }
}

fn compute_gain() -> f64 {
    0.8
}

fn normalize(_frames: &mut [BigFrame], _gain: f64) {}

/// Heavy per-item work over a large literal range: parallelism candidate.
pub fn shade_all(pixels: &mut [u8], width: usize) {
    for i in 0..1_000_000 {
        // expect: parallelism (call per iteration, large range)
        pixels[i] = shade_pixel(i, width);
    }
}

fn shade_pixel(_i: usize, _width: usize) -> u8 {
    0
}

/// The `.clone()` on `arc_state` must NOT be flagged (Arc heuristic).
pub fn share_state(arc_state: &std::sync::Arc<BigFrame>, slots: &mut Vec<std::sync::Arc<BigFrame>>) {
    for _ in 0..1000 {
        slots.push(Arc::clone(arc_state));
    }
}

/// Suppressed finding: must not surface.
pub fn suppressed(items: &[String]) -> usize {
    let mut total = 0;
    for item in items {
        let copy = item.clone(); // tpt-profiler:ignore
        total += copy.len();
    }
    total
}

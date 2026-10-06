//! Regression corpus: code with NO detectable opportunities. The engine
//! must report zero findings here — "no opportunity detected" is a valid
//! result, and false positives on clean code are the cardinal sin
//! (spec §3.1.12, §80).

pub fn add_one(x: u64) -> u64 {
    x + 1
}

pub fn sum(values: &[u64]) -> u64 {
    values.iter().sum()
}

pub fn once(value: &[u8]) -> Option<&u8> {
    value.first()
}

pub fn build_lookup() -> std::collections::HashMap<u32, char> {
    let mut map = std::collections::HashMap::new();
    for digit in 0u32..10 {
        map.insert(digit + 48, char::from(digit as u8 + b'0'));
    }
    map
}

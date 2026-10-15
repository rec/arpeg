//! Source-aligned octave displacements for repeated chord rotations.

pub fn inversion_offsets(keys: &[u8], count: i64) -> Vec<i128> {
    let mut offsets = vec![0; keys.len()];
    if keys.is_empty() || count == 0 {
        return offsets;
    }
    let direction = if count > 0 { 1 } else { -1 };
    let mut ordered: Vec<_> = (0..keys.len()).collect();
    ordered.sort_unstable_by_key(|i| (direction * i128::from(keys[*i]), *i));
    let edge = direction * i128::from(keys[*ordered.last().unwrap()]);
    let mut end = edge;
    let mut first = Vec::with_capacity(keys.len());
    for i in &ordered {
        end += (direction * i128::from(keys[*i]) - end - 1).rem_euclid(12) + 1;
        first.push(end);
    }
    let span = end - edge;
    let cycles = i128::from(count).abs() / keys.len() as i128;
    let remainder = (i128::from(count).abs() % keys.len() as i128) as usize;
    for (j, i) in ordered.into_iter().enumerate() {
        if cycles > 0 || j < remainder {
            let pitch = if cycles > 0 {
                first[j] + (cycles - 1 + i128::from(j < remainder)) * span
            } else {
                first[j]
            };
            offsets[i] = direction * pitch - i128::from(keys[i]);
        }
    }
    offsets
}

//! Level mapping (§14.14).

/// Map a raw score `q ∈ [0, 1]` to a discrete level 1–5.
///
/// Bands per §14.14 (lower-inclusive, upper-exclusive, except the
/// top band which is closed on both ends):
///   1: [0.00, 0.20)   2: [0.20, 0.40)   3: [0.40, 0.60)
///   4: [0.60, 0.80)   5: [0.80, 1.00]
pub fn map_level(q: f64) -> u8 {
    if q < 0.20 {
        1
    } else if q < 0.40 {
        2
    } else if q < 0.60 {
        3
    } else if q < 0.80 {
        4
    } else {
        5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spec §14.14 boundaries:
    //   1: [0.00, 0.20), 2: [0.20, 0.40), 3: [0.40, 0.60),
    //   4: [0.60, 0.80), 5: [0.80, 1.00].

    #[test]
    fn zero_score_maps_to_level_1() {
        assert_eq!(map_level(0.0), 1);
    }

    #[test]
    fn boundary_lower_inclusive() {
        assert_eq!(map_level(0.20), 2);
        assert_eq!(map_level(0.40), 3);
        assert_eq!(map_level(0.60), 4);
        assert_eq!(map_level(0.80), 5);
    }

    #[test]
    fn boundary_upper_exclusive_except_1() {
        assert_eq!(map_level(0.1999), 1);
        assert_eq!(map_level(0.3999), 2);
        assert_eq!(map_level(0.5999), 3);
        assert_eq!(map_level(0.7999), 4);
    }

    #[test]
    fn max_score_maps_to_level_5() {
        // §14.14 explicitly defines [0.80, 1.00] (inclusive) as level 5.
        assert_eq!(map_level(1.0), 5);
    }

    #[test]
    fn worked_example_26_yields_level_5() {
        // Spec §26 worked example: Q ≈ 0.846 ⇒ L = 5.
        assert_eq!(map_level(0.846), 5);
    }

    #[test]
    fn mapping_is_monotonically_non_decreasing() {
        let mut prev = map_level(0.0);
        let mut q = 0.05;
        while q < 1.0 {
            let l = map_level(q);
            assert!(l >= prev, "non-monotonic at q={}: {} < {}", q, l, prev);
            prev = l;
            q += 0.05;
        }
    }
}

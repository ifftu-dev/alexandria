//! Host-side grading. The correct-answer key lives only here (server-side);
//! the client only ever submits the option positions it selected, in the
//! *shuffled* order it was shown. Grading maps those positions back to the
//! original option indices (via the attempt's stored `option_order`) and
//! compares to the question's `correct_indices`.

use std::collections::BTreeSet;

/// One graded question: its points, the correct ORIGINAL option indices (the
/// key), and the shuffled option order that was served to the client.
#[derive(Debug, Clone)]
pub struct GradedQuestion {
    pub points: f64,
    /// Correct answers as ORIGINAL option indices.
    pub correct_indices: Vec<usize>,
    /// `option_order[pos]` = original option index shown at served position `pos`.
    pub option_order: Vec<usize>,
}

/// The client's answer to one question: the served POSITIONS it selected.
pub type Answer = Vec<usize>;

/// Grade an attempt. `questions` and `answers` are parallel (same order the
/// questions were served). A question is correct only if the mapped-back
/// original selection set equals the correct set exactly (all-correct,
/// no-extra). Returns the fraction of points earned in `[0, 1]`.
pub fn grade(questions: &[GradedQuestion], answers: &[Answer]) -> f64 {
    let total: f64 = questions.iter().map(|q| q.points).sum();
    if total <= 0.0 {
        return 0.0;
    }
    let mut earned = 0.0;
    for (q, ans) in questions.iter().zip(answers.iter()) {
        // Map each served position back to its original option index.
        let selected: BTreeSet<usize> = ans
            .iter()
            .filter_map(|&pos| q.option_order.get(pos).copied())
            .collect();
        let correct: BTreeSet<usize> = q.correct_indices.iter().copied().collect();
        if selected == correct {
            earned += q.points;
        }
    }
    earned / total
}

#[cfg(test)]
mod tests {
    use super::*;

    // Question with 4 options, correct original index = 2, served shuffled so
    // that original 2 is at served position 0.
    fn q(correct: &[usize], order: &[usize]) -> GradedQuestion {
        GradedQuestion {
            points: 1.0,
            correct_indices: correct.to_vec(),
            option_order: order.to_vec(),
        }
    }

    #[test]
    fn maps_shuffled_selection_back_to_original() {
        // original correct = 2; served order [2,0,1,3] → original 2 at pos 0.
        let questions = vec![q(&[2], &[2, 0, 1, 3])];
        // client selects served position 0 → original 2 → correct.
        assert_eq!(grade(&questions, &[vec![0]]), 1.0);
        // client selects served position 1 → original 0 → wrong.
        assert_eq!(grade(&questions, &[vec![1]]), 0.0);
    }

    #[test]
    fn multi_select_requires_exact_set() {
        // correct originals {1,3}; served order [3,1,0,2] → orig1 at pos1, orig3 at pos0.
        let questions = vec![q(&[1, 3], &[3, 1, 0, 2])];
        assert_eq!(grade(&questions, &[vec![0, 1]]), 1.0); // {3,1} ✓
        assert_eq!(grade(&questions, &[vec![0]]), 0.0); // missing 1
        assert_eq!(grade(&questions, &[vec![0, 1, 2]]), 0.0); // extra 0
    }

    #[test]
    fn partial_score_across_questions() {
        let questions = vec![q(&[0], &[0, 1, 2, 3]), q(&[1], &[0, 1, 2, 3])];
        // first right (pos0→orig0), second wrong.
        assert_eq!(grade(&questions, &[vec![0], vec![0]]), 0.5);
    }

    #[test]
    fn empty_is_zero() {
        assert_eq!(grade(&[], &[]), 0.0);
    }
}

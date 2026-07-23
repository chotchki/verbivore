//! Detection eval: mAP@0.5 + mean matched IoU, accumulated image by image.
//! Pure Rust so the numbers are testable without a model in the loop.

use crate::data::NUM_CLASSES;
use crate::decode::{Detection, iou};

const MATCH_IOU: f32 = 0.5;

#[derive(Debug, Default, Clone)]
struct ClassAcc {
    /// (score, was_true_positive), across every observed image.
    scored: Vec<(f32, bool)>,
    ground_truth: usize,
}

/// Feed it (detections, ground truth) per image; ask for metrics at the end.
#[derive(Debug, Clone)]
pub struct EvalAccumulator {
    classes: Vec<ClassAcc>,
    matched_iou_sum: f64,
    matched: usize,
}

impl Default for EvalAccumulator {
    fn default() -> Self {
        Self {
            classes: vec![ClassAcc::default(); NUM_CLASSES],
            matched_iou_sum: 0.0,
            matched: 0,
        }
    }
}

impl EvalAccumulator {
    /// Greedy match by score: each detection takes the best unmatched same-class
    /// ground truth at IoU >= 0.5; duplicates become false positives.
    pub fn observe(&mut self, detections: &[Detection], gt_boxes: &[[f32; 4]], gt_classes: &[i64]) {
        for (&class, _) in gt_classes.iter().zip(gt_boxes) {
            if (class as usize) < NUM_CLASSES {
                self.classes[class as usize].ground_truth += 1;
            }
        }

        let mut dets: Vec<&Detection> = detections.iter().collect();
        dets.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut taken = vec![false; gt_boxes.len()];

        for det in dets {
            let mut best: Option<(usize, f32)> = None;
            for (i, (gt, &gc)) in gt_boxes.iter().zip(gt_classes).enumerate() {
                if taken[i] || gc as usize != det.class {
                    continue;
                }
                let overlap = iou(&det.bbox, gt);
                if overlap >= MATCH_IOU && best.is_none_or(|(_, b)| overlap > b) {
                    best = Some((i, overlap));
                }
            }
            let tp = if let Some((i, overlap)) = best {
                taken[i] = true;
                self.matched_iou_sum += overlap as f64;
                self.matched += 1;
                true
            } else {
                false
            };
            if det.class < NUM_CLASSES {
                self.classes[det.class].scored.push((det.score, tp));
            }
        }
    }

    /// Mean AP@0.5 over classes that have ground truth.
    pub fn map50(&self) -> f64 {
        let aps: Vec<f64> = self
            .classes
            .iter()
            .filter(|c| c.ground_truth > 0)
            .map(average_precision)
            .collect();
        if aps.is_empty() {
            return 0.0;
        }
        aps.iter().sum::<f64>() / aps.len() as f64
    }

    /// Mean IoU of matched pairs — how TIGHT the boxes are, not how many.
    pub fn mean_matched_iou(&self) -> f64 {
        if self.matched == 0 {
            return 0.0;
        }
        self.matched_iou_sum / self.matched as f64
    }
}

/// All-point interpolated AP (area under the precision envelope).
fn average_precision(acc: &ClassAcc) -> f64 {
    if acc.scored.is_empty() {
        return 0.0;
    }
    let mut scored = acc.scored.clone();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut points = Vec::with_capacity(scored.len());
    let mut tp = 0usize;
    for (i, &(_, is_tp)) in scored.iter().enumerate() {
        if is_tp {
            tp += 1;
        }
        points.push((
            tp as f64 / acc.ground_truth as f64, // recall
            tp as f64 / (i + 1) as f64,          // precision
        ));
    }
    // Precision envelope: monotone non-increasing from the right.
    for i in (0..points.len().saturating_sub(1)).rev() {
        points[i].1 = points[i].1.max(points[i + 1].1);
    }
    let mut ap = 0.0;
    let mut prev_recall = 0.0;
    for (recall, precision) in points {
        ap += (recall - prev_recall) * precision;
        prev_recall = recall;
    }
    ap
}

#[cfg(test)]
mod tests {
    use super::*;

    fn det(class: usize, score: f32, bbox: [f32; 4]) -> Detection {
        Detection { bbox, class, score }
    }

    #[test]
    fn perfect_detections_score_full_map() {
        let mut acc = EvalAccumulator::default();
        let boxes = [[0.0, 0.0, 20.0, 20.0], [100.0, 100.0, 140.0, 120.0]];
        acc.observe(
            &[det(0, 0.9, boxes[0]), det(1, 0.8, boxes[1])],
            &boxes,
            &[0, 1],
        );
        assert_eq!(acc.map50(), 1.0);
        assert_eq!(acc.mean_matched_iou(), 1.0);
    }

    #[test]
    fn missing_everything_scores_zero() {
        let mut acc = EvalAccumulator::default();
        acc.observe(&[], &[[0.0, 0.0, 20.0, 20.0]], &[0]);
        assert_eq!(acc.map50(), 0.0);
    }

    #[test]
    fn one_perfect_one_missed_class_averages_to_half() {
        let mut acc = EvalAccumulator::default();
        let boxes = [[0.0, 0.0, 20.0, 20.0], [100.0, 100.0, 140.0, 120.0]];
        acc.observe(&[det(0, 0.9, boxes[0])], &boxes, &[0, 1]);
        assert_eq!(acc.map50(), 0.5);
    }

    #[test]
    fn duplicate_detection_costs_precision() {
        let mut acc = EvalAccumulator::default();
        let gt = [[0.0, 0.0, 20.0, 20.0]];
        // Same box twice: the higher-scored one matches, the second is a FP...
        acc.observe(
            &[det(0, 0.9, gt[0]), det(0, 0.8, [1.0, 1.0, 21.0, 21.0])],
            &gt,
            &[0],
        );
        // ...but AP stays 1.0 because the TP outranks the FP; recall saturates
        // before precision drops, and the envelope ignores the tail.
        assert_eq!(acc.map50(), 1.0);

        // Flip the ranking (FP outscores the TP) and AP must drop.
        let mut acc = EvalAccumulator::default();
        acc.observe(
            &[det(0, 0.9, [50.0, 50.0, 70.0, 70.0]), det(0, 0.8, gt[0])],
            &gt,
            &[0],
        );
        assert_eq!(acc.map50(), 0.5);
    }

    #[test]
    fn wrong_class_never_matches() {
        let mut acc = EvalAccumulator::default();
        let gt = [[0.0, 0.0, 20.0, 20.0]];
        acc.observe(&[det(1, 0.9, gt[0])], &gt, &[0]);
        assert_eq!(acc.map50(), 0.0);
    }
}

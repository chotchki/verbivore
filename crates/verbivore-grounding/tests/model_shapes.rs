use burn::module::Module;
use burn::prelude::*;
use verbivore_grounding::data::{INPUT_SIZE, NUM_CLASSES};
use verbivore_grounding::model::{GroundingModel, OUTPUT_STRIDE};

// Metal, not ndarray: a 640px conv forward takes minutes on CPU, sub-second on
// the GPU this model actually trains on.
type B = burn::backend::Wgpu;

#[test]
fn forward_produces_stride4_heads() {
    let device = Default::default();
    let model = GroundingModel::<B>::init(&device);

    let side = INPUT_SIZE as usize;
    let grid = side / OUTPUT_STRIDE;
    let images = Tensor::<B, 4>::zeros([2, 3, side, side], &device);
    let out = model.forward(images);

    assert_eq!(out.heatmap.dims(), [2, NUM_CLASSES, grid, grid]);
    assert_eq!(out.sizes.dims(), [2, 2, grid, grid]);
    assert_eq!(out.offsets.dims(), [2, 2, grid, grid]);

    let params = model.num_params();
    assert!(
        (500_000..10_000_000).contains(&params),
        "unexpected model size: {params} params"
    );
}

/// Metal (via burn-wgpu) must initialize and do arithmetic on this machine —
/// this is the backend 2.6 trains on. Failing here means driver/backend trouble,
/// not model trouble.
#[test]
fn wgpu_metal_backend_computes() {
    use burn::prelude::*;
    type B = burn::backend::Wgpu;
    let device = Default::default();
    let t = Tensor::<B, 1>::from_floats([1.0, 2.0, 3.0, 4.0], &device);
    let sum: f32 = (t.clone() * t).sum().into_scalar();
    assert_eq!(sum, 30.0);
}

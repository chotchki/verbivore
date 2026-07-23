//! CenterNet-style anchor-free detector: per-class center heatmap + size + offset
//! heads at stride 4. Chosen over FCOS/YOLO shapes because decode is trivial
//! (heatmap peaks ARE the detections) and UI elements rarely overlap enough to
//! need anchor machinery. ~2M params, from scratch — the auto-labeled corpus is
//! big enough that pretrained backbones aren't worth a porting slog (see SPEC).

use burn::nn::conv::{Conv2d, Conv2dConfig, ConvTranspose2d, ConvTranspose2dConfig};
use burn::nn::{BatchNorm, BatchNormConfig, PaddingConfig2d, Relu};
use burn::prelude::*;

use crate::data::NUM_CLASSES;

/// Input px per output cell: 640 -> 160x160 grid.
pub const OUTPUT_STRIDE: usize = 4;

/// Raw head outputs; heatmap is logits (loss applies the sigmoid/focal shaping).
#[derive(Debug, Clone)]
pub struct Detections<B: Backend> {
    /// [batch, NUM_CLASSES, 160, 160]
    pub heatmap: Tensor<B, 4>,
    /// [batch, 2, 160, 160] — box w,h in input px at the center cell.
    pub sizes: Tensor<B, 4>,
    /// [batch, 2, 160, 160] — sub-cell center offset in [0,1) cell units.
    pub offsets: Tensor<B, 4>,
}

#[derive(Module, Debug)]
pub struct ConvBlock<B: Backend> {
    conv: Conv2d<B>,
    bn: BatchNorm<B>,
    relu: Relu,
}

impl<B: Backend> ConvBlock<B> {
    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        self.relu.forward(self.bn.forward(self.conv.forward(x)))
    }
}

fn conv_block<B: Backend>(
    cin: usize,
    cout: usize,
    stride: usize,
    device: &B::Device,
) -> ConvBlock<B> {
    ConvBlock {
        conv: Conv2dConfig::new([cin, cout], [3, 3])
            .with_stride([stride, stride])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_bias(false)
            .init(device),
        bn: BatchNormConfig::new(cout).init(device),
        relu: Relu,
    }
}

#[derive(Module, Debug)]
pub struct UpBlock<B: Backend> {
    deconv: ConvTranspose2d<B>,
    bn: BatchNorm<B>,
    relu: Relu,
}

impl<B: Backend> UpBlock<B> {
    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        self.relu.forward(self.bn.forward(self.deconv.forward(x)))
    }
}

fn up_block<B: Backend>(cin: usize, cout: usize, device: &B::Device) -> UpBlock<B> {
    UpBlock {
        // k4 s2 p1: exact 2x upsample without checkerboard from odd kernels.
        deconv: ConvTranspose2dConfig::new([cin, cout], [4, 4])
            .with_stride([2, 2])
            .with_padding([1, 1])
            .with_bias(false)
            .init(device),
        bn: BatchNormConfig::new(cout).init(device),
        relu: Relu,
    }
}

#[derive(Module, Debug)]
pub struct Head<B: Backend> {
    conv: ConvBlock<B>,
    out: Conv2d<B>,
}

impl<B: Backend> Head<B> {
    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        self.out.forward(self.conv.forward(x))
    }
}

fn head<B: Backend>(
    channels: usize,
    out: usize,
    bias_prior: Option<f32>,
    device: &B::Device,
) -> Head<B> {
    let mut out_conv = Conv2dConfig::new([channels, out], [1, 1]).init(device);
    if let Some(prior) = bias_prior {
        // CenterNet's focal-prior trick: start the head believing "almost nothing
        // is an object" so the negatives don't drown the first epochs.
        out_conv.bias = Some(burn::module::Param::from_tensor(Tensor::full(
            [out], prior, device,
        )));
    }
    Head {
        conv: conv_block(channels, channels, 1, device),
        out: out_conv,
    }
}

#[derive(Module, Debug)]
pub struct GroundingModel<B: Backend> {
    stem: ConvBlock<B>,
    stage1: ConvBlock<B>,
    down1: ConvBlock<B>,
    stage2: ConvBlock<B>,
    down2: ConvBlock<B>,
    stage3: ConvBlock<B>,
    down3: ConvBlock<B>,
    stage4: ConvBlock<B>,
    up1: UpBlock<B>,
    up2: UpBlock<B>,
    heat: Head<B>,
    size: Head<B>,
    offset: Head<B>,
}

impl<B: Backend> GroundingModel<B> {
    pub fn init(device: &B::Device) -> Self {
        Self {
            stem: conv_block(3, 32, 2, device),      // 320
            stage1: conv_block(32, 32, 1, device),
            down1: conv_block(32, 64, 2, device),    // 160
            stage2: conv_block(64, 64, 1, device),
            down2: conv_block(64, 128, 2, device),   // 80
            stage3: conv_block(128, 128, 1, device),
            down3: conv_block(128, 256, 2, device),  // 40
            stage4: conv_block(256, 256, 1, device),
            up1: up_block(256, 128, device),         // 80
            up2: up_block(128, 64, device),          // 160 = stride 4
            heat: head(64, NUM_CLASSES, Some(-2.19), device),
            size: head(64, 2, None, device),
            offset: head(64, 2, None, device),
        }
    }

    pub fn forward(&self, images: Tensor<B, 4>) -> Detections<B> {
        let x = self.stem.forward(images);
        let x = self.stage1.forward(x);
        let x = self.down1.forward(x);
        let x = self.stage2.forward(x);
        let x = self.down2.forward(x);
        let x = self.stage3.forward(x);
        let x = self.down3.forward(x);
        let x = self.stage4.forward(x);
        let x = self.up1.forward(x);
        let x = self.up2.forward(x);
        Detections {
            heatmap: self.heat.forward(x.clone()),
            sizes: self.size.forward(x.clone()),
            offsets: self.offset.forward(x),
        }
    }
}

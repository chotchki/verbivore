//! The 3.5 contenders. Both share the same conv encoder skeleton; they differ
//! in WHERE the comparison happens: siamese compares learned embeddings,
//! diff-stack lets convs see both frames plus their difference directly.

use burn::nn::conv::{Conv2d, Conv2dConfig};
use burn::nn::{BatchNorm, BatchNormConfig, Linear, LinearConfig, PaddingConfig2d, Relu};
use burn::prelude::*;

#[derive(Module, Debug)]
pub struct ConvBlock<B: Backend> {
    conv: Conv2d<B>,
    bn: BatchNorm<B>,
    relu: Relu,
}

fn conv_block<B: Backend>(cin: usize, cout: usize, device: &B::Device) -> ConvBlock<B> {
    ConvBlock {
        conv: Conv2dConfig::new([cin, cout], [3, 3])
            .with_stride([2, 2])
            .with_padding(PaddingConfig2d::Explicit(1, 1, 1, 1))
            .with_bias(false)
            .init(device),
        bn: BatchNormConfig::new(cout).init(device),
        relu: Relu,
    }
}

impl<B: Backend> ConvBlock<B> {
    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 4> {
        self.relu.forward(self.bn.forward(self.conv.forward(x)))
    }
}

/// 4 stride-2 blocks then global average pool to an embedding.
#[derive(Module, Debug)]
pub struct Encoder<B: Backend> {
    b1: ConvBlock<B>,
    b2: ConvBlock<B>,
    b3: ConvBlock<B>,
    b4: ConvBlock<B>,
}

pub const EMBED: usize = 128;

fn encoder<B: Backend>(cin: usize, device: &B::Device) -> Encoder<B> {
    Encoder {
        b1: conv_block(cin, 16, device),
        b2: conv_block(16, 32, device),
        b3: conv_block(32, 64, device),
        b4: conv_block(64, EMBED, device),
    }
}

impl<B: Backend> Encoder<B> {
    fn forward(&self, x: Tensor<B, 4>) -> Tensor<B, 2> {
        let x = self.b4.forward(self.b3.forward(self.b2.forward(self.b1.forward(x))));
        // Global average pool: [B, EMBED, h, w] -> [B, EMBED]
        let [b, c, h, w] = x.dims();
        x.reshape([b, c, h * w]).mean_dim(2).reshape([b, c])
    }
}

/// Siamese: shared encoder, |embedding difference| -> logit.
#[derive(Module, Debug)]
pub struct SiameseModel<B: Backend> {
    encoder: Encoder<B>,
    head: Linear<B>,
}

impl<B: Backend> SiameseModel<B> {
    pub fn init(device: &B::Device) -> Self {
        Self {
            encoder: encoder(3, device),
            head: LinearConfig::new(EMBED, 1).init(device),
        }
    }

    /// Logits [B,1]: positive = changed.
    pub fn forward(&self, before: Tensor<B, 4>, after: Tensor<B, 4>) -> Tensor<B, 2> {
        let a = self.encoder.forward(before);
        let b = self.encoder.forward(after);
        self.head.forward((a - b).abs())
    }
}

/// Diff-stack: [before; after; |diff|] as 9 channels through one encoder.
#[derive(Module, Debug)]
pub struct DiffStackModel<B: Backend> {
    encoder: Encoder<B>,
    head: Linear<B>,
}

impl<B: Backend> DiffStackModel<B> {
    pub fn init(device: &B::Device) -> Self {
        Self {
            encoder: encoder(9, device),
            head: LinearConfig::new(EMBED, 1).init(device),
        }
    }

    pub fn forward(&self, before: Tensor<B, 4>, after: Tensor<B, 4>) -> Tensor<B, 2> {
        let diff = (before.clone() - after.clone()).abs();
        let stacked = Tensor::cat(vec![before, after, diff], 1);
        self.head.forward(self.encoder.forward(stacked))
    }
}

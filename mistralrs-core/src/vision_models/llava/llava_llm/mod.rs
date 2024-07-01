#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
use candle_core::{DType, Device, Result, Tensor};

use crate::pipeline::{Cache, IsqModel};

pub(crate) trait LLaVALLM: IsqModel + Sync + Send {
    //Normal model without anymoe, but add embed and forward_input_embed. This is only a temporary solution. Finally when the rope problem solved for normal LLM models, we should refactor this.
    fn embed(&self, input_ids: &Tensor) -> Result<Tensor>;
    fn forward_input_embed(
        &self,
        input_ids: &Tensor,  // only for masking
        input_embed: Tensor, // we don't want to clone, so we pass it in
        seqlen_offsets: &[usize],
        start_offsets_kernel: Tensor,
        context_lens: Vec<(usize, usize)>,
    ) -> Result<Tensor>;
    fn forward(
        &self,
        input_ids: &Tensor,
        seqlen_offsets: &[usize],
        start_offsets_kernel: Tensor,
        context_lens: Vec<(usize, usize)>,
        position_ids: Vec<usize>,
    ) -> candle_core::Result<Tensor>;
    fn cache(&self) -> &Cache;
}

#[derive(Debug)]
pub(crate) struct OrdinaryRoPE;

impl OrdinaryRoPE {
    fn create_parameters(
        n_elem: usize,
        max_seq_len: usize,
        rope_theta: f32,
        dtype: DType,
        device: &Device,
    ) -> Result<(Tensor, Tensor)> {
        let theta: Vec<_> = (0..n_elem)
            .step_by(2)
            .map(|i| 1f32 / rope_theta.powf(i as f32 / n_elem as f32))
            .collect();
        let theta = Tensor::new(theta.as_slice(), device)?;
        let idx_theta = Tensor::arange(0, max_seq_len as u32, device)?
            .to_dtype(DType::F32)?
            .reshape((max_seq_len, 1))?
            .matmul(&theta.reshape((1, theta.elem_count()))?)?;
        let cos = idx_theta.cos()?.to_dtype(dtype)?;
        let sin = idx_theta.sin()?.to_dtype(dtype)?;
        Result::Ok((cos, sin))
    }
    fn forward(x: &Tensor, index_pos: usize, cos: &Tensor, sin: &Tensor) -> Result<Tensor> {
        let (_b_sz, _, seq_len, _hidden_size) = x.dims4()?;
        let cos = cos.narrow(0, index_pos, seq_len)?;
        let sin = sin.narrow(0, index_pos, seq_len)?;
        candle_nn::rotary_emb::rope(x, &cos, &sin)
    }
}
pub(crate) mod llama;
pub(crate) mod mistral;

pub use llama::Llama;
pub use mistral::Model as Mistral;

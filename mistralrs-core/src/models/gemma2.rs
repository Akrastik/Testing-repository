#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]

use std::sync::Arc;

use candle_core::{quantized::QMatMul, DType, Device, Module, Result, Tensor, D};
use candle_nn::{linear_b as linear, Activation, RotaryEmbedding, VarBuilder};

use crate::{
    device_map::DeviceMapper,
    layers::{repeat_kv, CausalMasker, MatMul, QLinear},
    pipeline::{extract_logits, Cache, IsqModel, NormalLoadingMetadata, NormalModel},
    utils::progress::NiceProgressBar,
};

fn default_max_position_embeddings() -> usize {
    4096
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct Config {
    pub attention_bias: bool,
    pub head_dim: usize,
    // The code gemma configs include both hidden_act and hidden_activation.
    pub hidden_act: Option<Activation>,
    pub hidden_activation: Option<Activation>,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_attention_heads: usize,
    pub num_hidden_layers: usize,
    pub num_key_value_heads: usize,
    pub rms_norm_eps: f64,
    pub rope_theta: f64,
    pub vocab_size: usize,
    pub sliding_window: usize,
    pub attn_logit_softcapping: Option<f64>,
    pub final_logit_softcapping: Option<f64>,
    pub query_pre_attn_scalar: usize,

    #[serde(default = "default_max_position_embeddings")]
    pub max_position_embeddings: usize,
}

impl Config {
    pub fn hidden_act(&self) -> Result<Activation> {
        match (self.hidden_act, self.hidden_activation) {
            (None, Some(act)) | (Some(act), None) => Ok(act),
            (Some(act), Some(_)) => {
                // If both are set just use hidden_act
                Ok(act)
            }
            (None, None) => candle_core::bail!("none of hidden_act and hidden_activation are set"),
        }
    }
}

#[derive(Debug, Clone)]
struct RmsNorm {
    weight: Tensor,
    eps: f64,
}

impl RmsNorm {
    fn new(dim: usize, eps: f64, vb: VarBuilder) -> Result<Self> {
        let weight = vb.get(dim, "weight")?;
        Ok(Self { weight, eps })
    }
}

impl Module for RmsNorm {
    fn forward(&self, x: &Tensor) -> Result<Tensor> {
        let x_dtype = x.dtype();
        let internal_dtype = match x_dtype {
            DType::F16 | DType::BF16 => DType::F32,
            d => d,
        };
        let hidden_size = x.dim(D::Minus1)?;
        let x = x.to_dtype(internal_dtype)?;
        let norm_x = (x.sqr()?.sum_keepdim(D::Minus1)? / hidden_size as f64)?;
        let x_normed = x.broadcast_div(&(norm_x + self.eps)?.sqrt()?)?;
        x_normed
            .to_dtype(x_dtype)?
            .broadcast_mul(&(&self.weight + 1.0)?)
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::upper_case_acronyms)]
struct MLP {
    gate_proj: QLinear,
    up_proj: QLinear,
    down_proj: QLinear,
    act_fn: candle_nn::Activation,
}

impl MLP {
    fn new(cfg: &Config, vb: VarBuilder) -> Result<Self> {
        let hidden_sz = cfg.hidden_size;
        let intermediate_sz = cfg.intermediate_size;
        let gate_proj = linear(hidden_sz, intermediate_sz, false, vb.pp("gate_proj"))?;
        let up_proj = linear(hidden_sz, intermediate_sz, false, vb.pp("up_proj"))?;
        let down_proj = linear(intermediate_sz, hidden_sz, false, vb.pp("down_proj"))?;
        Ok(Self {
            gate_proj: QLinear::from_linear(gate_proj),
            up_proj: QLinear::from_linear(up_proj),
            down_proj: QLinear::from_linear(down_proj),
            act_fn: cfg.hidden_act()?,
        })
    }
}

impl Module for MLP {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let original_dtype = xs.dtype();
        let mut xs = xs.clone();
        if self.gate_proj.is_quant() {
            xs = xs.to_dtype(DType::F32)?;
        }
        let lhs = xs.apply(&self.gate_proj)?.apply(&self.act_fn)?;
        let rhs = xs.apply(&self.up_proj)?;
        let mut res = (lhs * rhs)?.apply(&self.down_proj)?;
        if self.gate_proj.is_quant() {
            res = res.to_dtype(original_dtype)?;
        }
        Ok(res)
    }
}

#[derive(Debug, Clone)]
struct Attention {
    q_proj: QLinear,
    k_proj: QLinear,
    v_proj: QLinear,
    o_proj: QLinear,
    num_heads: usize,
    num_kv_heads: usize,
    num_kv_groups: usize,
    head_dim: usize,
    rotary_emb: Arc<RotaryEmbedding>,
    query_pre_attn_scalar: usize,
    attn_logit_softcapping: Option<f64>,
    use_sliding_window: bool,
    sliding_window: Option<usize>,
}

impl Attention {
    fn new(
        rotary_emb: Arc<RotaryEmbedding>,
        cfg: &Config,
        layer_idx: usize,
        vb: VarBuilder,
    ) -> Result<Self> {
        let hidden_sz = cfg.hidden_size;
        let num_heads = cfg.num_attention_heads;
        let num_kv_heads = cfg.num_key_value_heads;
        let num_kv_groups = num_heads / num_kv_heads;
        let head_dim = cfg.head_dim;
        let bias = cfg.attention_bias;
        let q_proj = linear(hidden_sz, num_heads * head_dim, bias, vb.pp("q_proj"))?;
        let k_proj = linear(hidden_sz, num_kv_heads * head_dim, bias, vb.pp("k_proj"))?;
        let v_proj = linear(hidden_sz, num_kv_heads * head_dim, bias, vb.pp("v_proj"))?;
        let o_proj = linear(num_heads * head_dim, hidden_sz, bias, vb.pp("o_proj"))?;
        Ok(Self {
            q_proj: QLinear::from_linear(q_proj),
            k_proj: QLinear::from_linear(k_proj),
            v_proj: QLinear::from_linear(v_proj),
            o_proj: QLinear::from_linear(o_proj),
            num_heads,
            num_kv_heads,
            num_kv_groups,
            head_dim,
            rotary_emb,
            query_pre_attn_scalar: cfg.query_pre_attn_scalar,
            attn_logit_softcapping: cfg.attn_logit_softcapping,
            use_sliding_window: layer_idx % 2 != 0,
            sliding_window: if layer_idx % 2 != 0 {
                Some(cfg.sliding_window)
            } else {
                None
            },
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        attention_mask: Option<&Tensor>,
        sliding_attention_mask: Option<&Tensor>,
        seqlen_offsets: &[usize],
        start_offsets_kernel: Tensor,
        kv_cache: &mut Option<(Tensor, Tensor)>,
    ) -> Result<Tensor> {
        let (b_sz, q_len, _) = xs.dims3()?;

        let original_dtype = xs.dtype();
        let mut xs = xs.clone();
        if self.q_proj.is_quant() {
            xs = xs.to_dtype(DType::F32)?;
        }
        let mut q = self.q_proj.forward(&xs)?;
        let mut k = self.k_proj.forward(&xs)?;
        let mut v = self.v_proj.forward(&xs)?;
        if self.q_proj.is_quant() {
            q = q.to_dtype(original_dtype)?;
            k = k.to_dtype(original_dtype)?;
            v = v.to_dtype(original_dtype)?;
        }

        let mut q = q.reshape((b_sz * q_len, self.num_heads, self.head_dim))?;
        let mut k = k.reshape((b_sz * q_len, self.num_kv_heads, self.head_dim))?;
        let v = if q_len != 1 {
            v.reshape((b_sz, q_len, self.num_kv_heads, self.head_dim))?
                .transpose(1, 2)?
        } else {
            // Optimization for seqlen = 1, avoid transpose and just modify reshape dims
            v.reshape((b_sz, self.num_kv_heads, q_len, self.head_dim))?
        };

        self.rotary_emb
            .forward(seqlen_offsets, &start_offsets_kernel, &mut q, &mut k, b_sz)?;

        if q.rank() == 3 && q_len != 1 {
            q = q
                .reshape((b_sz, q_len, self.num_heads, self.head_dim))?
                .transpose(1, 2)?
                .contiguous()?;
            k = k
                .reshape((b_sz, q_len, self.num_kv_heads, self.head_dim))?
                .transpose(1, 2)?
                .contiguous()?;
        } else if q.rank() == 3 {
            // Optimization for seqlen = 1, avoid transpose and just modify reshape dims
            q = q
                .reshape((b_sz, self.num_heads, q_len, self.head_dim))?
                .contiguous()?;
            k = k
                .reshape((b_sz, self.num_kv_heads, q_len, self.head_dim))?
                .contiguous()?;
        }

        let mask = if self.use_sliding_window {
            sliding_attention_mask
        } else {
            attention_mask
        };

        // self.sliding_window is None if !self.use_sliding_window
        let (k, v, mask) = Cache::update_kv_cache_sliding_window(
            kv_cache,
            k,
            v,
            mask,
            self.sliding_window,
            false,
        )?;

        let k = repeat_kv(k, self.num_kv_groups)?.contiguous()?;
        let v = repeat_kv(v, self.num_kv_groups)?.contiguous()?;

        let mut att = MatMul.matmul_affine_div(
            &q.contiguous()?,
            &k.t()?.contiguous()?,
            (self.query_pre_attn_scalar as f64).sqrt(),
        )?;

        if let Some(attn_logit_softcapping) = self.attn_logit_softcapping {
            att = (att / attn_logit_softcapping)?;
            att = att.tanh()?;
            att = (att * attn_logit_softcapping)?;
        }

        let att = match mask {
            Some(m) => att.broadcast_add(&m)?,
            None => att,
        };
        let att = candle_nn::ops::softmax_last_dim(&att)?;
        // Convert to contiguous as matmul doesn't support strided vs for now.
        let mut attn_output = MatMul.matmul(&att, &v.contiguous()?)?;

        if self.q_proj.is_quant() {
            attn_output = attn_output.to_dtype(DType::F32)?;
        }
        let mut res = attn_output
            .transpose(1, 2)?
            .reshape((b_sz, q_len, ()))?
            .apply(&self.o_proj)?;
        if self.q_proj.is_quant() {
            res = res.to_dtype(original_dtype)?;
        }
        Ok(res)
    }
}

#[derive(Debug, Clone)]
struct DecoderLayer {
    self_attn: Attention,
    mlp: MLP,
    input_layernorm: RmsNorm,
    post_attention_layernorm: RmsNorm,
    pre_feedforward_layernorm: RmsNorm,
    post_feedforward_layernorm: RmsNorm,
}

impl DecoderLayer {
    fn new(
        rotary_emb: Arc<RotaryEmbedding>,
        cfg: &Config,
        vb: VarBuilder,
        mapper: &dyn DeviceMapper,
        layer_idx: usize,
        loading_isq: bool,
    ) -> Result<Self> {
        let self_attn = Attention::new(
            rotary_emb,
            cfg,
            layer_idx,
            mapper.set_device(layer_idx, vb.pp("self_attn"), loading_isq),
        )?;
        let mlp = MLP::new(cfg, mapper.set_device(layer_idx, vb.pp("mlp"), loading_isq))?;
        let input_layernorm = RmsNorm::new(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            mapper.set_device(layer_idx, vb.pp("input_layernorm"), false),
        )?;
        let post_attention_layernorm = RmsNorm::new(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            mapper.set_device(layer_idx, vb.pp("post_attention_layernorm"), false),
        )?;
        let pre_feedforward_layernorm = RmsNorm::new(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            mapper.set_device(layer_idx, vb.pp("pre_feedforward_layernorm"), false),
        )?;
        let post_feedforward_layernorm = RmsNorm::new(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            mapper.set_device(layer_idx, vb.pp("post_feedforward_layernorm"), false),
        )?;
        Ok(Self {
            self_attn,
            mlp,
            input_layernorm,
            post_attention_layernorm,
            pre_feedforward_layernorm,
            post_feedforward_layernorm,
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        attention_mask: Option<&Tensor>,
        sliding_attention_mask: Option<&Tensor>,
        seqlen_offsets: &[usize],
        start_offsets_kernel: Tensor,
        kv_cache: &mut Option<(Tensor, Tensor)>,
    ) -> Result<Tensor> {
        let residual = xs;
        let xs = self.input_layernorm.forward(xs)?;
        let xs = self
            .self_attn
            .forward(
                &xs,
                attention_mask,
                sliding_attention_mask,
                seqlen_offsets,
                start_offsets_kernel,
                kv_cache,
            )?
            .apply(&self.post_attention_layernorm)?;
        let xs = (xs + residual)?;
        let residual = &xs;
        let xs = xs
            .apply(&self.pre_feedforward_layernorm)?
            .apply(&self.mlp)?
            .apply(&self.post_feedforward_layernorm)?;
        residual + xs
    }
}

#[derive(Debug)]
pub struct Model {
    embed_tokens: candle_nn::Embedding,
    layers: Vec<DecoderLayer>,
    norm: RmsNorm,
    lm_head: QMatMul,
    hidden_size: usize,
    pub device: Device,
    pub cache: Cache,
    pub max_seq_len: usize,
    mapper: Box<dyn DeviceMapper + Send + Sync>,
    sliding_window: usize,
    pub final_logit_softcapping: Option<f64>,
}

impl Model {
    pub fn new(
        cfg: &Config,
        vb: VarBuilder,
        is_gptx: bool,
        normal_loading_metadata: NormalLoadingMetadata,
    ) -> Result<Self> {
        let mapper = normal_loading_metadata
            .mapper
            .into_mapper(cfg.num_hidden_layers, &normal_loading_metadata.real_device)?;
        let vb = vb.set_dtype(mapper.get_min_dtype()?);

        let vb_m = vb.pp("model");
        let embed_tokens = candle_nn::embedding(
            cfg.vocab_size,
            cfg.hidden_size,
            mapper.set_nm_device(vb_m.pp("embed_tokens"), false),
        )?;
        let mut layers = Vec::with_capacity(cfg.num_hidden_layers);
        let vb_l = vb_m.pp("layers");
        for layer_idx in NiceProgressBar(0..cfg.num_hidden_layers, "Loading repeating layers") {
            let rotary_emb = Arc::new(RotaryEmbedding::new(
                cfg.rope_theta as f32,
                cfg.head_dim,
                cfg.max_position_embeddings,
                mapper
                    .device_for(layer_idx, false)
                    .unwrap_or(&normal_loading_metadata.real_device),
                is_gptx,
                vb.dtype(),
            )?);
            let layer = DecoderLayer::new(
                rotary_emb.clone(),
                cfg,
                vb_l.pp(layer_idx),
                &*mapper,
                layer_idx,
                normal_loading_metadata.loading_isq,
            )?;
            layers.push(layer)
        }
        let norm = RmsNorm::new(
            cfg.hidden_size,
            cfg.rms_norm_eps,
            mapper.set_nm_device(vb_m.pp("norm"), false),
        )?;
        let lm_head = QMatMul::Tensor(embed_tokens.embeddings().clone());
        Ok(Self {
            embed_tokens,
            layers,
            norm,
            lm_head,
            device: normal_loading_metadata.real_device,
            hidden_size: cfg.hidden_size,
            cache: Cache::new(cfg.num_hidden_layers, false),
            max_seq_len: default_max_position_embeddings(),
            mapper,
            sliding_window: cfg.sliding_window,
            final_logit_softcapping: cfg.final_logit_softcapping,
        })
    }

    pub fn forward(
        &self,
        input_ids: &Tensor,
        seqlen_offsets: &[usize],
        start_offsets_kernel: Tensor,
        context_lens: Vec<(usize, usize)>,
    ) -> Result<Tensor> {
        let xs = self.embed_tokens.forward(input_ids)?;
        let mut xs = (xs * (self.hidden_size as f64).sqrt())?;
        let mut cache = self.cache.lock();
        let attention_mask = CausalMasker.make_causal_mask_as_attn_bias(
            input_ids,
            &cache,
            xs.dtype(),
            self.layers[0].self_attn.num_heads,
        )?;
        let sliding_attention_mask = CausalMasker
            .make_causal_mask_with_sliding_window_as_attn_bias(
                input_ids,
                &cache,
                Some(self.sliding_window),
                xs.dtype(),
                self.layers[0].self_attn.num_heads,
            )?;
        for (i, layer) in self.layers.iter().enumerate() {
            xs = self.mapper.map(xs, i)?;
            xs = layer.forward(
                &xs,
                attention_mask
                    .as_ref()
                    .map(|m| m.to_device(xs.device()).unwrap())
                    .as_ref(),
                sliding_attention_mask
                    .as_ref()
                    .map(|m| m.to_device(xs.device()).unwrap())
                    .as_ref(),
                seqlen_offsets,
                start_offsets_kernel.clone(),
                &mut cache[i],
            )?;
        }
        let xs = xs.to_device(&self.device)?;
        let mut xs = xs.apply(&self.norm)?;
        if matches!(self.lm_head, QMatMul::QTensor(_)) {
            xs = xs.to_dtype(DType::F32)?;
        }

        let mut xs = MatMul.qmatmul(&xs, &self.lm_head)?;

        if let Some(final_logit_softcapping) = self.final_logit_softcapping {
            xs = (xs / final_logit_softcapping)?;
            xs = xs.tanh()?;
            xs = (xs * final_logit_softcapping)?;
        }

        extract_logits(&xs, context_lens)
    }
}

impl IsqModel for Model {
    fn get_tensors(&mut self) -> (Vec<(&mut QMatMul, Option<usize>)>, &dyn DeviceMapper) {
        let mut tensors = Vec::new();
        tensors.push((&mut self.lm_head, None));
        for (i, layer) in self.layers.iter_mut().enumerate() {
            tensors.push((layer.self_attn.q_proj.inner(), Some(i)));
            tensors.push((layer.self_attn.k_proj.inner(), Some(i)));
            tensors.push((layer.self_attn.v_proj.inner(), Some(i)));
            tensors.push((layer.self_attn.o_proj.inner(), Some(i)));
            tensors.push((layer.mlp.down_proj.inner(), Some(i)));
            tensors.push((layer.mlp.gate_proj.inner(), Some(i)));
            tensors.push((layer.mlp.up_proj.inner(), Some(i)));
        }
        (tensors, &*self.mapper)
    }
}

impl NormalModel for Model {
    fn forward(
        &self,
        input_ids: &Tensor,
        seqlen_offsets: &[usize],
        start_offsets_kernel: Tensor,
        context_lens: Vec<(usize, usize)>,
        _position_ids: Vec<usize>,
    ) -> Result<Tensor> {
        self.forward(
            input_ids,
            seqlen_offsets,
            start_offsets_kernel,
            context_lens,
        )
    }
    fn xlora_forward(
        &self,
        _input_ids: &Tensor,
        _input_ids_full: &Tensor,
        _seqlen_offsets: &[usize],
        _seqlen_offsets_full: &[usize],
        _start_offsets_kernel: Tensor,
        _start_offsets_kernel_full: Tensor,
        _no_kv_cache: bool,
        _non_granular_state: &Option<crate::xlora_models::NonGranularState>,
        _context_lens: Vec<(usize, usize)>,
        _position_ids: Vec<usize>,
    ) -> Result<Tensor> {
        unimplemented!()
    }
    fn cache(&self) -> &Cache {
        &self.cache
    }
    fn device(&self) -> &Device {
        &self.device
    }
    fn is_xlora(&self) -> bool {
        false
    }
    fn max_seq_len(&self) -> usize {
        self.max_seq_len
    }
}

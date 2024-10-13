use candle_core::{DType, Device, Result, Tensor};

use mistralrs_paged_attn::{paged_attention, reshape_and_cache};
use mistralrs_quant::{FP8Linear, FP8QuantizationResult};

use crate::{
    paged_attention::{KVCacheType, PagedAttentionKVCache},
    pipeline::text_models_inputs_processor::PagedAttentionInputMetadata,
};

const _PARTITION_SIZE: usize = 512;

#[allow(dead_code)]
pub struct PagedAttention {
    num_attention_heads: usize,
    head_dim: usize,
    num_key_value_heads: usize,
    scale: f32,
    sliding_window: Option<usize>,
    num_queries_per_kv: usize,
    alibi_slopes: Option<Tensor>,
    cache_dtype: KVCacheType,
    dummy_scale: Tensor,
}

impl PagedAttention {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        num_attention_heads: usize,
        head_dim: usize,
        scale: f32,
        num_key_value_heads: Option<usize>,
        sliding_window: Option<usize>,
        device: &Device,
        alibi_slopes: Option<Vec<f64>>,
        cache_dtype: KVCacheType,
    ) -> Result<Self> {
        let num_key_value_heads = num_key_value_heads.unwrap_or(num_attention_heads);
        let num_queries_per_kv = num_attention_heads / num_key_value_heads;
        let alibi_slopes = if let Some(alibi_slopes) = alibi_slopes {
            Some(Tensor::new(alibi_slopes, device)?)
        } else {
            None
        };
        Ok(Self {
            num_attention_heads,
            head_dim,
            num_key_value_heads,
            scale,
            sliding_window,
            num_queries_per_kv,
            alibi_slopes,
            cache_dtype,
            dummy_scale: Tensor::new(1f32, device)?,
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(unused_variables)]
    /// query: shape = [batch_size, seq_len, num_heads * head_size]
    /// key: shape = [batch_size, seq_len, num_kv_heads * head_size]
    /// value: shape = [batch_size, num_kv_heads * head_size]
    /// key_cache: shape = [num_blocks, num_kv_heads, head_size/x,
    ///     block_size, x]
    /// value_cache: shape = [num_blocks, num_kv_heads, head_size,
    ///     block_size]
    /// input_metadata: metadata for paged attention.
    pub fn forward(
        &self,
        query: &Tensor,
        key: &Tensor,
        value: &Tensor,
        attention_mask: Option<&Tensor>,
        mut kv_cache: Option<PagedAttentionKVCache>,
        input_metadata: &mut PagedAttentionInputMetadata,
        softcapping: Option<f64>,
    ) -> Result<Tensor> {
        let dims = input_metadata.slot_mappings.dims();
        let slot_mapping = if dims.len() > 1 {
            input_metadata
                .slot_mappings
                .flatten(0, input_metadata.slot_mappings.dims().len())?
        } else {
            input_metadata.slot_mappings.clone()
        };

        let (batch_size, attention_heads, seq_len, head_size) = query.shape().dims4()?;
        let (_, key_value_heads, _, _) = key.shape().dims4()?;

        let att = match attention_mask {
            None => None,
            Some(mask) => {
                //Only perform key/value repeat in prefiling stage, this will reduce kvcache
                //and remove redundant repeat_kv in decoding stage
                let att = if key_value_heads != attention_heads {
                    let key_repeat = if key_value_heads == 1 {
                        key.broadcast_as((batch_size, attention_heads, seq_len, head_size))?
                    } else {
                        Tensor::cat(&vec![&key; attention_heads / key_value_heads], 2)?
                            .reshape((batch_size, attention_heads, seq_len, head_size))?
                    };
                    (query.matmul(&key_repeat.t()?.contiguous()?)? * self.scale as f64)?
                } else {
                    (query.matmul(&key.t()?)? * self.scale as f64)?
                };
                let att = match softcapping {
                    None => att,
                    Some(sc) => ((att / sc)?.tanh()? * sc)?,
                };

                let att = att.broadcast_add(mask)?;
                let att = candle_nn::ops::softmax_last_dim(&att)?;
                if key_value_heads != attention_heads {
                    let value_repeat = if key_value_heads == 1 {
                        value.broadcast_as((batch_size, attention_heads, seq_len, head_size))?
                    } else {
                        Tensor::cat(&vec![&value; attention_heads / key_value_heads], 2)?
                            .reshape((batch_size, attention_heads, seq_len, head_size))?
                    };
                    Some(att.matmul(&value_repeat.contiguous()?)?)
                } else {
                    Some(att.matmul(&value.contiguous()?).unwrap())
                }
            }
        };

        // // paged-attn expects [batch_size, num_tokens, num_heads, head_size]
        let (query, key, value) = if seq_len > 1 {
            let q = query
                .transpose(1, 2)?
                .reshape(((), attention_heads, head_size))?;
            let k = key
                .transpose(1, 2)?
                .reshape(((), key_value_heads, head_size))?;
            let v = value
                .transpose(1, 2)?
                .reshape(((), key_value_heads, head_size))?;
            (q, k, v)
        } else {
            //avoid unnecessary transpose for decoding
            let q = query.reshape(((), attention_heads, head_size))?;
            let k = key.reshape(((), key_value_heads, head_size))?;
            let v = value.reshape(((), key_value_heads, head_size))?;
            (q, k, v)
        };

        // let (key, value, key_scale, value_scale) = match self.cache_dtype {
        //     KVCacheType::F8E4M3 => {
        //         let FP8QuantizationResult {
        //             qw: key,
        //             quantize_scale: key_scale,
        //             dequantize_scale,
        //         } = FP8Linear::quantize(&key, DType::F8E4M3)?;
        //         let FP8QuantizationResult {
        //             qw: value,
        //             quantize_scale: value_scale,
        //             dequantize_scale,
        //         } = FP8Linear::quantize(&value, DType::F8E4M3)?;
        //         (key, value, key_scale, value_scale)
        //     }
        //     KVCacheType::FullPrecision => (
        //         key,
        //         value,
        //         self.dummy_scale.clone(),
        //         self.dummy_scale.clone(),
        //     ),
        // };

        let (key_scale, value_scale) = match self.cache_dtype {
            KVCacheType::F8E4M3 => {
                let FP8QuantizationResult {
                    qw: _key,
                    quantize_scale: _,
                    dequantize_scale: key_scale,
                } = FP8Linear::quantize(&key, DType::F8E4M3)?;
                let FP8QuantizationResult {
                    qw: _value,
                    quantize_scale,
                    dequantize_scale: value_scale,
                } = FP8Linear::quantize(&value, DType::F8E4M3)?;
                (key_scale, value_scale)
            }
            KVCacheType::FullPrecision => (self.dummy_scale.clone(), self.dummy_scale.clone()),
        };

        // key: Tensor,              // [num_tokens, num_heads, head_size]
        // value: Tensor,            // [num_tokens, num_heads, head_size]
        // key_cache: &mut Tensor,   // [num_blocks, num_heads, head_size/x, block_size, x] 48,32,16,16,8
        // value_cache: &mut Tensor, // [num_blocks, num_heads, head_size, block_size] 48,32,128,16
        // slot_mapping: Tensor,     // [num_tokens]
        if let Some(PagedAttentionKVCache { k_cache, v_cache }) = &mut kv_cache {
            reshape_and_cache(
                &key,
                &value,
                k_cache,
                v_cache,
                &slot_mapping,
                &key_scale,
                &value_scale,
            )?;
        }

        if let Some(att) = att {
            // Return result in prefill
            return Ok(att);
        }

        let Some(PagedAttentionKVCache { k_cache, v_cache }) = &mut kv_cache else {
            unreachable!()
        };

        //  Args:
        //  output: shape = [num_generation_tokens, num_heads, head_size]
        //
        //  query: shape = [num_generation_tokens, num_heads, head_size]
        //
        //  key_cache: shape = [num_blocks, num_kv_heads, head_size/x,
        //      block_size, x]
        //
        //  value_cache: shape = [num_blocks, num_kv_heads, head_size,
        //      block_size]
        //
        //  input_metadata: metadata for paged attention.
        //
        //  alibi_slopes: shape = [num_heads]
        #[allow(clippy::cast_possible_truncation)]
        paged_attention(
            &query,
            k_cache,
            v_cache,
            input_metadata.block_tables.as_ref().unwrap(),
            input_metadata.context_lens.as_ref().unwrap(),
            input_metadata.max_context_len.unwrap(),
            self.scale,
            softcapping.unwrap_or(1.0f64) as f32,
            &key_scale,
            &value_scale,
        )
    }
}

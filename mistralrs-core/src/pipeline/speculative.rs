use std::{
    iter::zip,
    sync::{Arc, Mutex},
};

use candle_core::{quantized::GgmlDType, Device, Result, Tensor};
use rand::Rng;
use rand_isaac::Isaac64Rng;
use tokenizers::Tokenizer;

use crate::{
    finish_and_add_tokens_to_seq, get_mut_arcmutex,
    models::Cache,
    pipeline::sampling::{sample_sequence, sample_target_sequence_speculative},
    prefix_cacher::PrefixCacheManager,
    sequence::{Sequence, SequenceRecognizer},
    DeviceMapMetadata, Loader, ModelKind, Pipeline, TokenSource,
};

use super::{
    cache_manager::DefaultCacheManager, calculate_inputs, chat_template::ChatTemplate,
    sampling::SpeculativeSample, CacheInstruction, CacheManager, GeneralMetadata, ModelInputs,
};

pub struct SpeculativeLoader {
    pub target: Box<dyn Loader>,
    pub draft: Box<dyn Loader>,
    pub config: SpeculativeConfig,
}

impl Loader for SpeculativeLoader {
    fn load_model(
        &self,
        revision: Option<String>,
        token_source: TokenSource,
        dtype: Option<candle_core::DType>,
        device: &Device,
        silent: bool,
        mapper: DeviceMapMetadata,
        in_situ_quant: Option<GgmlDType>,
    ) -> anyhow::Result<Arc<tokio::sync::Mutex<dyn Pipeline + Send + Sync>>> {
        let target = self.target.load_model(
            revision.clone(),
            token_source.clone(),
            dtype,
            device,
            silent,
            mapper.clone(),
            in_situ_quant,
        )?;
        let draft = self.draft.load_model(
            revision,
            token_source,
            dtype,
            device,
            silent,
            mapper,
            in_situ_quant,
        )?;
        Ok(Arc::new(tokio::sync::Mutex::new(SpeculativePipeline::new(
            target,
            draft,
            self.config,
        )?)))
    }
    fn get_id(&self) -> String {
        format!(
            "Speculative: tgt = `{}`, draft = `{}`, gamma = `{}`",
            self.target.get_id(),
            self.draft.get_id(),
            self.config.gamma,
        )
    }
    fn get_kind(&self) -> ModelKind {
        ModelKind::Speculative {
            target: Box::new(self.target.get_kind()),
            draft: Box::new(self.draft.get_kind()),
        }
    }
}

/// Speculative decoding pipeline: https://arxiv.org/pdf/2211.17192
///
/// # Algorithm
/// Given draft model q and target model p with probability distributions \
/// q_i(x) and p_i(x) for each token
///
/// - Keep the sample for token i if q_i(x) <= p_i(x)
///     - This means the target model agrees
/// - Else (q_i(x) > p_i(x)) accept that token with prob p_i(x)/q_i(x)
///     - If rejected, sample token from from p'_i(x) = norm(max(0, p(x) − q(x))) and do not take any more'
///
pub struct SpeculativePipeline {
    target: Arc<tokio::sync::Mutex<dyn Pipeline>>,
    draft: Arc<tokio::sync::Mutex<dyn Pipeline>>,
    gamma: usize,
    metadata: GeneralMetadata,
}

#[derive(Copy, Clone)]
pub struct SpeculativeConfig {
    /// γ completions to run of the draft model
    pub gamma: usize,
}

impl SpeculativePipeline {
    pub fn new(
        target: Arc<tokio::sync::Mutex<dyn Pipeline>>,
        draft: Arc<tokio::sync::Mutex<dyn Pipeline>>,
        config: SpeculativeConfig,
    ) -> Result<Self> {
        if get_mut_arcmutex!(target).tokenizer().get_vocab(true)
            != get_mut_arcmutex!(draft).tokenizer().get_vocab(true)
        {
            candle_core::bail!("Target and draft models' tokenzier vocab do not match. This is required for speculative decoding.");
        }
        let metadata = get_mut_arcmutex!(target).get_metadata().clone();
        // todo: some checks or relaxation here?
        Ok(Self {
            target,
            draft,
            gamma: config.gamma,
            metadata,
        })
    }
}

#[async_trait::async_trait]
impl Pipeline for SpeculativePipeline {
    async fn step(
        &mut self,
        input_seqs: &mut [&mut Sequence],
        is_prompt: bool,
        prefix_cacher: &mut PrefixCacheManager,
        disable_eos_stop: bool,
        rng: Arc<Mutex<Isaac64Rng>>,
        pre_op: CacheInstruction,
        post_op: CacheInstruction,
    ) -> Result<()> {
        let n_seqs = input_seqs.len();

        match pre_op {
            CacheInstruction::In => self.clone_in_cache(input_seqs),
            CacheInstruction::Nonthing => (),
            CacheInstruction::Reset { reset_non_granular } => {
                self.set_none_cache(reset_non_granular)
            }
            _ => unreachable!("Unreachable pre cache op."),
        }

        assert_eq!(input_seqs.len(), 1);

        let seq = &mut input_seqs[0];

        // ======================= Run draft model gamma times producing tokens ============================
        // ======================= Sample the `gamma` logits. ============================
        let mut draft_samples = Vec::new();
        let repeat_last_n = get_mut_arcmutex!(self.draft).get_metadata().repeat_last_n;
        for i in 0..self.gamma {
            let is_xlora = get_mut_arcmutex!(self.draft).get_metadata().is_xlora;
            let device = get_mut_arcmutex!(self.draft).device();
            let has_no_kv_cache = get_mut_arcmutex!(self.draft).get_metadata().has_no_kv_cache;
            let inputs = calculate_inputs(
                &[seq],
                i == 0, // Only prompt (no kv cache) if first
                is_xlora,
                &device,
                has_no_kv_cache,
                None,
            )
            .unwrap();
            let logits = get_mut_arcmutex!(self.draft).forward_inputs(inputs)?;

            let sample = sample_sequence(
                logits.clone(),
                seq,
                seq.return_logprobs(),
                repeat_last_n,
                get_mut_arcmutex!(self.draft)
                    .get_metadata()
                    .tok_trie
                    .clone(),
                rng.clone(),
                false, // todo tune
                false, // do not add to tok trie yet
            )
            .await?;
            seq.add_tmp_tok(sample.token);
            draft_samples.push(SpeculativeSample {
                sample,
                distribution: logits.clone(),
            });
        }
        seq.remove_tmp_tok(self.gamma);

        // ======================= Reset the draft. ============================
        get_mut_arcmutex!(self.draft).set_none_cache(true);

        // ======================= Add all draft tokens but the last one. Add the last from the seq. ============================
        let mut draft_prefill_tokens = if is_prompt {
            seq.get_toks().to_vec()
        } else {
            vec![*seq.get_toks().last().unwrap()]
        };
        for (i, sample) in draft_samples.iter().enumerate() {
            if i == draft_samples.len() - 1 {
                continue;
            }
            draft_prefill_tokens.push(sample.sample.token);
        }

        seq.set_prefill_toks(draft_prefill_tokens);
        let (l, c, s) = seq.remove_last_token();

        // ======================= Run the model with all draft tokens. ============================
        // x{} and y{} are the output logits for each token and are the distribubtion for the next token.
        // In the next example, the prefix's last token is x2.
        // gamma=2
        // Target result:  [x0,x1,x2,y1]
        // Draft toks:  [y1,y2]
        // Draft prompt:  [x0,x1,x2,y1,y2]
        // Comparing the following: x2->y1, y1->y2
        // KV cache optimization: narrow the kv cache for last token. Add the prefix last token (x2) to the
        // target input tokens.

        // ========= Narrow the kv cache ============

        if !is_prompt {
            let t = get_mut_arcmutex!(self.target);
            let mut cache = t.cache().lock();
            let initial_cache_len = cache[0].as_ref().map(|(k, _)| k.dims()[2]).unwrap_or(0);
            for (k, v) in cache.iter_mut().flatten() {
                *k = k.narrow(2, 0, initial_cache_len - 1)?;
                *v = v.narrow(2, 0, initial_cache_len - 1)?;
            }
            if seq.is_xlora() {
                let mut cache = t.cache().xlora_lock();
                for (k, v) in cache.iter_mut().flatten() {
                    *k = k.narrow(2, 0, initial_cache_len - 1)?;
                    *v = v.narrow(2, 0, initial_cache_len - 1)?;
                }
            }
        }

        // ========= Run the model ============
        let is_xlora = get_mut_arcmutex!(self.target).get_metadata().is_xlora;
        let device = get_mut_arcmutex!(self.target).device();
        let has_no_kv_cache = get_mut_arcmutex!(self.target)
            .get_metadata()
            .has_no_kv_cache;
        let inputs = calculate_inputs(
            &[seq],
            true, // use the "prefill" tokens
            is_xlora,
            &device,
            has_no_kv_cache,
            Some(self.gamma), // Get the last gamma, see above
        )
        .unwrap();

        let logits = get_mut_arcmutex!(self.target).forward_inputs(inputs)?;

        // Reset the prefill tokens
        seq.reset_prefill_toks();
        seq.add_token(l, c, &s);

        // ======================= Rejection sampling. ============================
        // Map from each target sample to corresponding in draft sample
        let samples = sample_target_sequence_speculative(
            logits,
            seq,
            seq.return_logprobs(),
            repeat_last_n,
            get_mut_arcmutex!(self.draft)
                .get_metadata()
                .tok_trie
                .clone(),
            rng.clone(),
            self.gamma,
        )
        .await?;

        let mut accepted_tokens = Vec::new();
        for (target_sample, draft_sample) in zip(samples, draft_samples) {
            if draft_sample.sample.token == target_sample.sample.token {
                if draft_sample.sample.logprob <= target_sample.sample.logprob {
                    // Target model agrees.
                    accepted_tokens.push(target_sample.sample);
                } else {
                    // Target model disagrees.
                    let acceptance_prob = (target_sample.sample.logprob
                        / draft_sample.sample.logprob)
                        .clamp(0.0, 1.0);
                    let is_accepted = get_mut_arcmutex!(rng).gen_bool(acceptance_prob as f64);
                    if is_accepted {
                        accepted_tokens.push(target_sample.sample);
                    } else {
                        // Do not accept. Resample with updated prob dist relu(p(x) − q(x))
                        let corrected_distribution =
                            (target_sample.distribution - draft_sample.distribution)?.relu()?;
                        let t = get_mut_arcmutex!(self.target)
                            .get_metadata()
                            .tok_trie
                            .clone();
                        let r = rng.clone();
                        let sampled = sample_sequence(
                            corrected_distribution,
                            seq,
                            seq.return_logprobs(),
                            repeat_last_n,
                            t,
                            r,
                            n_seqs > 1,
                            false, // Do not add to trie
                        )
                        .await?;
                        accepted_tokens.push(sampled);
                        break;
                    }
                }
            } else {
                // Did not agree. Use the target model's choice. Return it.
                accepted_tokens.push(target_sample.sample);
                break;
            }
        }

        // Add the tokens to the seq and the trie
        for accepted in accepted_tokens {
            let eos_owned = get_mut_arcmutex!(self.target)
                .get_metadata()
                .eos_tok
                .clone();
            let eos_tok = if disable_eos_stop {
                None
            } else {
                Some(&eos_owned[..])
            };
            // Do not use the prefix cacher
            finish_and_add_tokens_to_seq!(self, prefix_cacher, seq, accepted, eos_tok, false);
            match seq.recognizer {
                SequenceRecognizer::Regex(ref mut rx) => {
                    get_mut_arcmutex!(self.target)
                        .get_metadata()
                        .tok_trie
                        .append_token(rx.as_mut(), accepted.token);
                }
                SequenceRecognizer::Cfg(ref mut cfg) => {
                    get_mut_arcmutex!(self.target)
                        .get_metadata()
                        .tok_trie
                        .append_token(cfg.as_mut(), accepted.token);
                }
                SequenceRecognizer::None => {}
            }
        }

        match post_op {
            CacheInstruction::Out => {
                get_mut_arcmutex!(self.target).clone_out_cache(input_seqs);
            }
            CacheInstruction::Nonthing => (),
            CacheInstruction::Reset { reset_non_granular } => {
                self.set_none_cache(reset_non_granular)
            }
            _ => unreachable!("Unreachable pre cache op."),
        }

        // Done! We have:
        // - Run the draft model gamma times
        // - Reset draft model cache fully
        // - Sampled draft model's distributions
        // - Run target model
        // - Execute speculative decoding algorithm on the resulting distributions
        // - Added the accepted tokens to buffer and trie
        // - Maybe fixed up cache of base model based on accepted tokens.

        Ok(())
    }
    fn forward_inputs(&mut self, _: ModelInputs) -> anyhow::Result<Tensor, candle_core::Error> {
        unreachable!("Speculative pipeline handles the forward pass in `.step`")
    }
    async fn sample(
        &self,
        _seqs: &mut [&mut Sequence],
        _logits: Tensor,
        _prefix_cacher: &mut PrefixCacheManager,
        _disable_eos_stop: bool,
        __rng: Arc<Mutex<Isaac64Rng>>,
    ) -> Result<()> {
        unreachable!("Speculative pipeline handles sampling in `.step`")
    }
    fn device(&self) -> Device {
        get_mut_arcmutex!(self.target).device()
    }
    fn tokenizer(&self) -> Arc<Tokenizer> {
        get_mut_arcmutex!(self.target).tokenizer()
    }
    fn name(&self) -> String {
        format!(
            "Speculative: tgt = `{}`, draft = `{}`, gamma = `{}`",
            get_mut_arcmutex!(self.target).name(),
            get_mut_arcmutex!(self.draft).name(),
            self.gamma,
        )
    }
    fn get_chat_template(&self) -> Arc<ChatTemplate> {
        get_mut_arcmutex!(self.target).get_chat_template()
    }
    fn reset_non_granular_state(&self) {
        get_mut_arcmutex!(self.target).reset_non_granular_state();
        get_mut_arcmutex!(self.draft).reset_non_granular_state();
    }
    fn re_isq_model(&mut self, dtype: GgmlDType) -> anyhow::Result<()> {
        get_mut_arcmutex!(self.target).re_isq_model(dtype)?;
        get_mut_arcmutex!(self.draft).re_isq_model(dtype)
    }
    fn get_metadata(&self) -> &GeneralMetadata {
        &self.metadata
    }
    fn clone_in_cache(&mut self, seqs: &mut [&mut Sequence]) {
        DefaultCacheManager.clone_in_cache(&mut *get_mut_arcmutex!(self.target), seqs)
    }
    fn clone_out_cache(&mut self, seqs: &mut [&mut Sequence]) {
        DefaultCacheManager.clone_out_cache(&mut *get_mut_arcmutex!(self.target), seqs)
    }
    fn set_none_cache(&mut self, reset_non_granular: bool) {
        DefaultCacheManager.set_none_cache(&mut *get_mut_arcmutex!(self.target));
        DefaultCacheManager.set_none_cache(&mut *get_mut_arcmutex!(self.draft));
        if reset_non_granular {
            self.reset_non_granular_state()
        }
    }
    fn cache(&self) -> &Cache {
        unreachable!()
    }
}
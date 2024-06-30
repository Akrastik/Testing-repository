use std::{any::Any, sync::Arc};

use candle_core::{quantized::GgmlDType, DType, Device, Tensor};
use candle_nn::{AdamW, Optimizer, ParamsAdamW};
use either::Either;
use indexmap::IndexMap;
use rand::{seq::SliceRandom, thread_rng};
use rand_isaac::Isaac64Rng;
use tracing::info;

use crate::{
    amoe::{AnyMoeConfig, AnyMoeTrainingInputRow, AnyMoeTrainingInputs, AnyMoeTrainingResult},
    get_mut_arcmutex,
    prefix_cacher::PrefixCacheManager,
    sampler::Sampler,
    sequence::{Sequence, SequenceGroup, SequenceRecognizer},
    utils::progress::NiceProgressBar,
    DeviceMapMetadata, Loader, ModelCategory, ModelKind, ModelPaths, Pipeline, Response,
    TokenSource, TryIntoDType,
};

use super::{
    AdapterActivationMixin, AnyMoePipelineMixin, CacheManagerMixin, IsqPipelineMixin,
    MetadataMixin, PreProcessingMixin,
};

pub struct AnyMoeLoader {
    pub target: Box<dyn Loader>,
    pub config: AnyMoeConfig,
    pub path: String,
    pub prefix: String,
    pub mlp: String,
    pub model_ids: Vec<String>,
    pub layers: Vec<usize>,
}

pub struct AnyMoePipeline {
    target: Arc<tokio::sync::Mutex<dyn Pipeline>>,
    config: AnyMoeConfig,
}

impl Loader for AnyMoeLoader {
    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    fn load_model_from_hf(
        &self,
        revision: Option<String>,
        token_source: TokenSource,
        dtype: &dyn TryIntoDType,
        device: &Device,
        silent: bool,
        mapper: DeviceMapMetadata,
        in_situ_quant: Option<GgmlDType>,
    ) -> anyhow::Result<Arc<tokio::sync::Mutex<dyn Pipeline + Send + Sync>>> {
        let target = self.target.load_model_from_hf(
            revision.clone(),
            token_source.clone(),
            dtype,
            device,
            silent,
            mapper.clone(),
            in_situ_quant,
        )?;
        Ok(Arc::new(tokio::sync::Mutex::new(AnyMoePipeline::new(
            target,
            self.config,
            self.path.clone(),
            self.prefix.clone(),
            self.mlp.clone(),
            self.model_ids.clone(),
            token_source,
            revision,
            self.layers.clone(),
        )?)))
    }

    #[allow(clippy::type_complexity, clippy::too_many_arguments)]
    fn load_model_from_path(
        &self,
        paths: &Box<dyn ModelPaths>,
        dtype: &dyn TryIntoDType,
        device: &Device,
        silent: bool,
        mapper: DeviceMapMetadata,
        in_situ_quant: Option<GgmlDType>,
    ) -> anyhow::Result<Arc<tokio::sync::Mutex<dyn Pipeline + Send + Sync>>> {
        let target = self.target.load_model_from_path(
            paths,
            dtype,
            device,
            silent,
            mapper.clone(),
            in_situ_quant,
        )?;
        Ok(Arc::new(tokio::sync::Mutex::new(AnyMoePipeline::new(
            target,
            self.config,
            self.path.clone(),
            self.prefix.clone(),
            self.mlp.clone(),
            self.model_ids.clone(),
            TokenSource::None,
            None,
            self.layers.clone(),
        )?)))
    }
    fn get_id(&self) -> String {
        format!("AnyMoE: tgt = `{}`", self.target.get_id(),)
    }
    fn get_kind(&self) -> ModelKind {
        ModelKind::AnyMoe {
            target: Box::new(self.target.get_kind()),
        }
    }
}

impl AnyMoePipeline {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        target: Arc<tokio::sync::Mutex<dyn Pipeline>>,
        config: AnyMoeConfig,
        path: String,
        prefix: String,
        mlp: String,
        model_ids: Vec<String>,
        token: TokenSource,
        revision: Option<String>,
        layers: Vec<usize>,
    ) -> anyhow::Result<Self> {
        let this = Self { target, config };
        let inputs = AnyMoeTrainingInputs::from_csv(path)?;
        info!("Loaded pretraining dataset of {} samples.", inputs.0.len());
        let AnyMoeTrainingResult { steps, final_loss } =
            this.amoe_pre_train(inputs, (prefix, mlp), model_ids, token, revision, layers)?;
        info!("Finished training in {steps} steps. Final losses per layer: {final_loss:?}");
        Ok(this)
    }
}

impl AdapterActivationMixin for AnyMoePipeline {
    fn activate_adapters(&mut self, adapters: Vec<String>) -> anyhow::Result<usize> {
        get_mut_arcmutex!(self.target).activate_adapters(adapters)
    }
}

impl CacheManagerMixin for AnyMoePipeline {
    fn cache(&self) -> &super::Cache {
        unreachable!()
    }
    fn clone_in_cache(&self, seqs: &mut [&mut Sequence], modify_draft_cache: bool) {
        get_mut_arcmutex!(self.target).clone_in_cache(seqs, modify_draft_cache)
    }
    fn clone_out_cache(&self, seqs: &mut [&mut Sequence], modify_draft_cache: bool) {
        get_mut_arcmutex!(self.target).clone_out_cache(seqs, modify_draft_cache)
    }
    fn set_none_cache(&self, reset_non_granular: bool, modify_draft_cache: bool) {
        get_mut_arcmutex!(self.target).set_none_cache(reset_non_granular, modify_draft_cache)
    }
}

impl IsqPipelineMixin for AnyMoePipeline {
    fn re_isq_model(&mut self, dtype: GgmlDType) -> anyhow::Result<()> {
        get_mut_arcmutex!(self.target).re_isq_model(dtype)
    }
}

impl PreProcessingMixin for AnyMoePipeline {
    fn get_chat_template(&self) -> Arc<crate::ChatTemplate> {
        get_mut_arcmutex!(self.target).get_chat_template()
    }
    fn get_input_processor_config(&self) -> Option<Arc<dyn Any>> {
        get_mut_arcmutex!(self.target).get_input_processor_config()
    }
    fn get_processor(&self) -> Arc<dyn super::Processor> {
        get_mut_arcmutex!(self.target).get_processor()
    }
}

impl MetadataMixin for AnyMoePipeline {
    fn device(&self) -> Device {
        get_mut_arcmutex!(self.target).device()
    }
    fn get_metadata(&self) -> Arc<super::GeneralMetadata> {
        get_mut_arcmutex!(self.target).get_metadata()
    }
    fn name(&self) -> String {
        get_mut_arcmutex!(self.target).name()
    }
    fn reset_non_granular_state(&self) {
        get_mut_arcmutex!(self.target).reset_non_granular_state()
    }
    fn tokenizer(&self) -> Arc<tokenizers::Tokenizer> {
        get_mut_arcmutex!(self.target).tokenizer()
    }
}

#[async_trait::async_trait]
impl Pipeline for AnyMoePipeline {
    fn forward_inputs(&self, inputs: Box<dyn Any>) -> Result<Tensor, candle_core::Error> {
        get_mut_arcmutex!(self.target).forward_inputs(inputs)
    }

    async fn sample(
        &self,
        seqs: &mut [&mut Sequence],
        logits: Tensor,
        prefix_cacher: &mut PrefixCacheManager,
        disable_eos_stop: bool,
        rng: Arc<std::sync::Mutex<Isaac64Rng>>,
    ) -> Result<(), candle_core::Error> {
        get_mut_arcmutex!(self.target)
            .sample(seqs, logits, prefix_cacher, disable_eos_stop, rng)
            .await
    }

    fn category(&self) -> ModelCategory {
        get_mut_arcmutex!(self.target).category()
    }
}

impl AnyMoePipelineMixin for AnyMoePipeline {
    fn amoe_pre_train(
        &self,
        inputs: AnyMoeTrainingInputs,
        (prefix, mlp): (String, String),
        model_ids: Vec<String>,
        token: TokenSource,
        revision: Option<String>,
        layers: Vec<usize>,
    ) -> anyhow::Result<AnyMoeTrainingResult, candle_core::Error> {
        let mut target = get_mut_arcmutex!(self.target);
        if !target.amoe_supported() {
            candle_core::bail!("AnyMoE is not supported for this model.");
        }

        let device = target.device();
        let processor = target.get_processor();
        let inputs_processor = target.get_processor().inputs_processor();
        let tokenizer = target.tokenizer();
        let metadata = target.get_metadata().clone();
        let input_processor_cfg = target.get_input_processor_config().clone();

        let AnyMoeConfig {
            hidden_size: _,
            lr,
            epochs,
            batch_size,
            expert_type,
        } = self.config;
        let mut steps = 0;

        info!("Expert type: {expert_type:?}");
        info!("Expert model ids: {model_ids:?}");

        // Inject the AnyMoE layers
        target.amoe_create_layers(
            model_ids,
            &token,
            revision,
            &mlp.clone(),
            self.config,
            metadata.activation_dtype,
            &device,
            (prefix, mlp),
            layers,
            expert_type,
        )?;
        let layer_vars = target.amoe_layer_vars();

        info!(
            "{} gating layers, {} trainable parameters, lr = {lr}, {epochs} epochs, batch size = {batch_size}",
            layer_vars.len(),
            target.amoe_base_model_trainable_params()
        );

        let mut optimizers = layer_vars
            .into_iter()
            .map(|vars| {
                AdamW::new(
                    vars,
                    ParamsAdamW {
                        lr,
                        beta1: 0.9,
                        beta2: 0.999,
                        eps: 1e-8,
                        weight_decay: 0.0,
                    },
                )
            })
            .collect::<candle_core::Result<Vec<_>>>()?;

        let mut rng = thread_rng();
        let mut samples = inputs.0;

        // Create several dummy objects for the sequences.
        let (dummy_sender, _) = tokio::sync::mpsc::channel(10000);
        let dummy_sampler = Sampler::new(None, 0, tokenizer.clone(), None, None, None, -1, 0.0);
        let dummy_group = Arc::new(tokio::sync::Mutex::new(SequenceGroup::new(
            1, false, false, 0,
        )));

        // Clear KV cache in prep for training
        target.set_none_cache(true, true);

        let mut latest_loss = vec![0.0; optimizers.len()];
        let mut all_losses = Vec::new();

        for _ in NiceProgressBar::<_, 'g'>(0..epochs, "Training gating layers") {
            samples.as_mut_slice().shuffle(&mut rng);
            for batch in samples.chunks(batch_size) {
                steps += 1;

                // === PREPARE INPUTS ==
                let mut seqs = Vec::new();
                for AnyMoeTrainingInputRow { prompt, expert: _ } in batch {
                    let tokens = processor
                        .process(
                            &*target,
                            vec![IndexMap::from([
                                ("role".to_string(), Either::Left("user".to_string())),
                                ("content".to_string(), Either::Left(prompt.clone())),
                            ])],
                            true,
                        )
                        .map_err(|e| candle_core::Error::Msg(e.to_string()))?;
                    seqs.push(new_dummy_seq(
                        tokens,
                        dummy_sender.clone(),
                        dummy_sampler.clone(),
                        dummy_group.clone(),
                    ));
                }
                let mut input_seqs = seqs.iter_mut().collect::<Vec<_>>();
                let inputs = inputs_processor
                    .process_inputs(
                        tokenizer.clone(),
                        &mut input_seqs,
                        true, // Always a prompt
                        metadata.is_xlora,
                        &device,
                        metadata.has_no_kv_cache,
                        None,
                        input_processor_cfg.clone(),
                    )
                    .unwrap();

                // === PREPARE AND RUN MODEL ==

                // Run the model, ignoring the logits
                let _ = target.forward_inputs(inputs)?;

                // Clear the KV cache
                target.set_none_cache(true, true);

                // === BACKWARD STEP ==
                #[allow(clippy::cast_possible_truncation)]
                let labels = Tensor::from_vec(
                    batch
                        .iter()
                        .map(|AnyMoeTrainingInputRow { prompt: _, expert }| *expert as u32)
                        .collect::<Vec<_>>(),
                    (batch.len(),),
                    &device,
                )?;

                let cached = target.amoe_take_cached_gating_outputs();
                for (layer, (optimizer, output)) in optimizers.iter_mut().zip(cached).enumerate() {
                    let loss = candle_nn::loss::cross_entropy(&output, &labels)?;
                    let gradstore = loss.backward()?;
                    optimizer.step(&gradstore)?;
                    latest_loss[layer] = loss.to_dtype(DType::F32)?.to_scalar::<f32>()?;
                }
                all_losses.push(latest_loss.clone());
            }
        }

        target.amoe_done_training();
        assert_eq!(target.amoe_base_model_trainable_params(), 0);

        Ok(AnyMoeTrainingResult {
            steps,
            final_loss: latest_loss,
        })
    }
}

/// Create a dummy sequence containing just the prompt. This is OK because we just want a sequence that
/// has no information other than the input tokens (and maybe images).
fn new_dummy_seq(
    tokens: Vec<u32>,
    dummy_sender: tokio::sync::mpsc::Sender<Response>,
    dummy_sampler: Sampler,
    dummy_group: Arc<tokio::sync::Mutex<SequenceGroup>>,
) -> Sequence {
    Sequence::new_waiting(
        tokens,
        0,
        0,
        1,
        dummy_sender,
        dummy_sampler,
        vec![],
        vec![],
        None,
        false,
        false,
        dummy_group,
        0,
        0,
        SequenceRecognizer::None,
        None,
        None,
        None,
        None, // TODO support images
    )
}
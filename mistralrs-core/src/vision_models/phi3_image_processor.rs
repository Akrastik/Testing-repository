use std::{any::Any, sync::Arc};

use candle_core::{Device, Result};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use indexmap::IndexMap;

use crate::{
    pipeline::{
        text_models_inputs_processor::{self, get_completion_input, get_prompt_input},
        InputsProcessor, InputsProcessorType, Processor,
    },
    sequence::Sequence,
    Content, Pipeline,
};

use super::{
    image_processor::{from_pixel_data, get_pixel_data, ImagePreProcessor, PreprocessedImages},
    phi3::Phi3VisionSpecificArgs,
    preprocessor_config::PreProcessorConfig,
    processor_config::ProcessorConfig,
    ModelInputs,
};

// Input processor
pub struct Phi3ImageProcessor {
    image_mean: [f64; 3],
    image_std: [f64; 3],
    num_crops: usize,
}
// Processor
pub struct Phi3Processor {
    image_mean: [f64; 3],
    image_std: [f64; 3],
    num_crops: usize,
    num_img_tokens: usize,
}

impl Phi3Processor {
    pub fn new(_: ProcessorConfig, preprocessor_config: PreProcessorConfig) -> Self {
        Self {
            image_mean: preprocessor_config
                .image_mean
                .unwrap_or(Phi3ImageProcessor::DEFAULT_MEAN),
            image_std: preprocessor_config
                .image_std
                .unwrap_or(Phi3ImageProcessor::DEFAULT_STD),
            num_crops: preprocessor_config.num_crops.unwrap_or(1),
            num_img_tokens: preprocessor_config
                .num_img_tokens
                .expect("Require `num_img_tokens` for phi3 preprocessor config."),
        }
    }
}

impl Processor for Phi3Processor {
    fn process(
        &self,
        pipeline: &dyn Pipeline,
        messages: Vec<IndexMap<String, Content>>,
        add_generation_prompt: bool,
    ) -> anyhow::Result<Vec<u32>> {
        todo!()
    }
    fn inputs_processor(&self) -> Arc<dyn InputsProcessor> {
        Arc::new(Phi3ImageProcessor {
            image_mean: self.image_mean,
            image_std: self.image_std,
            num_crops: self.num_crops,
        })
    }
    fn get_special_tokens(&self) -> &[&'static str] {
        &[]
    }
}

impl InputsProcessor for Phi3ImageProcessor {
    fn get_type(&self) -> InputsProcessorType {
        InputsProcessorType::Vision
    }
    fn process_inputs(
        &self,
        input_seqs: &mut [&mut Sequence],
        is_prompt: bool,
        is_xlora: bool,
        device: &Device,
        no_kv_cache: bool,
        last_n_context_len: Option<(usize, usize)>,
        other_config: Option<Arc<dyn Any>>,
    ) -> anyhow::Result<Box<dyn Any>> {
        if is_xlora {
            anyhow::bail!("Cannot make inputs for X-LoRA vision model.");
        }
        if no_kv_cache {
            anyhow::bail!("Vision model must have kv cache.");
        }
        let text_models_inputs_processor::InputMetadata {
            input,
            positions,
            positions_kernel,
            context_lens,
            position_ids,
        } = if is_prompt {
            get_prompt_input(input_seqs, device, last_n_context_len)?
        } else {
            get_completion_input(input_seqs, device, no_kv_cache, last_n_context_len)?
        };
        let config = other_config.expect("Need a PreProcessorConfig config.");
        let config: &PreProcessorConfig = config.downcast_ref().expect("Downcast failed.");
        Ok(Box::new(ModelInputs {
            input_ids: input,
            seqlen_offsets: positions,
            seqlen_offsets_kernel: positions_kernel,
            context_lens,
            position_ids,
            pixel_values: todo!(),
            model_specific_args: Box::new(Phi3VisionSpecificArgs {
                image_sizes: todo!(),
            }),
        }))
    }
}

/// Pad image, left and right if transposed, and top to bottom if not
fn pad(image: &DynamicImage, trans: bool, device: &Device) -> DynamicImage {
    let (mut w, mut h) = image.dimensions();
    if w < h {
        std::mem::swap(&mut w, &mut h);
    }

    let tar = ((h as f32 / 336.).ceil() * 336.) as u32;
    let top_pad = ((tar - h as u32) as f32 / 2.) as u32; // also right if transposed
    let bottom_pad = tar - h as u32 - top_pad; // also left if transposed
    let left_pad = 0;
    let right_pad = 0;
    /*from_pixel_data(
        get_pixel_data(image, image.dimensions().1, image.dimensions().0),
        pad_h,
        pad_w,
        255,
    )*/
    todo!() // TODO POINT HERE
}

impl Phi3ImageProcessor {
    fn hd_transform(&self, image: &DynamicImage) -> Result<DynamicImage> {
        let (mut w, mut h) = image.dimensions();
        let trans = if w < h {
            std::mem::swap(&mut w, &mut h);
            true
        } else {
            false
        };
        let ratio = w as f32 / h as f32;
        let mut scale = 1.0;
        while (scale * (scale / ratio).ceil()) as usize <= self.num_crops {
            scale += 1.0;
        }
        let new_w = (scale * 336.) as u32;
        let new_h = (new_w as f32 / ratio) as u32;

        // torchvision.transforms.functional.resize's default interpolation mode is bilinear
        let img = image.resize(
            if trans { new_h } else { new_w },
            if trans { new_w } else { new_h },
            FilterType::Triangle,
        );
        todo!()
    }
}

impl ImagePreProcessor for Phi3ImageProcessor {
    #[allow(clippy::excessive_precision)]
    const DEFAULT_MEAN: [f64; 3] = [0.48145466, 0.4578275, 0.40821073];
    #[allow(clippy::excessive_precision)]
    const DEFAULT_STD: [f64; 3] = [0.26862954, 0.26130258, 0.27577711];

    fn preprocess(
        &self,
        mut images: Vec<DynamicImage>,
        config: &PreProcessorConfig,
        device: &Device,
    ) -> Result<PreprocessedImages> {
        for image in images.iter_mut() {
            // Convert to rgb
            if config.do_convert_rgb {
                *image = DynamicImage::ImageRgb8(image.to_rgb8());
            }

            // Normalize
            if config.do_normalize {
                *image = self.normalize(
                    image,
                    config.image_mean.unwrap_or(Self::DEFAULT_MEAN),
                    config.image_std.unwrap_or(Self::DEFAULT_STD),
                );
            }
        }
        todo!()
    }
}

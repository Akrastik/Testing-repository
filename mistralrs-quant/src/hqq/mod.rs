use candle_core::{DType, Device, Result, Shape, Tensor};

#[cfg(feature = "cuda")]
use candle_core::{
    cuda::{cudarc::driver::DevicePtr, CudaStorageSlice, WrapErr},
    from_storage_no_op, CudaStorage, Storage,
};

#[cfg(feature = "cuda")]
use half::{bf16, f16};
use std::{
    num::NonZeroUsize,
    sync::{atomic::AtomicUsize, Arc},
};

use crate::{
    utils::{BitWiseOp, LeftshiftOp},
    IsqType, QuantMethod, QuantMethodConfig, QuantizedSerde,
};

#[cfg(feature = "cuda")]
use crate::utils::{get_cuda_device, get_cuda_slice};

#[cfg(feature = "cuda")]
use ffi::{eight_bit, four_bit, one_bit, three_bit, two_bit};

#[cfg(feature = "cuda")]
mod ffi;

#[cfg(not(feature = "cuda"))]
mod hqq_cpu;

mod optimize;
mod quantize;

pub(crate) const ISQ_HQQ_GROUP_SIZE: usize = 64;
pub(crate) const ISQ_HQQ_DEFAULT_OPT_STEPS: Option<usize> = Some(10);
pub(crate) const OPTIMIZER_HQQ_DEFAULT_STEPS: usize = 20;

#[cfg(feature = "cuda")]
macro_rules! dequant_for_dtype {
    ($this:expr, w=$wq_t:ty, sz=$scale_t:ty, $dtype:ident, pack=$pack:expr, $dev:expr, $bit_thing:ident, $postfix:tt) => {{
        paste::paste! {
            let w_slice = get_cuda_slice::<$wq_t>(&$this.w_q)?;
            let scale_slice = get_cuda_slice::<$scale_t>(&$this.scales)?;
            let zero_slice = get_cuda_slice::<$scale_t>(&$this.zeros)?;

            let (h, w) = $this.w_q.dims2()?;
            let num_packed_elems = $pack;
            let out_shape = Shape::from_dims(&[num_packed_elems * h, w]);

            let out = unsafe { $dev.alloc::<$scale_t>(out_shape.elem_count()).w()? };
            let out_ptr = *out.device_ptr() as *mut $scale_t;
            unsafe {
                $bit_thing::[< dequantize_ $postfix >](
                    w_slice,
                    scale_slice,
                    zero_slice,
                    out_ptr,
                    h as i32,
                    w as i32,
                );
            }

            let storage = CudaStorage {
                slice: CudaStorageSlice::$dtype(out),
                device: $dev.clone(),
            };
            let storage = Storage::Cuda(storage);

            from_storage_no_op(storage, out_shape, false)
        }
    }};
}

#[derive(Debug, Clone, Copy)]
pub enum HqqAxis {
    Zero = 0,
    One = 1,
}

#[derive(Debug, Clone, Copy)]
pub enum HqqBits {
    Eight = 8,
    Four = 4,
    Three = 3,
    Two = 2,
    One = 1,
}

impl HqqBits {
    // https://github.com/mobiusml/hqq/blob/306e30d9400629523c8e0af70101d8d7073cb3d5/hqq/core/bitpack.py#L10
    pub(crate) fn bitpack_type(&self) -> impl Fn(Tensor) -> Result<Tensor> {
        match self {
            Self::Eight => |wq: Tensor| wq.to_dtype(DType::U8),
            Self::Four => |wq: Tensor| {
                let wq = wq.to_dtype(DType::U8)?;
                let step = (wq.dims()[0] as f64 / 2.) as usize;

                let a = wq.narrow(0, 0, step)?;
                let b = wq.narrow(0, step, step)?;
                a.leftshift(4)?.bitwise_or(&b)
            },
            Self::Two => |wq: Tensor| {
                let wq = wq.to_dtype(DType::U8)?;
                let step = (wq.dims()[0] as f64 / 4.) as usize;

                let a = wq.narrow(0, 0, step)?;
                let b = wq.narrow(0, step, step)?;
                let c = wq.narrow(0, step * 2, step)?;
                let d = wq.narrow(0, step * 3, step)?;

                a.leftshift(6)?
                    .bitwise_or(&b.leftshift(4)?)?
                    .bitwise_or(&c.leftshift(2)?)?
                    .bitwise_or(&d)
            },
            Self::Three => |wq_in: Tensor| {
                let wq = Tensor::zeros(
                    (
                        (10. * (wq_in.dims()[0] as f64 / 10.).ceil()) as usize,
                        wq_in.dims()[1],
                    ),
                    DType::I32,
                    wq_in.device(),
                )?;
                let wq =
                    wq.slice_assign(&[&(..wq_in.dims()[0]), &..], &wq_in.to_dtype(DType::I32)?)?;
                let step = (wq.dims()[0] as f64 / 10.) as usize;

                let a = wq.narrow(0, 0, step)?;
                let b = wq.narrow(0, step, step)?;
                let c = wq.narrow(0, step * 2, step)?;
                let d = wq.narrow(0, step * 3, step)?;
                let e = wq.narrow(0, step * 4, step)?;
                let f = wq.narrow(0, step * 5, step)?;
                let g = wq.narrow(0, step * 6, step)?;
                let h = wq.narrow(0, step * 7, step)?;
                let i = wq.narrow(0, step * 8, step)?;
                let j = wq.narrow(0, step * 9, step)?;

                a.leftshift(27)?
                    .bitwise_or(&b.leftshift(24)?)?
                    .bitwise_or(&c.leftshift(21)?)?
                    .bitwise_or(&d.leftshift(18)?)?
                    .bitwise_or(&e.leftshift(15)?)?
                    .bitwise_or(&f.leftshift(12)?)?
                    .bitwise_or(&g.leftshift(9)?)?
                    .bitwise_or(&h.leftshift(6)?)?
                    .bitwise_or(&i.leftshift(3)?)?
                    .bitwise_or(&j)
            },
            Self::One => |wq: Tensor| {
                let wq = wq.to_dtype(DType::U8)?;
                let step = (wq.dims()[0] as f64 / 8.) as usize;

                let a = wq.narrow(0, 0, step)?;
                let b = wq.narrow(0, step, step)?;
                let c = wq.narrow(0, step * 2, step)?;
                let d = wq.narrow(0, step * 3, step)?;
                let e = wq.narrow(0, step * 4, step)?;
                let f = wq.narrow(0, step * 5, step)?;
                let g = wq.narrow(0, step * 6, step)?;
                let h = wq.narrow(0, step * 7, step)?;

                a.leftshift(7)?
                    .bitwise_or(&b.leftshift(6)?)?
                    .bitwise_or(&c.leftshift(5)?)?
                    .bitwise_or(&d.leftshift(4)?)?
                    .bitwise_or(&e.leftshift(3)?)?
                    .bitwise_or(&f.leftshift(2)?)?
                    .bitwise_or(&g.leftshift(1)?)?
                    .bitwise_or(&h)
            },
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HqqConfig {
    pub bits: HqqBits,
    pub group_size: NonZeroUsize,
    pub axis: HqqAxis,
    pub optimization_steps: Option<usize>,
    pub round_zeros: bool,  // default false
    pub channel_wise: bool, // default true
}

#[derive(Debug)]
pub struct HqqLayer {
    pub(crate) w_q: Tensor,
    pub(crate) zeros: Tensor,
    pub(crate) scales: Tensor,
    pub(crate) bias: Option<Tensor>,
    pub(crate) w_shape: Shape,
    pub(crate) cfg: HqqConfig,
}

impl HqqLayer {
    /// Dequantize `self` into a tensor of shape `scales` or `zeros`.
    #[cfg(not(feature = "cuda"))]
    fn dequantize(&self) -> Result<Tensor> {
        use crate::hqq::hqq_cpu::{
            Dequant1Bit, Dequant2Bit, Dequant3Bit, Dequant4Bit, Dequant8Bit,
        };

        match (self.scales.dtype(), self.zeros.dtype()) {
            (DType::F16, DType::F16) | (DType::BF16, DType::BF16) | (DType::F32, DType::F32) => (),
            (a, b) => {
                candle_core::bail!("Expected all dtypes to be the same, got ({a:?}, {b:?}).")
            }
        }
        if !(self.w_q.is_contiguous() && self.scales.is_contiguous() && self.zeros.is_contiguous())
        {
            candle_core::bail!("All tensors must be contiguous!");
        }
        if self.cfg.axis as usize != 0 {
            candle_core::bail!(
                "CPU HQQ dequantization requires axis == 0, got {}.",
                self.cfg.axis as usize
            );
        }
        let (h, w) = self.w_q.dims2()?;

        match self.cfg.bits as usize {
            8 => self
                .w_q
                .apply_op3_no_bwd(&self.scales, &self.zeros, &Dequant8Bit { h, w })?
                .reshape(&self.w_shape),
            4 => self
                .w_q
                .apply_op3_no_bwd(&self.scales, &self.zeros, &Dequant4Bit { h, w })?
                .reshape(&self.w_shape),
            3 => self
                .w_q
                .apply_op3_no_bwd(&self.scales, &self.zeros, &Dequant3Bit { h, w })?
                .reshape(&self.w_shape),
            2 => self
                .w_q
                .apply_op3_no_bwd(&self.scales, &self.zeros, &Dequant2Bit { h, w })?
                .reshape(&self.w_shape),
            1 => self
                .w_q
                .apply_op3_no_bwd(&self.scales, &self.zeros, &Dequant1Bit { h, w })?
                .reshape(&self.w_shape),
            b => candle_core::bail!("Unreachable bits {b}"),
        }
    }

    /// Dequantize `self` into a tensor of shape `scales` or `zeros`.
    #[cfg(feature = "cuda")]
    fn dequantize(&self) -> Result<Tensor> {
        match (self.scales.dtype(), self.zeros.dtype()) {
            (DType::F16, DType::F16) | (DType::BF16, DType::BF16) | (DType::F32, DType::F32) => (),
            (a, b) => {
                candle_core::bail!("Expected all dtypes to be the same, got ({a:?}, {b:?}).")
            }
        }
        if !(self.w_q.is_contiguous() && self.scales.is_contiguous() && self.zeros.is_contiguous())
        {
            candle_core::bail!("All tensors must be contiguous!");
        }
        if self.cfg.axis as usize != 0 {
            candle_core::bail!(
                "CUDA HQQ dequantization requires axis == 0, got {}.",
                self.cfg.axis as usize
            );
        }
        let dev = get_cuda_device(&self.w_q)?;

        let inner = match (self.cfg.bits as usize, self.scales.dtype()) {
            // 8 bits
            (8, DType::F32) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = f32,
                    F32,
                    pack = 1,
                    dev,
                    eight_bit,
                    8bit_u8_kernel_f32
                )
            }
            (8, DType::F16) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = f16,
                    F16,
                    pack = 1,
                    dev,
                    eight_bit,
                    8bit_u8_kernel_f16
                )
            }
            (8, DType::BF16) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = bf16,
                    BF16,
                    pack = 1,
                    dev,
                    eight_bit,
                    8bit_u8_kernel_bf16
                )
            }

            // 4 bits
            (4, DType::F32) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = f32,
                    F32,
                    pack = 2,
                    dev,
                    four_bit,
                    4bit_u8_kernel_f32
                )
            }
            (4, DType::F16) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = f16,
                    F16,
                    pack = 2,
                    dev,
                    four_bit,
                    4bit_u8_kernel_f16
                )
            }
            (4, DType::BF16) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = bf16,
                    BF16,
                    pack = 2,
                    dev,
                    four_bit,
                    4bit_u8_kernel_bf16
                )
            }

            // 3 bits
            // https://github.com/mobiusml/hqq/blob/306e30d9400629523c8e0af70101d8d7073cb3d5/hqq/kernels/hqq_aten_cuda.cpp#L42-L45
            (3, DType::F32) => {
                let res = dequant_for_dtype!(
                    self,
                    w = i32,
                    sz = f32,
                    F32,
                    pack = 10,
                    dev,
                    three_bit,
                    3bit_32_kernel_f32
                );
                res.narrow(self.cfg.axis as usize, 0, self.cfg.group_size.into())?
            }
            (3, DType::F16) => {
                let res = dequant_for_dtype!(
                    self,
                    w = i32,
                    sz = f16,
                    F16,
                    pack = 10,
                    dev,
                    three_bit,
                    3bit_32_kernel_f16
                );
                res.narrow(self.cfg.axis as usize, 0, self.cfg.group_size.into())?
            }
            (3, DType::BF16) => {
                let res = dequant_for_dtype!(
                    self,
                    w = i32,
                    sz = bf16,
                    BF16,
                    pack = 10,
                    dev,
                    three_bit,
                    3bit_32_kernel_bf16
                );
                res.narrow(self.cfg.axis as usize, 0, self.cfg.group_size.into())?
            }

            // 2 bits
            (2, DType::F32) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = f32,
                    F32,
                    pack = 4,
                    dev,
                    two_bit,
                    2bit_u8_kernel_f32
                )
            }
            (2, DType::F16) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = f16,
                    F16,
                    pack = 4,
                    dev,
                    two_bit,
                    2bit_u8_kernel_f16
                )
            }
            (2, DType::BF16) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = bf16,
                    BF16,
                    pack = 4,
                    dev,
                    two_bit,
                    2bit_u8_kernel_bf16
                )
            }

            // 1 bit
            (1, DType::F32) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = f32,
                    F32,
                    pack = 8,
                    dev,
                    one_bit,
                    1bit_u8_kernel_f32
                )
            }
            (1, DType::F16) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = f16,
                    F16,
                    pack = 8,
                    dev,
                    one_bit,
                    1bit_u8_kernel_f16
                )
            }
            (1, DType::BF16) => {
                dequant_for_dtype!(
                    self,
                    w = u8,
                    sz = bf16,
                    BF16,
                    pack = 8,
                    dev,
                    one_bit,
                    1bit_u8_kernel_bf16
                )
            }
            (bits, dtype) => candle_core::bail!("Unsupported bit width {bits} and dtype {dtype:?}"),
        };
        inner.reshape(&self.w_shape)
    }

    fn dequantize_matmul(&self, xs: &Tensor) -> Result<Tensor> {
        let w = self.dequantize()?;
        let w = match *xs.dims() {
            [b1, b2, _, _] => w.broadcast_left((b1, b2))?.t()?,
            [bsize, _, _] => w.broadcast_left(bsize)?.t()?,
            _ => w.t()?,
        };
        let res = xs.matmul(&w)?;
        if let Some(ref bias) = self.bias {
            res + bias
        } else {
            Ok(res)
        }
    }

    pub fn with_bias(mut self, bias: Tensor) -> Self {
        self.bias = Some(bias);
        self
    }
}

impl QuantMethod for HqqLayer {
    fn new(method: QuantMethodConfig) -> Result<Self>
    where
        Self: Sized,
    {
        match method {
            QuantMethodConfig::Gguf { .. }
            | QuantMethodConfig::Unquantized(_)
            | QuantMethodConfig::Gptq { .. } => {
                unreachable!()
            }
            QuantMethodConfig::Hqq {
                tensor,
                bits,
                group_size,
                axis,
                optimization_steps,
                round_zeros,
                channel_wise,
                bias,
            } => {
                let cfg = HqqConfig {
                    bits,
                    group_size,
                    axis,
                    optimization_steps,
                    round_zeros: round_zeros.unwrap_or(false),
                    channel_wise: channel_wise.unwrap_or(true),
                };

                let this = Self::quantize(&tensor, tensor.device(), cfg)?;
                if let Some(bias) = bias {
                    Ok(this.with_bias(bias))
                } else {
                    Ok(this)
                }
            }
        }
    }

    fn forward(&self, a: &Tensor) -> Result<Tensor> {
        /*
        if self.cfg.force_dequantize {
            self.dequantize_matmul(a)
        } else {
            todo!()
        } */
        self.dequantize_matmul(a)
    }

    fn quantized_act_type(&self) -> Option<DType> {
        Some(self.scales.dtype())
    }

    fn add_delta_w(&self, _delta: &Tensor) -> Result<Arc<dyn QuantMethod>> {
        candle_core::bail!("HQQ quantization does not support adding weight delta.")
    }

    fn dtype_and_device(&self) -> (DType, Device) {
        (self.scales.dtype(), self.scales.device().clone())
    }

    fn get_bias_mut(&mut self) -> Option<&mut Tensor> {
        self.bias.as_mut()
    }

    fn apply_isq(
        self: Arc<Self>,
        dtype: Option<IsqType>,
        device: Device,
        n_quantized: &AtomicUsize,
    ) -> Result<Arc<dyn QuantMethod>> {
        n_quantized.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let bits = match dtype {
            Some(IsqType::HQQ8) => HqqBits::Eight,
            Some(IsqType::HQQ4) => HqqBits::Four,
            // Some(IsqType::HQQ3) => HqqBits::Three,
            // Some(IsqType::HQQ2) => HqqBits::Two,
            // Some(IsqType::HQQ1) => HqqBits::One,
            _ => candle_core::bail!("Expected a HQQ ISQ type."),
        };
        let cfg = HqqConfig {
            bits,
            group_size: ISQ_HQQ_GROUP_SIZE.try_into()?,
            axis: HqqAxis::Zero,
            optimization_steps: ISQ_HQQ_DEFAULT_OPT_STEPS,
            round_zeros: false,
            channel_wise: true,
        };
        let dequant = self.dequantize()?;
        let res = Self::quantize(&dequant, &device, cfg)?;
        if let Some(ref bias) = self.bias {
            let bias = bias
                .to_device(&device)?
                .to_dtype(res.dtype_and_device().0)?;
            Ok(Arc::new(res.with_bias(bias)))
        } else {
            Ok(Arc::new(res))
        }
    }

    fn get_max_isq_cpu_threads(&self, _dtype: IsqType) -> Option<NonZeroUsize> {
        // Use 1 because we quantize on the GPU
        Some(1.try_into().unwrap())
    }
}

impl QuantizedSerde for HqqLayer {}

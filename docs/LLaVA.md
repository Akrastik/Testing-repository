# LLaVA and LLaVANext Model: `llava-hf model family`

The [LLaVA](https://arxiv.org/abs/2310.03744) and [LLaVANext](https://llava-vl.github.io/blog/2024-01-30-llava-next/) are great multimodal models that can handle both text and vision inputs.

This implementation supports both LLaVA and LLaVANext(which adds multi resolution image processing) and two types of LLM base model: llama and mistral. Currently it is tested on:
* llava-hf/llava-v1.6-mistral-7b-hf
* llava-hf/llava-v1.6-vicuna-7b-hf
* llava-hf/llava-1.5-7b-hf


The LLaVA and LLaVANext Model has support in the Rust, Python, and HTTP APIs. The LLaVA and LLaVANext Model also supports ISQ for increased performance. 

The Python and HTTP APIs support sending images as:
- URL
- Path to a local image
- [Base64](https://en.wikipedia.org/wiki/Base64) encoded string

The Rust API takes an image from the [image](https://docs.rs/image/latest/image/index.html) crate.

## HTTP server
You can find this example [here](../examples/server/llava_next.py).

We support an OpenAI compatible HTTP API for vision models. This example demonstrates sending a chat completion request with an image.

> Note: The image_url may be either a path, URL, or a base64 encoded string.

---

**Image:**
<img src="https://d2r55xnwy6nx47.cloudfront.net/uploads/2018/02/Ants_Lede1300.jpg" width = "1000" height = "666">

**Prompt:**
```
What is shown in this image?
```

**Output:**
```
The image depicts a group of orange ants climbing over a black pole. The ants are moving in the same direction, forming a line as they ascend the pole.
```

---

1) Start the server
```
cargo run --release --features ... -- --port 1234 --isq Q4K vision-plain -m llava-hf/llava-v1.6-mistral-7b-hf -a llava_next
```

2) Send a request

```py
import openai

completion = openai.chat.completions.create(
    model="llava_next",
    messages=[
        {
            "role": "user",
            "content": [
                {
                    "type": "image_url",
                    "image_url": {
                        "url": "https://d2r55xnwy6nx47.cloudfront.net/uploads/2018/02/Ants_Lede1300.jpg"
                    },
                },
                {
                    "type": "text",
                    "text": "What is shown in this image?",
                },
            ],
        },
    ],
    max_tokens=256,
    frequency_penalty=1.0,
    top_p=0.1,
    temperature=0,
)
resp = completion.choices[0].message.content
print(resp)
```

- You can find an example of encoding the [image via base64 here](../examples/server/phi3v_base64.py).
- You can find an example of loading an [image locally here](../examples/server/phi3v_local_img.py).

---

## Rust
You can find this example [here](../mistralrs/examples/llava_next/main.rs).

This is a minimal example of running the LLaVA and LLaVANext model with a dummy image.

```rust
use either::Either;
use image::{ColorType, DynamicImage};
use indexmap::IndexMap;
use std::sync::Arc;
use tokio::sync::mpsc::channel;

use mistralrs::{
    Constraint, Device, DeviceMapMetadata, MistralRs, MistralRsBuilder, NormalRequest, Request,
    RequestMessage, Response, SamplingParams, SchedulerMethod, TokenSource, VisionLoaderBuilder,
    VisionLoaderType, VisionSpecificConfig,
};

fn setup() -> anyhow::Result<Arc<MistralRs>> {
    // Select a Mistral model
    let loader = VisionLoaderBuilder::new(
        VisionSpecificConfig {
            use_flash_attn: false,
            repeat_last_n: 64,
        },
        None,
        None,
        Some("llava-hf/llava-v1.6-mistral-7b-hf".to_string()),
    )
    .build(VisionLoaderType::Idefics2);
    // Load, into a Pipeline
    let pipeline = loader.load_model_from_hf(
        None,
        TokenSource::CacheToken,
        None,
        &Device::cuda_if_available(0)?,
        false,
        DeviceMapMetadata::dummy(),
        None,
    )?;
    // Create the MistralRs, which is a runner
    Ok(MistralRsBuilder::new(pipeline, SchedulerMethod::Fixed(5.try_into().unwrap())).build())
}

fn main() -> anyhow::Result<()> {
    let mistralrs = setup()?;

    let (tx, mut rx) = channel(10_000);
    let request = Request::Normal(NormalRequest {
        messages: RequestMessage::VisionChat {
            images: vec![DynamicImage::new(1280, 720, ColorType::Rgb8)],
            messages: vec![IndexMap::from([
                ("role".to_string(), Either::Left("user".to_string())),
                (
                    "content".to_string(),
                    Either::Left("What is shown in this image?".to_string()),
                ),
            ])],
        },
        sampling_params: SamplingParams::default(),
        response: tx,
        return_logprobs: false,
        is_streaming: false,
        id: 0,
        constraint: Constraint::None,
        suffix: None,
        adapters: None,
    });
    mistralrs.get_sender()?.blocking_send(request)?;

    let response = rx.blocking_recv().unwrap();
    match response {
        Response::Done(c) => println!("Text: {}", c.choices[0].message.content),
        _ => unreachable!(),
    }
    Ok(())
}
```

## Python
You can find this example [here](../examples/python/llava_next.py).

This example demonstrates loading and sending a chat completion request with an image.

> Note: the image_url may be either a path, URL, or a base64 encoded string.

```py
from mistralrs import Runner, Which, ChatCompletionRequest, VisionArchitecture

runner = Runner(
    which=Which.VisionPlain(
        model_id="llava-hf/llava-v1.6-mistral-7b-hf",
        tokenizer_json=None,
        repeat_last_n=64,
        arch=VisionArchitecture.Idefics2,
    ),
)

res = runner.send_chat_completion_request(
    ChatCompletionRequest(
        model="llava_next",
        messages=[
            {
                "role": "user",
                "content": [
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": "https://d2r55xnwy6nx47.cloudfront.net/uploads/2018/02/Ants_Lede1300.jpg"
                        },
                    },
                    {
                        "type": "text",
                        "text": "What is shown in this image?",
                    },
                ],
            },
        ],
        max_tokens=256,
        presence_penalty=1.0,
        top_p=0.1,
        temperature=0.1,
    )
)
print(res.choices[0].message.content)
print(res.usage)
```

- You can find an example of encoding the [image via base64 here](../examples/python/phi3v_base64.py).
- You can find an example of loading an [image locally here](../examples/python/phi3v_local_img.py).
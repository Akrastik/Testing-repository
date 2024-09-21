use std::fs::File;

use anyhow::Result;
use mistralrs::{
    LoraModelBuilder, RequestBuilder, TextMessageRole, TextMessages, TextModelBuilder,
};

#[tokio::main]
async fn main() -> Result<()> {
    let model =
        LoraModelBuilder::from_text_model_builder(
            TextModelBuilder::new("HuggingFaceH4/zephyr-7b-beta".to_string()).with_logging(),
            "lamm-mit/x-lora".to_string(),
            serde_json::from_reader(File::open("my-ordering-file.json").unwrap_or_else(|_| {
                panic!("Could not load ordering file at my-ordering-file.json")
            }))?,
        )
        .build()
        .await?;

    // First example: activate adapters per-request
    let messages = RequestBuilder::new()
        .set_adapters(vec!["adapter_2".to_string()])
        .add_message(
            TextMessageRole::User,
            "Hello! How are you? Please write generic binary search function in Rust.",
        );

    let response = model.send_chat_request(messages).await?;

    println!("{}", response.choices[0].message.content.as_ref().unwrap());
    dbg!(
        response.usage.avg_prompt_tok_per_sec,
        response.usage.avg_compl_tok_per_sec
    );

    // Second example: activate adapters for the whole model, used for all subsequent requests
    model
        .activate_adapters(vec!["adapter_1".to_string()])
        .await?;

    let messages = TextMessages::new().add_message(
        TextMessageRole::User,
        "Hello! How are you? Please write generic binary search function in Rust.",
    );

    let response = model.send_chat_request(messages).await?;

    println!("{}", response.choices[0].message.content.as_ref().unwrap());
    dbg!(
        response.usage.avg_prompt_tok_per_sec,
        response.usage.avg_compl_tok_per_sec
    );

    Ok(())
}

searchState.loadedDescShard("mistralrs", 0, "This crate provides an asynchronous, multithreaded API to …\nChat completion streaming request chunk.\nAn OpenAI compatible chat completion response.\nTemplate for chat models including bos/eos/unk as well as …\nChat completion choice.\nCompletion streaming chunk choice.\nCompletion request choice.\nAn OpenAI compatible completion response.\nControl the constraint with Regex or Yacc.\nThe different types of elements allowed in tensors.\nDelta in content for streaming response.\nMetadata to initialize the device mapper.\nContains the error value\nSelect a GGML model.\nA loader for a GGML model.\nA builder for a GGML loader.\nConfig for a GGML loader.\nSelect a GGUF model.\nLoader for a GGUF model.\nA builder for a GGUF loader.\nA config for a GGUF loader.\n<code>NormalLoader</code> for a Gemma model.\n<code>VisionLoader</code> for an Idefics 2 Vision model.\nA device mapper which does device mapping per hidden layer.\nA value of type <code>L</code>.\n<code>NormalLoader</code> for a Llama model.\nThe <code>Loader</code> trait abstracts the loading process. The …\nA builder for a loader using the selected model.\nAll local paths and metadata necessary to load a model.\nLogprobs per token.\nSelect a LoRA architecture\nSelect a GGML model with LoRA.\nSelect a GGUF model with LoRA.\nThe MistralRs struct handles sending requests to the …\nThe MistralRsBuilder takes the pipeline and a scheduler …\nDType for the model.\nThe kind of model to build.\n<code>ModelPaths</code> abstracts the mechanism to get all necessary …\nA loader for a “normal” (non-quantized) model.\nA builder for a loader for a “normal” (non-quantized) …\nThe architecture to load the normal model as.\nA normal request request to the <code>MistralRs</code>\nConfig specific to loading a normal model.\nContains the success value\nAdapter model ordering information.\n<code>NormalLoader</code> for a Phi 2 model.\n<code>NormalLoader</code> for a Phi 3 model.\n<code>VisionLoader</code> for a Phi 3 Vision model.\nSelect a plain model, without quantization or adapters\n<code>NormalLoader</code> for a Qwen 2 model.\nA request to the Engine, encapsulating the various …\nMessage or messages for a <code>Request</code>.\nThe response enum contains 3 types of variants:\nA logprob with the top logprobs for this token.\nChat completion response message.\nA value of type <code>R</code>.\nSampling params are used to control sampling.\nThe scheduler method controld how sequences are scheduled …\nMetadata for a speculative pipeline\nA loader for a speculative pipeline using 2 <code>Loader</code>s.\nSpeculative decoding pipeline: …\nStop sequences or ids.\nTerminate all sequences on the next scheduling step. Be …\nThe source of the HF token.\nSelect the model from a toml file\nTop-n logprobs element\nType which can be converted to a DType\nOpenAI compatible (superset) usage during a request.\nA loader for a vision (non-quantized) model.\nA builder for a loader for a vision (non-quantized) model.\nThe architecture to load the vision model as.\nSelect a vision plain model, without quantization or …\nConfig specific to loading a vision model.\nSelect an X-LoRA architecture\nSelect a GGML model with X-LoRA.\nSelect a GGUF model with X-LoRA.\nString representation for dtypes.\nThe block size, i.e. the number of elements stored in each …\nJinja format chat templating for chat completion.\nThe block dtype\nA device mapper to not map device.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nγ completions to run of the draft model\nConfiguration of optional adapters. <code>(String, String)</code> is of …\nOptional adapter files. <code>(String, PathBuf)</code> is of the form …\n<code>XLoraConfig</code> for the XLORA classifier\nFilepath for the XLORA classifier\nRetrieve the <code>PretrainedConfig</code> file.\nFilepath for general model configuration.\nInformation for preloading LoRA adapters (adapter name, …\nReturn the defined ordering of adapters and layers within …\nGet the preprocessor config (for the vision models). This …\nGet the processor config (for the vision models). This is …\nFile where the content is expected to deserialize to …\nA serialised <code>tokenizers.Tokenizer</code> HuggingFace object.\nModel weights files (multiple files supported).\nThis should be called to initialize the debug flag and …\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nIf <code>revision</code> is None, then it defaults to <code>main</code>. If <code>dtype</code> is …\nLoad a model from the specified paths. Also initializes …\nCreate a loader builder for a GGUF model. <code>tok_model_id</code> is …\nThe size used by each element in bytes, i.e. 1 for <code>U8</code>, 4 …\nThe type size for blocks in bytes.\nModel ID to load LoRA from. This may be a HF hub repo or a …\nModel ID to load LoRA from. This may be a HF hub repo or a …\nModel ID to load LoRA from. This may be a HF hub repo or a …\nThe architecture of the model.\nThe architecture of the model.\nThe architecture of the model.\nThe architecture of the model.\nModel data type. Defaults to <code>auto</code>.\nModel data type. Defaults to <code>auto</code>.\nModel data type. Defaults to <code>auto</code>.\nModel data type. Defaults to <code>auto</code>.\n.toml file containing the selector configuration.\nGQA value\nGQA value\nGQA value\nModel ID to load from. This may be a HF hub repo or a …\nForce a base model ID to load from instead of using the …\nForce a base model ID to load from instead of using the …\nModel ID to load from. This may be a HF hub repo or a …\nOrdering JSON file\nOrdering JSON file\nOrdering JSON file\nOrdering JSON file\nOrdering JSON file\nOrdering JSON file\nQuantized filename, only applicable if <code>quantized</code> is set.\nQuantized filename, only applicable if <code>quantized</code> is set.\nQuantized filename, only applicable if <code>quantized</code> is set.\nQuantized filename, only applicable if <code>quantized</code> is set.\nQuantized filename, only applicable if <code>quantized</code> is set.\nQuantized filename, only applicable if <code>quantized</code> is set.\nQuantized model ID to find the <code>quantized_filename</code>, only …\nQuantized model ID to find the <code>quantized_filename</code>, only …\nQuantized model ID to find the <code>quantized_filename</code>, only …\nQuantized model ID to find the <code>quantized_filename</code>, only …\nQuantized model ID to find the <code>quantized_filename</code>, only …\nQuantized model ID to find the <code>quantized_filename</code>, only …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nControl the application of repeat penalty for the last n …\nIndex of completion tokens to generate scalings up until. …\nIndex of completion tokens to generate scalings up until. …\nIndex of completion tokens to generate scalings up until. …\n<code>tok_model_id</code> is the local or remote model ID where you can …\n<code>tok_model_id</code> is the local or remote model ID where you can …\n<code>tok_model_id</code> is the local or remote model ID where you can …\nModel ID to load the tokenizer from. This may be a HF hub …\nModel ID to load the tokenizer from. This may be a HF hub …\nModel ID to load the tokenizer from. This may be a HF hub …\nPath to local tokenizer.json file. If this is specified it …\nPath to local tokenizer.json file. If this is specified it …\nPath to local tokenizer.json file. If this is specified it …\nPath to local tokenizer.json file. If this is specified it …\nPath to local tokenizer.json file. If this is specified it …\nPath to local tokenizer.json file. If this is specified it …\nPath to local tokenizer.json file. If this is specified it …\nModel ID to load X-LoRA from. This may be a HF hub repo or …\nModel ID to load X-LoRA from. This may be a HF hub repo or …\nModel ID to load X-LoRA from. This may be a HF hub repo or …\nLinear layer with fused bias matmul.\nMatrix multiplcation, configurable to be via f16 (to use …\nRoPE supporting LongRope\nExpands a mask from (bs, seq_len) to (bs, 1, tgt_len, …\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nReturns the argument unchanged.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCalls <code>U::from(self)</code>.\nCompute matrix-matrix product, optionally casting to f16 …\nCompute matrix-matrix product, optionally casting to f16 …\nCompute quantized matrix-matrix product, optionally …\nComputes softmax(QK^T*sqrt(d_k))V")
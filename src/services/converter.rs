use crate::models::{
    ChatCompletionRequest, ChatFunction, ChatMessage, ChatTool, ContentPart, ResponseContent,
    ResponseInput, ResponseInputItem, ResponseRequest, StreamOptions,
};
use serde_json::{json, Value};
use std::collections::VecDeque;

/// Convert OpenAI Responses API request to Chat Completions format
pub fn convert_to_chat_completions(
    req: &ResponseRequest,
    supports_native_tools: bool,
) -> Result<ChatCompletionRequest, String> {
    let model = req.model.as_ref().ok_or("Model is required")?.clone();

    let mut messages = Vec::new();

    // Prepare tool overrides
    let native_tool_override = "\n\n---\n\nIMPORTANT: Tool Calling Format Override\n\
When calling functions/tools, you MUST use the standard OpenAI Chat Completions JSON format, NOT any XML or custom syntax. \
The system will automatically handle tool execution. Never output tool calls as text - use the native function calling mechanism.";

    let xml_tool_override = "\n\n---\n\nIMPORTANT: Tool Calling Format Override\n\
To call a function, you MUST use the following XML format:\n\
<function=function_name>\n\
<parameter=param_name>value</parameter>\n\
...\n\
</function>\n\
\n\
Do not use JSON tool calls. Use the XML format above.";

    let file_ops_guidance = "\n\nFile Operation Best Practices:\n\
- Use relative paths (e.g. 'test.py', 'src/main.rs') for files in the workspace\n\
- Read each file ONCE before editing - do not re-read files you've already successfully read\n\
- After receiving file contents from read_file, proceed directly to editing without redundant reads\n\
- For apply_patch, include 3-5 lines of surrounding context for reliable matching\n\
- Never announce \"I will read the file\" after you've already read it - just use the content you received";

    // Determine which instructions to use
    let mut system_instructions = req.instructions.clone().unwrap_or_default();

    // Only append overrides if tools are actually present or requested
    if req.tools.is_some() {
        if supports_native_tools {
            system_instructions.push_str(native_tool_override);
        } else {
            system_instructions.push_str(xml_tool_override);
        }
        // Append general guidance
        system_instructions.push_str(file_ops_guidance);
    }

    // Add instructions as system message if not empty
    if !system_instructions.is_empty() {
        messages.push(ChatMessage {
            role: "system".to_string(),
            content: Some(json!(system_instructions)),
            tool_calls: None,
            tool_call_id: None,
        });
    }

    // Passthrough messages if provided (hybrid Chat Completions compatibility)
    // This allows advanced users to send pre-formatted messages while using the Responses endpoint
    if let Some(req_messages) = &req.messages {
        log::debug!(
            "📨 Processing {} pre-formatted messages (hybrid mode)",
            req_messages.len()
        );
        for msg in req_messages {
            if let Ok(chat_msg) = serde_json::from_value::<ChatMessage>(msg.clone()) {
                messages.push(normalize_chat_message_role(chat_msg));
            }
        }
    }

    // Convert input to messages
    if let Some(input) = &req.input {
        match input {
            ResponseInput::String(text) => {
                messages.push(ChatMessage {
                    role: "user".to_string(),
                    content: Some(json!(text)),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            ResponseInput::Array(items) => {
                let mut accumulated_reasoning: Vec<String> = Vec::new();
                let mut pending_tool_calls: Vec<Value> = Vec::new();
                let mut pending_generated_call_ids: VecDeque<String> = VecDeque::new();
                let mut generated_call_id_counter = 0usize;

                for item in items {
                    match item {
                        ResponseInputItem::Message {
                            role,
                            content,
                            tool_call_id,
                            attachments,
                            ..
                        } => {
                            if let Some(attached) = attachments {
                                if !attached.is_empty() {
                                    let file_ids: Vec<_> =
                                        attached.iter().map(|a| a.file_id.as_str()).collect();
                                    log::error!(
                                        "❌ Attachments are not supported in stateless mode (files: {:?})",
                                        file_ids
                                    );
                                    return Err("attachments_not_supported".to_string());
                                }
                            }

                            if role == "tool" {
                                flush_pending_tool_calls(&mut messages, &mut pending_tool_calls);

                                let call_id = tool_call_id.clone().ok_or_else(|| {
                                    log::error!("❌ Tool role message missing tool_call_id");
                                    "tool_message_missing_tool_call_id".to_string()
                                })?;
                                let call_id = normalize_tool_output_call_id(
                                    &call_id,
                                    &mut pending_generated_call_ids,
                                    &mut generated_call_id_counter,
                                );

                                let tool_payload = extract_tool_message_body(content)?;

                                messages.push(ChatMessage {
                                    role: "tool".to_string(),
                                    content: Some(json!(tool_payload)),
                                    tool_calls: None,
                                    tool_call_id: Some(call_id),
                                });

                                continue;
                            }

                            let (mut msg_content, content_reasoning) =
                                convert_response_content(content)?;

                            // If content has inline reasoning, accumulate it
                            if let Some(content_think) = content_reasoning {
                                accumulated_reasoning.push(content_think);
                            }

                            // If assistant message and we have accumulated reasoning, prepend as <think> tags
                            if role == "assistant" && !accumulated_reasoning.is_empty() {
                                let thinking_text = accumulated_reasoning.join("\n");
                                let original_content = msg_content.as_str().unwrap_or("");
                                let combined = format!(
                                    "<think>{}</think>\n{}",
                                    thinking_text, original_content
                                );
                                msg_content = json!(combined);
                                log::info!("🧠 INPUT: Prepended {} reasoning part(s) ({} chars) to assistant message as <think> tags", 
                                    accumulated_reasoning.len(), thinking_text.len());
                                accumulated_reasoning.clear();
                            }

                            let chat_role = normalize_chat_role(role);

                            // If assistant message and we have pending tool calls, add them to the message
                            if role == "assistant" && !pending_tool_calls.is_empty() {
                                log::info!(
                                    "🔧 Added {} tool call(s) to assistant message",
                                    pending_tool_calls.len()
                                );
                                messages.push(ChatMessage {
                                    role: chat_role.clone(),
                                    content: Some(msg_content),
                                    tool_calls: Some(std::mem::take(&mut pending_tool_calls)),
                                    tool_call_id: None,
                                });
                            } else {
                                messages.push(ChatMessage {
                                    role: chat_role,
                                    content: Some(msg_content),
                                    tool_calls: None,
                                    tool_call_id: None,
                                });
                            }
                        }
                        ResponseInputItem::FunctionCall {
                            call_id,
                            name,
                            arguments,
                        } => {
                            let call_id = normalize_tool_call_id(
                                call_id,
                                &mut pending_generated_call_ids,
                                &mut generated_call_id_counter,
                            );
                            // Accumulate tool calls to attach to the next assistant message
                            pending_tool_calls.push(json!({
                                "id": call_id,
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": arguments,
                                }
                            }));
                            log::info!("🔧 INPUT: Found function_call ({}) - will attach to assistant message", name);
                        }
                        ResponseInputItem::FunctionCallOutput { call_id, output } => {
                            // The output field is a string that may contain nested JSON from Codex
                            // (e.g., {"output":"...", "metadata":{...}}). Try to extract the actual
                            // output content, otherwise use the raw string.
                            let content_str = if let Ok(parsed) =
                                serde_json::from_str::<serde_json::Value>(output)
                            {
                                if let Some(inner_output) =
                                    parsed.get("output").and_then(|v| v.as_str())
                                {
                                    inner_output.to_string()
                                } else {
                                    // Fallback to the full JSON string
                                    output.clone()
                                }
                            } else {
                                // Already a plain string
                                output.clone()
                            };
                            flush_pending_tool_calls(&mut messages, &mut pending_tool_calls);
                            let call_id = normalize_tool_output_call_id(
                                call_id,
                                &mut pending_generated_call_ids,
                                &mut generated_call_id_counter,
                            );

                            messages.push(ChatMessage {
                                role: "tool".to_string(),
                                content: Some(json!(content_str)),
                                tool_calls: None,
                                tool_call_id: Some(call_id.clone()),
                            });
                            log::info!(
                                "🔧 INPUT: Added function_call_output (call_id: {}, {} bytes)",
                                call_id,
                                content_str.len()
                            );
                        }
                        ResponseInputItem::Reasoning {
                            text,
                            encrypted_content,
                        } => {
                            // Accumulate reasoning to prepend to next assistant message
                            if let Some(reasoning_text) = text {
                                accumulated_reasoning.push(reasoning_text.clone());
                                log::info!("🧠 INPUT: Found reasoning item ({} chars), will prepend to next assistant message", reasoning_text.len());
                            } else if encrypted_content.is_some() {
                                log::warn!("⚠️  Encrypted reasoning content not supported (stateless mode), skipping");
                            }
                        }
                        ResponseInputItem::ItemReference { id } => {
                            log::warn!("⚠️  Item references (id: {}) are not supported in stateless mode, skipping", id);
                        }
                    }
                }

                // If reasoning items remain without an assistant message, log warning
                if !accumulated_reasoning.is_empty() {
                    log::warn!("⚠️  {} reasoning item(s) found but no following assistant message to attach to", accumulated_reasoning.len());
                }

                // If tool calls remain, we need to create an assistant message for them
                if !pending_tool_calls.is_empty() {
                    flush_pending_tool_calls(&mut messages, &mut pending_tool_calls);
                }
            }
        }
    }

    let response_format = req
        .text
        .as_ref()
        .and_then(|t| t.format.clone())
        .or_else(|| req.response_format.clone());

    // Handle logprobs - support both Responses API (top_logprobs) and Chat Completions (logprobs + top_logprobs)
    let (logprobs, top_logprobs) = match (req.logprobs, req.top_logprobs) {
        (_, Some(0)) => {
            log::warn!("⚠️ top_logprobs=0 requested - ignoring logprob request");
            (None, None)
        }
        (Some(true), tl) => (Some(true), tl.or(Some(5))), // Default to 5 if logprobs=true but no top_logprobs
        (_, Some(value)) => (Some(true), Some(value)),
        (None, None) => (None, None),
        (Some(false), None) => (None, None),
    };

    // Convert tools if provided - ONLY function tools are supported
    // Simply forward tools from the client; no injection needed
    let tools = if let Some(tools_vec) = req.tools.as_ref() {
        // Filter to only function tools; others are not supported in Chat Completions API
        let non_function_tools: Vec<_> = tools_vec
            .iter()
            .filter(|t| t.type_() != "function")
            .map(|t| t.type_())
            .collect();

        if !non_function_tools.is_empty() {
            log::debug!(
                "⚠️ Skipping non-function tools (not supported by Chat Completions API): {}",
                non_function_tools.join(", ")
            );
        }

        tools_vec
            .iter()
            .filter_map(|t| {
                if t.type_() == "function" {
                    let f = t.function_def();
                    Some(ChatTool::Function {
                        type_: "function".to_string(),
                        function: ChatFunction {
                            name: f.name.clone(),
                            description: f.description.clone(),
                            parameters: f.parameters.clone(),
                        },
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    // Tool injection has been removed. Codex CLI now properly sends all tools
    // (read_file, list_dir, grep_files, etc.) when experimental_supported_tools
    // is configured in the model family. The proxy simply forwards whatever
    // tools the client provides.

    let tools = if tools.is_empty() { None } else { Some(tools) };

    // Convert tool_choice to Value for backend
    let tool_choice = req.tool_choice.as_ref().map(|tc| {
        use crate::models::ToolChoice;
        match tc {
            ToolChoice::String(s) => json!(s),
            ToolChoice::Specific(spec) => json!(spec),
        }
    });
    let stream = req.stream.unwrap_or(false);
    let stream_options = chat_stream_options(req.stream_options.as_ref(), stream);

    Ok(ChatCompletionRequest {
        model,
        messages,
        max_tokens: req.max_output_tokens.or(req.max_tokens), // Support both field names
        temperature: req.temperature,
        top_p: req.top_p,
        response_format,
        tools,
        tool_choice,
        parallel_tool_calls: req.parallel_tool_calls,
        user: req.user.clone(),
        logprobs,
        top_logprobs,
        stream,
        stop: req.stop.clone(),
        frequency_penalty: req.frequency_penalty,
        presence_penalty: req.presence_penalty,
        seed: req.seed,
        logit_bias: req.logit_bias.clone(),
        metadata: req.metadata.clone(),
        service_tier: req.service_tier.clone(),
        store: req.store,
        n: req.n,
        stream_options,
        max_completion_tokens: req.max_completion_tokens,
        modalities: req.modalities.clone(),
        prediction: req.prediction.clone(),
        reasoning_effort: req.reasoning_effort.clone(),
        verbosity: req.verbosity.clone(),
        safety_identifier: req.safety_identifier.clone(),
        prompt_cache_key: req.prompt_cache_key.clone(),
        web_search_options: req.web_search_options.clone(),
        function_call: req.function_call.clone(),
        functions: req.functions.clone(),
    })
}

fn normalize_chat_message_role(mut message: ChatMessage) -> ChatMessage {
    message.role = normalize_chat_role(&message.role);
    message
}

fn normalize_chat_role(role: &str) -> String {
    match role {
        // OpenAI Responses uses `developer`; Ark Chat Completions rejects it.
        "developer" => "system".to_string(),
        other => other.to_string(),
    }
}

fn chat_stream_options(stream_options: Option<&StreamOptions>, stream: bool) -> Option<Value> {
    if !stream {
        return stream_options.and_then(|options| serde_json::to_value(options).ok());
    }

    let mut value = stream_options
        .and_then(|options| serde_json::to_value(options).ok())
        .unwrap_or_else(|| json!({}));
    match &mut value {
        Value::Object(map) => {
            map.insert("include_usage".to_string(), Value::Bool(true));
            Some(value)
        }
        _ => Some(json!({ "include_usage": true })),
    }
}

fn flush_pending_tool_calls(messages: &mut Vec<ChatMessage>, pending_tool_calls: &mut Vec<Value>) {
    if pending_tool_calls.is_empty() {
        return;
    }

    log::info!(
        "🔧 Added {} pending tool call(s) to synthetic assistant message",
        pending_tool_calls.len()
    );
    messages.push(ChatMessage {
        role: "assistant".to_string(),
        content: None,
        tool_calls: Some(std::mem::take(pending_tool_calls)),
        tool_call_id: None,
    });
}

fn normalize_tool_call_id(
    call_id: &str,
    pending_generated_call_ids: &mut VecDeque<String>,
    generated_call_id_counter: &mut usize,
) -> String {
    if !call_id.trim().is_empty() {
        return call_id.to_string();
    }

    let generated = next_generated_tool_call_id(generated_call_id_counter);
    pending_generated_call_ids.push_back(generated.clone());
    generated
}

fn normalize_tool_output_call_id(
    call_id: &str,
    pending_generated_call_ids: &mut VecDeque<String>,
    generated_call_id_counter: &mut usize,
) -> String {
    if !call_id.trim().is_empty() {
        return call_id.to_string();
    }

    pending_generated_call_ids
        .pop_front()
        .unwrap_or_else(|| next_generated_tool_call_id(generated_call_id_counter))
}

fn next_generated_tool_call_id(generated_call_id_counter: &mut usize) -> String {
    let id = format!("call_proxy_{}", generated_call_id_counter);
    *generated_call_id_counter += 1;
    id
}

/// Convert ResponseContent to JSON value for Chat Completions
/// Returns (content_value, extracted_reasoning_text)
fn convert_response_content(content: &ResponseContent) -> Result<(Value, Option<String>), String> {
    match content {
        ResponseContent::String(text) => Ok((json!(text), None)),
        ResponseContent::Array(parts) => {
            let mut reasoning_text = String::new();
            let mut text_parts: Vec<String> = Vec::new();
            let mut converted: Vec<Value> = Vec::new();

            for part in parts {
                match part {
                    ContentPart::InputText { text } | ContentPart::OutputText { text } => {
                        text_parts.push(text.clone());
                        converted.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                    ContentPart::ToolOutput { body, .. } => {
                        text_parts.push(body.clone());
                        converted.push(json!({
                            "type": "text",
                            "text": body
                        }));
                    }
                    ContentPart::InputImage { image_url } => {
                        converted.push(json!({
                            "type": "image_url",
                            "image_url": {
                                "url": image_url.url
                            }
                        }));
                    }
                    ContentPart::InputFile { .. } => {
                        let rendered = render_inline_file_part(part)?;
                        text_parts.push(rendered.clone());
                        converted.push(json!({
                            "type": "text",
                            "text": rendered
                        }));
                    }
                    ContentPart::Reasoning { text, .. } => {
                        // Reasoning within message content - accumulate for <think> tags
                        if !reasoning_text.is_empty() {
                            reasoning_text.push('\n');
                        }
                        reasoning_text.push_str(text);
                        log::info!(
                            "🧠 INPUT: Found reasoning in message content ({} chars)",
                            text.len()
                        );
                    }
                }
            }

            // If all text parts (no images), concatenate into string
            let has_images = parts
                .iter()
                .any(|p| matches!(p, ContentPart::InputImage { .. }));
            let has_reasoning = !reasoning_text.is_empty();

            if !has_images && !converted.is_empty() {
                let text = text_parts.join("\n");
                Ok((
                    json!(text),
                    if has_reasoning {
                        Some(reasoning_text)
                    } else {
                        None
                    },
                ))
            } else {
                Ok((
                    json!(converted),
                    if has_reasoning {
                        Some(reasoning_text)
                    } else {
                        None
                    },
                ))
            }
        }
    }
}

fn render_inline_file_part(part: &ContentPart) -> Result<String, String> {
    match part {
        ContentPart::InputFile {
            file_id,
            filename,
            file_url,
            file_data,
        } => {
            let label = filename
                .as_deref()
                .or(file_id.as_deref())
                .unwrap_or("input_file");

            if let Some(data) = file_data.as_ref().filter(|s| !s.trim().is_empty()) {
                return Ok(format!("[input_file:{label}]\n{data}"));
            }

            if let Some(url) = file_url.as_ref().filter(|s| !s.trim().is_empty()) {
                return Ok(format!("[input_file:{label}]\n{url}"));
            }

            Err("input_file_content_not_supported".to_string())
        }
        _ => Err("input_file_content_not_supported".to_string()),
    }
}

/// Extract tool role content into a plain string suitable for Chat Completions
fn extract_tool_message_body(content: &ResponseContent) -> Result<String, String> {
    match content {
        ResponseContent::String(text) => Ok(text.clone()),
        ResponseContent::Array(parts) => {
            let mut combined = String::new();

            for part in parts {
                match part {
                    ContentPart::InputText { text } | ContentPart::OutputText { text } => {
                        if !combined.is_empty() {
                            combined.push('\n');
                        }
                        combined.push_str(text);
                    }
                    ContentPart::ToolOutput { body, .. } => {
                        if !combined.is_empty() {
                            combined.push('\n');
                        }
                        combined.push_str(body);
                    }
                    other => {
                        log::error!(
                            "❌ Tool message content part not supported in proxy: {:?}",
                            other
                        );
                        return Err("tool_output_content_not_supported".to_string());
                    }
                }
            }

            if combined.is_empty() {
                Err("tool_output_empty".to_string())
            } else {
                Ok(combined)
            }
        }
    }
}

/// Translate Chat Completions finish_reason to Responses API status
pub fn translate_finish_reason(finish_reason: Option<&str>) -> &'static str {
    match finish_reason {
        Some("stop") => "completed",
        Some("length") => "incomplete",
        Some("content_filter") => "failed",
        Some("tool_calls") => "completed",
        Some(_) => "completed",
        None => "in_progress",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_file_data_is_preserved_as_text() {
        let content = ResponseContent::Array(vec![ContentPart::InputFile {
            file_id: None,
            filename: Some("notes.txt".to_string()),
            file_url: None,
            file_data: Some("hello world".to_string()),
        }]);

        let (converted, reasoning) = convert_response_content(&content).unwrap();
        assert!(reasoning.is_none());
        assert_eq!(converted, json!("[input_file:notes.txt]\nhello world"));
    }

    #[test]
    fn file_id_only_is_still_rejected() {
        let content = ResponseContent::Array(vec![ContentPart::InputFile {
            file_id: Some("file_123".to_string()),
            filename: None,
            file_url: None,
            file_data: None,
        }]);

        let err = convert_response_content(&content).unwrap_err();
        assert_eq!(err, "input_file_content_not_supported");
    }
}

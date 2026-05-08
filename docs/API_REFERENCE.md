# API Reference - OpenAI Responses Proxy

## Endpoints

### POST /v1/responses

Create a model response using the OpenAI Responses API format.

**Request Headers:**
- `Authorization: Bearer <api_key>` (required) - API key forwarded to Chutes.ai
- `Content-Type: application/json` (required)

**Request Body:**

```json
{
  "model": "gpt-4o",              // Required: Model name
  "input": "...",                 // Required: String or array of input items
  "instructions": "...",          // Optional: System instructions
  "max_output_tokens": 1024,     // Optional: Max tokens to generate
  "temperature": 0.7,            // Optional: Sampling temperature (0-2)
  "top_p": 1.0,                  // Optional: Nucleus sampling
  "tools": [...],                // Optional: Function calling tools
  "stream": true,                // Optional: Enable streaming (default: false)
  "metadata": {...},             // Optional: Custom metadata
  "store": false                 // Optional: Store response (not implemented)
}
```

**Input Formats:**

1. **Simple string:**
```json
{
  "input": "What is 2+2?"
}
```

2. **Structured messages:**
```json
{
  "input": [
    {
      "type": "message",
      "role": "user",
      "content": "What is 2+2?"
    }
  ]
}
```

3. **Multi-turn with content parts:**
```json
{
  "input": [
    {
      "type": "message",
      "role": "user",
      "content": [
        {
          "type": "input_text",
          "text": "What is in this image?"
        },
        {
          "type": "input_image",
          "image_url": {
            "url": "data:image/jpeg;base64,..."
          }
        }
      ]
    }
  ]
}
```

**Response (Streaming):**

Server-sent events with the following event types:

1. **response.created** - Response started
```json
{
  "type": "response.created",
  "response": {
    "id": "resp_abc123",
    "object": "response",
    "created_at": 1234567890,
    "status": "in_progress",
    "model": "gpt-4o",
    "output": [],
    "usage": null
  },
  "sequence_number": 1
}
```

2. **response.output_item.added** - Output item added
```json
{
  "type": "response.output_item.added",
  "item_id": "msg_abc123",
  "output_index": 0,
  "item": {
    "id": "msg_abc123",
    "type": "message",
    "status": "in_progress",
    "role": "assistant",
    "content": []
  },
  "sequence_number": 2
}
```

3. **response.content_part.added** - Content part added
```json
{
  "type": "response.content_part.added",
  "item_id": "msg_abc123",
  "output_index": 0,
  "content_index": 0,
  "sequence_number": 3
}
```

4. **response.output_text.delta** - Text delta (multiple events)
```json
{
  "type": "response.output_text.delta",
  "item_id": "msg_abc123",
  "output_index": 0,
  "content_index": 0,
  "delta": "Hello",
  "sequence_number": 4
}
```

5. **response.output_text.done** - Text complete
```json
{
  "type": "response.output_text.done",
  "item_id": "msg_abc123",
  "output_index": 0,
  "content_index": 0,
  "text": "Hello there!",
  "sequence_number": 10
}
```

6. **response.content_part.done** - Content part done
```json
{
  "type": "response.content_part.done",
  "item_id": "msg_abc123",
  "output_index": 0,
  "content_index": 0,
  "sequence_number": 11
}
```

7. **response.output_item.done** - Output item done
```json
{
  "type": "response.output_item.done",
  "item_id": "msg_abc123",
  "output_index": 0,
  "item": {
    "id": "msg_abc123",
    "type": "message",
    "status": "completed",
    "role": "assistant",
    "content": [
      {
        "type": "output_text",
        "text": "Hello there!",
        "annotations": []
      }
    ]
  },
  "sequence_number": 12
}
```

8. **response.completed** - Response complete
```json
{
  "type": "response.completed",
  "response": {
    "id": "resp_abc123",
    "object": "response",
    "created_at": 1234567890,
    "status": "completed",
    "model": "gpt-4o",
    "output": [
      {
        "id": "msg_abc123",
        "type": "message",
        "status": "completed",
        "role": "assistant",
        "content": [
          {
            "type": "output_text",
            "text": "Hello there!",
            "annotations": []
          }
        ]
      }
    ],
    "usage": {
      "input_tokens": 10,
      "output_tokens": 5,
      "total_tokens": 15,
      "input_tokens_details": {
        "cached_tokens": null
      },
      "output_tokens_details": {
        "reasoning_tokens": null
      }
    }
  },
  "sequence_number": 13
}
```

**Error Response:**

```json
{
  "type": "response.failed",
  "response": {
    "id": "resp_abc123",
    "status": "failed",
    "error": {
      "code": "backend_error",
      "message": "Error details..."
    }
  }
}
```

### GET /health

Health check endpoint with circuit breaker status.

**Response:**
```json
{
  "status": "healthy",
  "circuit_breaker": {
    "enabled": true,
    "is_open": false,
    "consecutive_failures": 0
  }
}
```

## Status Codes

- `200 OK` - Successful request
- `400 Bad Request` - Invalid request format
- `401 Unauthorized` - Missing or invalid API key
- `404 Not Found` - Model not found
- `503 Service Unavailable` - Circuit breaker open
- `502 Bad Gateway` - Backend connection failed

## Conversion Mapping

| Responses API | Chat Completions |
|---------------|------------------|
| `input` (string) | `messages[0].content` (user role) |
| `input` (array) | `messages` (mapped by role) |
| `instructions` | `messages[0]` (system role) |
| `max_output_tokens` | `max_tokens` |
| `temperature` | `temperature` |
| `top_p` | `top_p` |
| `tools` | `tools` |

**Event Mapping:**

| Chat Completions | Responses API |
|------------------|---------------|
| SSE chunk with `delta.content` | `response.output_text.delta` |
| `finish_reason: "stop"` | `status: "completed"` |
| `finish_reason: "length"` | `status: "incomplete"` |
| `error` object | `response.failed` |

## Limitations

- **Stateless only** - No conversation state storage
- **item_reference** - Not supported (logged and ignored)
- **store parameter** - Not implemented
- **Tool calls** - Basic support (needs enhancement for complex tools)
- **Image outputs** - Not supported (text only)

## Examples

See `tests/manual/` directory:
- `simple_request.sh` - Bash/curl example
- `python_client.py` - Python example
- `nodejs_client.js` - Node.js example
- `multi_turn.sh` - Multi-turn conversation
- `with_tools.sh` - Function calling example

## OpenAI Responses API Spec Compliance

## Summary

This document describes the compliance of the responses-proxy with the official OpenAI Responses API specification (`specs/openai-openapi.yml` v2.3.0, 66,009 lines).

**Last Updated:** November 4, 2025

---

## ✅ Implemented Features

### 1. **Complete Response Model Fields**

All required and optional fields from the official spec are now included in the `Response` object:

- `id`, `object`, `created_at`, `status` ✅
- `error`, `incomplete_details` ✅
- `model`, `output`, `usage`, `metadata` ✅
- **Echo Parameters** (NEW): `instructions`, `tools`, `tool_choice`, `parallel_tool_calls`, `temperature`, `top_p`, `max_output_tokens` ✅

### 2. **Tool Validation**

**Strict validation** now enforces that only `function` type tools are supported:

```rust
// In src/services/converter.rs
if tool.type_ != "function" {
    return Err(format!(
        "Unsupported tool type '{}'. Only 'function' tools are supported..."
    ));
}
```

**Why:** Chat Completions backends (like Chutes.ai) only support `function` tools. Advanced tools (`file_search`, `web_search`, `code_interpreter`, etc.) require native Responses API backends.

### 3. **Complete OutputItem Types**

Supports all output item types from the spec:

- `message` ✅
- `function_call` ✅
- `function_call_output` ✅ (with `output` field)
- `reasoning` ✅
- `refusal` ✅

### 4. **Complete OutputContent Types**

- `output_text` ✅
- `reasoning` ✅
- `refusal` ✅ (NEW)

### 5. **Incomplete Details**

Proper `incomplete_details` support with reason tracking:

```rust
incomplete_details: Some(IncompleteDetails {
    reason: "max_output_tokens" // or "content_filter"
})
```

### 6. **Streaming Events**

**Core Events Implemented (13):**

| Event Type | Status |
|------------|--------|
| `response.created` | ✅ |
| `response.output_item.added` | ✅ |
| `response.content_part.added` | ✅ |
| `response.output_text.delta` | ✅ |
| `response.output_text.done` | ✅ |
| `response.content_part.done` | ✅ |
| `response.output_item.done` | ✅ |
| `response.function_call_arguments.delta` | ✅ |
| `response.function_call_arguments.done` | ✅ |
| `response.reasoning_text.delta` | ✅ |
| `response.reasoning_text.done` | ✅ |
| `response.completed` | ✅ |
| `response.failed` | ✅ |

All events now properly include the `error` field (even when `None`).

---

## ⚠️ Known Limitations (By Design)

### Unsupported Features (Due to Stateless Chat Completions Backend)

The following features from the full Responses API spec are **intentionally not supported** because they require:
1. Native Responses API backends (not Chat Completions)
2. Stateful storage (Redis/DB)
3. Advanced capabilities beyond function calling

**Advanced Tool Events (38 events):**
- ❌ Audio: `response.audio.*`
- ❌ Code Interpreter: `response.code_interpreter_call.*`
- ❌ File Search: `response.file_search_call.*`
- ❌ Web Search: `response.web_search_call.*`
- ❌ Image Gen: `response.image_gen_call.*`
- ❌ MCP (Model Context Protocol): `response.mcp_call.*`
- ❌ Custom Tools: `response.custom_tool_call.*`
- ❌ Reasoning Summary: `response.reasoning_summary.*`
- ❌ Other: `response.queued`, `response.in_progress`, `response.error` (during stream)

**Request Parameters:**
- ❌ `background` - async processing
- ❌ `previous_response_id` - stateful conversations
- ❌ `conversation` - conversation tracking
- ❌ `modalities` - audio/vision modes
- ❌ `audio` - audio configuration
- ❌ `truncation` - context truncation strategies
- ❌ `reasoning.effort` - o-series model control
- ❌ `include` - optional data inclusion

**Partial support:**
- ⚠️ `input_file` with inline `file_data` or `file_url` is preserved as text best-effort payload
- ❌ `input_file` with only `file_id` remains unsupported

**Why These Limitations Exist:**

This proxy translates Responses API → Chat Completions API → Responses API for **stateless operation** with standard backends like:
- Chutes.ai
- OpenAI Chat Completions
- Azure OpenAI
- Any OpenAI-compatible API

These backends **only support**:
- Text generation
- Function calling
- Reasoning models (via `reasoning_content`)

They do **not support**:
- Advanced tool execution
- Stateful conversations
- Audio/multimodal responses

---

## 🎯 What This Proxy Does Best

### ✅ Fully Supported Use Cases:

1. **Text Generation**
   - User messages → Assistant responses
   - System instructions
   - Multi-turn conversations (via input history)
   - Streaming responses

2. **Function Calling**
   - Tool definitions
   - Tool calls from model
   - Tool results back to model
   - Parallel tool calls
   - Multi-turn function conversations

3. **Reasoning Models**
   - DeepSeek-R1, OpenAI o-series
   - Reasoning content extraction
   - `<think>` tag support
   - Reasoning items in output

4. **Request Echo**
   - All request parameters echoed back in response
   - Maintains request context for clients

---

## 📊 Compliance Matrix

### Request Parameters

| Parameter | Supported | Notes |
|-----------|-----------|-------|
| `model` | ✅ | Required |
| `input` | ✅ | String or array of items |
| `instructions` | ✅ | Maps to system message |
| `max_output_tokens` | ✅ | Mapped to `max_tokens` |
| `temperature` | ✅ | Pass-through |
| `top_p` | ✅ | Pass-through |
| `tools` | ⚠️ | **Only `function` type** |
| `tool_choice` | ✅ | Pass-through |
| `parallel_tool_calls` | ✅ | Pass-through |
| `stream` | ✅ | SSE streaming |
| `metadata` | ✅ | Pass-through |
| `store` | ⚠️ | Accepted but ignored (stateless) |
| `background` | ❌ | Not supported |
| `previous_response_id` | ❌ | Not supported (stateless) |
| `conversation` | ❌ | Not supported |
| `include` | ❌ | Not supported |
| `modalities` | ❌ | Not supported |
| `audio` | ❌ | Not supported |
| `truncation` | ❌ | Not supported |

### Response Fields

| Field | Supported | Notes |
|-------|-----------|-------|
| `id` | ✅ | Generated |
| `object` | ✅ | Always "response" |
| `created_at` | ✅ | Unix timestamp |
| `status` | ✅ | completed/failed/incomplete |
| `error` | ✅ | Error details |
| `incomplete_details` | ✅ | Reason for incomplete |
| `model` | ✅ | Echoed back |
| `output` | ✅ | Array of output items |
| `usage` | ✅ | Token counts |
| `metadata` | ✅ | Echoed back |
| `instructions` | ✅ | Echoed back |
| `tools` | ✅ | Echoed back |
| `tool_choice` | ✅ | Echoed back |
| `parallel_tool_calls` | ✅ | Echoed back |
| `temperature` | ✅ | Echoed back |
| `top_p` | ✅ | Echoed back |
| `max_output_tokens` | ✅ | Echoed back |

---

## 🔧 Architecture

```
┌─────────────────────┐
│  Client             │
│  (Responses API)    │
└──────────┬──────────┘
           │
           │ POST /responses
           │ (Responses API format)
           ▼
┌─────────────────────┐
│  Responses Proxy    │
│  - Validate tools   │
│  - Convert request  │
│  - Map events       │
│  - Echo params      │
└──────────┬──────────┘
           │
           │ POST /chat/completions
           │ (Chat Completions format)
           ▼
┌─────────────────────┐
│  Backend            │
│  - Chutes.ai        │
│  - OpenAI           │
│  - Azure OpenAI     │
└──────────┬──────────┘
           │
           │ SSE Stream
           ▼
┌─────────────────────┐
│  Responses Proxy    │
│  - Parse chunks     │
│  - Generate events  │
│  - Track state      │
└──────────┬──────────┘
           │
           │ SSE Stream
           │ (Responses API events)
           ▼
┌─────────────────────┐
│  Client             │
│  (Receives events)  │
└─────────────────────┘
```

---

## 📝 Error Messages

### Tool Validation Error

When a non-function tool is used:

```json
{
  "error": "Unsupported tool type 'file_search'. Only 'function' tools are supported when translating to Chat Completions API. Advanced tool types (file_search, web_search, code_interpreter, etc.) require native Responses API backends."
}
```

### Backend Error

When backend returns error:

```json
{
  "type": "response.failed",
  "response": {
    "status": "failed",
    "error": {
      "code": "backend_error",
      "message": "..."
    }
  }
}
```

---

## 🚀 Usage Example

```bash
curl https://api.yourserver.com/responses \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $YOUR_API_KEY" \
  -d '{
    "model": "gpt-4",
    "input": "Tell me a joke",
    "instructions": "Be funny and concise",
    "tools": [{
      "type": "function",
      "function": {
        "name": "get_weather",
        "parameters": { "type": "object", "properties": {} }
      }
    }],
    "stream": true
  }'
```

**Response Stream:**

```
data: {"type":"response.created","response":{...,"instructions":"Be funny and concise","tools":[...]}}
data: {"type":"response.output_item.added",...}
data: {"type":"response.content_part.added",...}
data: {"type":"response.output_text.delta","delta":"Why"}
data: {"type":"response.output_text.delta","delta":" did"}
data: {"type":"response.output_text.done","text":"Why did..."}
data: {"type":"response.completed","response":{...}}
```

---

## 📚 References

- **Official Spec**: `specs/openai-openapi.yml` (66,009 lines, OpenAPI 3.1.0)
- **Source**: https://app.stainless.com/api/spec/documented/openai/openapi.documented.yml
- **GitHub**: https://github.com/openai/openai-openapi
- **Docs**: https://platform.openai.com/docs/api-reference/responses

---

## ✨ Summary

This proxy provides **complete compliance** with the core Responses API for:
- ✅ Text generation
- ✅ Function calling
- ✅ Reasoning models
- ✅ Streaming events
- ✅ Parameter echo
- ✅ Error handling
- ✅ Tool validation

With clear **documented limitations** for features that require native Responses API backends or stateful storage.

**Perfect for:** Translating Responses API clients to work with any OpenAI-compatible Chat Completions backend.

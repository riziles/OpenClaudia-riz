/**
 * OpenClaudia Web GUI — API client module.
 *
 * Communicates with the OpenClaudia proxy server via the
 * `/v1/chat/completions` endpoint. Handles SSE streaming,
 * request construction, and error propagation.
 *
 * @module api
 */

/**
 * @typedef {Object} ChatRequest
 * @property {string} model - Model name (e.g. "deepseek-v4-pro")
 * @property {Array<{role: string, content: string}>} messages - Conversation messages
 * @property {boolean} [stream=true] - Whether to stream the response
 */

/**
 * @typedef {Object} SseEvent
 * @property {'text'|'thinking'|'tool_start'|'tool_done'|'error'|'done'} type - Event type
 * @property {string} [content] - Text content (for text/thinking/error events)
 * @property {string} [tool_name] - Tool name (for tool_start/tool_done events)
 * @property {boolean} [success] - Whether tool succeeded (for tool_done)
 * @property {string} [tool_content] - Tool output (for tool_done)
 */

/**
 * Resolve the default model from the `<meta name="default-model">` tag
 * injected by the proxy at request time.
 *
 * @returns {string} The default model name
 */
function getDefaultModel() {
  const meta = document.querySelector('meta[name="default-model"]');
  return meta?.content || '';
}

/**
 * Send a chat completion request to the proxy and stream SSE events.
 *
 * Uses `fetch` with a `ReadableStream` to parse the SSE response.
 * Calls `onEvent` for each parsed event and `onDone` when the stream ends.
 * Returns an `AbortController` the caller can use to cancel.
 *
 * @param {string} prompt - The user's input text
 * @param {ChatRequest['messages']} history - Prior conversation messages
 * @param {function(SseEvent): void} onEvent - Called for each SSE event
 * @param {function(): void} onDone - Called when the stream completes
 * @param {function(string): void} onError - Called on fatal errors
 * @returns {AbortController}
 */
export function sendChatRequest(prompt, history, onEvent, onDone, onError) {
  const controller = new AbortController();
  const messages = [
    ...history,
    { role: 'user', content: prompt }
  ];

  const body = JSON.stringify({
    model: getDefaultModel(),
    messages,
    stream: true
  });

  fetch('/v1/chat/completions', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body,
    signal: controller.signal
  })
    .then(response => {
      if (!response.ok) {
        return response.text().then(text => {
          throw new Error(`HTTP ${response.status}: ${text.slice(0, 500)}`);
        });
      }
      return readSseStream(response, onEvent);
    })
    .then(() => onDone())
    .catch(err => {
      if (err.name === 'AbortError') return;
      onError(err.message || 'Unknown error');
    });

  return controller;
}

/**
 * Read an SSE (`text/event-stream`) response body, parsing `data:` lines
 * into JSON objects and dispatching them via `onEvent`.
 *
 * Handles multi-line data (where an event spans multiple `data:` lines by
 * joining them with newlines). Empty lines delimit event boundaries.
 * Non-JSON `data:` lines are treated as plain text events.
 *
 * @param {Response} response - Fetch Response with a streaming body
 * @param {function(SseEvent): void} onEvent - Called for each parsed event
 * @returns {Promise<void>}
 */
async function readSseStream(response, onEvent) {
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });
    const lines = buffer.split('\n');
    // Keep the last (possibly incomplete) line in the buffer
    buffer = lines.pop() || '';

    let dataLines = [];
    for (const line of lines) {
      if (line.startsWith('data: ')) {
        dataLines.push(line.slice(6));
      } else if (line.trim() === '' && dataLines.length > 0) {
        // Empty line = event boundary — flush accumulated data
        const data = dataLines.join('\n');
        dataLines = [];
        parseAndDispatch(data, onEvent);
      }
      // Ignore `event:` and `id:` lines — we don't use them
    }
  }

  // Flush any remaining data on stream end
  if (buffer.startsWith('data: ')) {
    parseAndDispatch(buffer.slice(6), onEvent);
  }
}

/**
 * Parse an SSE data payload and dispatch the appropriate event.
 *
 * Handles the structured SSE format produced by the proxy:
 * - JSON objects with known keys (`choices[0].delta.content`, etc.)
 * - Raw text ending with `[DONE]` marker
 * - Plain text tool results
 *
 * @param {string} data - Raw data payload from an SSE `data:` line
 * @param {function(SseEvent): void} onEvent - Event dispatcher
 */
function parseAndDispatch(data, onEvent) {
  if (data === '[DONE]') {
    onEvent({ type: 'done' });
    return;
  }

  // Try JSON parse for structured events
  try {
    const parsed = JSON.parse(data);

    // OpenAI streaming format: choices[0].delta.content
    const delta = parsed?.choices?.[0]?.delta;
    if (delta?.content) {
      onEvent({ type: 'text', content: delta.content });
      return;
    }

    // Tool calls in delta
    if (delta?.tool_calls) {
      for (const tc of delta.tool_calls) {
        if (tc.function?.name) {
          onEvent({ type: 'tool_start', tool_name: tc.function.name });
        }
      }
      return;
    }

    // Non-streaming response
    const message = parsed?.choices?.[0]?.message;
    if (message?.content) {
      onEvent({ type: 'text', content: message.content });
      return;
    }

    // Pass through unknown JSON as text for debugging
    if (parsed.content || parsed.text) {
      onEvent({ type: 'text', content: parsed.content || parsed.text });
      return;
    }
    return;
  } catch {
    // Not JSON — treat as plain text
  }

  // Plain text
  onEvent({ type: 'text', content: data });
}

/**
 * Fetch the list of available models from the proxy.
 *
 * @returns {Promise<Array<{id: string}>>}
 */
export async function fetchModels() {
  const res = await fetch('/v1/models');
  if (!res.ok) throw new Error(`Failed to fetch models: ${res.status}`);
  const data = await res.json();
  return data.data || [];
}

/**
 * Check proxy health.
 *
 * @returns {Promise<{status: string, service: string, version: string}>}
 */
export async function healthCheck() {
  const res = await fetch('/health');
  if (!res.ok) throw new Error(`Health check failed: ${res.status}`);
  return res.json();
}

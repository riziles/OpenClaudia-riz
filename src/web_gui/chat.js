/**
 * OpenClaudia Web GUI — Chat rendering module.
 *
 * Manages the chat message list DOM with browser-native virtualization
 * via `content-visibility: auto`. Handles streaming text, tool results,
 * thinking indicators, and auto-scroll behavior.
 *
 * @module chat
 */

/**
 * @typedef {Object} Message
 * @property {'user'|'assistant'|'thinking'|'tool'|'error'} role
 * @property {string} content - Display text
 * @property {string} [toolName] - Tool name (for tool messages)
 * @property {string} [id] - Unique identifier
 */

/**
 * Manages the chat message list and streaming state.
 */
export class ChatRenderer {
  /** @type {HTMLElement} */
  #container;

  /** @type {HTMLElement|null} */
  #streamingEl = null;

  /** @type {string} */
  #streamingBuffer = '';

  /** @type {number} */
  #messageCount = 0;

  /** @type {boolean} */
  #userScrolledUp = false;

  /** @type {IntersectionObserver|null} */
  #scrollSentinel = null;

  /**
   * @param {HTMLElement} container - The scrollable chat container element
   */
  constructor(container) {
    this.#container = container;
    this.#setupScrollDetection();
    this.#showEmptyState();
  }

  /**
   * Detect when the user manually scrolls up (away from the bottom)
   * so we can suppress auto-scroll temporarily.
   */
  #setupScrollDetection() {
    // Create a sentinel element pinned to the bottom
    const sentinel = document.createElement('div');
    sentinel.className = 'chat__sentinel';
    sentinel.style.height = '1px';
    this.#container.appendChild(sentinel);

    this.#scrollSentinel = new IntersectionObserver(
      ([entry]) => {
        // When the sentinel is NOT intersecting, the user has scrolled up
        this.#userScrolledUp = !entry.isIntersecting;
      },
      { root: this.#container, threshold: 0 }
    );
    this.#scrollSentinel.observe(sentinel);
  }

  /**
   * Scroll to the bottom of the chat, but only if the user hasn't
   * manually scrolled up to read older messages.
   */
  #scrollToBottomIfNeeded() {
    if (!this.#userScrolledUp) {
      this.#container.scrollTop = this.#container.scrollHeight;
    }
  }

  /**
   * Show the "send a message to start" placeholder.
   */
  #showEmptyState() {
    this.#container.innerHTML = `
      <div class="chat__empty">
        <div>
          <div style="font-size:32px;margin-bottom:12px;">&#8203;</div>
          <div>Send a message to start</div>
        </div>
      </div>`;
  }

  /**
   * Remove the empty state placeholder (called before first message).
   */
  #clearEmptyState() {
    const empty = this.#container.querySelector('.chat__empty');
    if (empty) empty.remove();
  }

  /**
   * Add a new message bubble to the chat.
   *
   * @param {Message} msg - The message to render
   * @returns {HTMLElement} The created message element
   */
  addMessage(msg) {
    this.#clearEmptyState();
    this.#messageCount++;

    const el = document.createElement('div');
    el.className = `message message--${msg.role}`;
    el.dataset.msgId = String(this.#messageCount);

    const inner = document.createElement('div');
    inner.className = 'message__inner';
    inner.textContent = msg.content;
    el.appendChild(inner);

    if (msg.toolName) {
      const meta = document.createElement('div');
      meta.className = 'message__meta';
      meta.textContent = msg.toolName;
      el.prepend(meta);
    }

    this.#container.appendChild(el);
    this.#scrollToBottomIfNeeded();

    return el;
  }

  /**
   * Start or continue a streaming assistant response.
   *
   * On the first call, creates a new streaming message bubble.
   * Subsequent calls append text to the same bubble.
   *
   * @param {string} text - Text delta to append
   */
  appendStreamingText(text) {
    if (!this.#streamingEl) {
      this.#clearEmptyState();
      this.#streamingBuffer = '';
      this.#messageCount++;

      this.#streamingEl = document.createElement('div');
      this.#streamingEl.className = 'message message--assistant';
      this.#streamingEl.dataset.msgId = String(this.#messageCount);

      const inner = document.createElement('div');
      inner.className = 'message__inner';
      this.#streamingEl.appendChild(inner);

      this.#container.appendChild(this.#streamingEl);
    }

    this.#streamingBuffer += text;
    this.#streamingEl.querySelector('.message__inner').textContent =
      this.#streamingBuffer;
    this.#scrollToBottomIfNeeded();
  }

  /**
   * Finalize the current streaming message.
   *
   * Removes the streaming state so the next `appendStreamingText`
   * call starts a new message. Safe to call when not streaming.
   */
  finishStreaming() {
    this.#streamingEl = null;
    this.#streamingBuffer = '';
  }

  /**
   * Show a thinking indicator (e.g. "Thinking..." or "Thought for Xs").
   *
   * @param {string} text - Thinking status text
   * @returns {HTMLElement} The thinking element (auto-removes on next non-thinking message)
   */
  showThinking(text) {
    this.#clearEmptyState();
    const el = document.createElement('div');
    el.className = 'message message--thinking';

    const inner = document.createElement('div');
    inner.className = 'message__inner';
    inner.textContent = text;
    el.appendChild(inner);

    this.#container.appendChild(el);
    this.#scrollToBottomIfNeeded();
    return el;
  }

  /**
   * Show a stream-in-progress indicator (pulsing dot).
   *
   * @returns {HTMLElement} The indicator element
   */
  showStreamIndicator() {
    const el = document.createElement('div');
    el.className = 'stream-indicator';

    const pulse = document.createElement('span');
    pulse.className = 'stream-indicator__pulse';
    el.appendChild(pulse);

    const label = document.createElement('span');
    label.textContent = 'Thinking';
    el.appendChild(label);

    this.#container.appendChild(el);
    return el;
  }

  /**
   * Remove the stream indicator element.
   *
   * @param {HTMLElement} el - The indicator element to remove
   */
  removeStreamIndicator(el) {
    el.remove();
  }

  /**
   * Clear all messages and reset to empty state.
   */
  clear() {
    this.#container.innerHTML = '';
    this.#streamingEl = null;
    this.#streamingBuffer = '';
    this.#messageCount = 0;
    this.#showEmptyState();
  }

  /**
   * Get the current number of rendered messages.
   *
   * @returns {number}
   */
  get messageCount() {
    return this.#messageCount;
  }
}

/**
 * OpenClaudia Web GUI — Application entry point.
 *
 * Wires together the ChatRenderer, API client, theme toggle,
 * and input handling. Manages the conversation state and
 * orchestrates the request/response lifecycle.
 *
 * @module app
 */

import { sendChatRequest, healthCheck } from './api.js';
import { ChatRenderer } from './chat.js';
import { CommandPalette } from './palette.js';

/**
 * @typedef {Object} AppState
 * @property {ChatRenderer} chat - Chat renderer instance
 * @property {Array<{role: string, content: string}>} history - Conversation messages
 * @property {AbortController|null} activeRequest - Controller for the in-flight request
 * @property {boolean} connected - Whether the proxy is reachable
 * @property {string} theme - Current theme: 'light', 'dark', or 'auto'
 */

/** @type {AppState} */
const state = {
  chat: null,
  history: [],
  activeRequest: null,
  connected: false,
  theme: loadThemePreference(),
  palette: null
};

// ── DOM References ────────────────────────────────────────────────────────
const form = /** @type {HTMLFormElement} */ (document.getElementById('chat-form'));
const input = /** @type {HTMLTextAreaElement} */ (document.getElementById('chat-input'));
const submitBtn = /** @type {HTMLButtonElement} */ (document.getElementById('chat-submit'));
const chatContainer = /** @type {HTMLElement} */ (document.getElementById('chat'));
const statusEl = /** @type {HTMLElement} */ (document.getElementById('connection-status'));
const themeToggle = /** @type {HTMLButtonElement} */ (document.getElementById('theme-toggle'));

// ── Initialization ────────────────────────────────────────────────────────

/**
 * Bootstrap the application: set up chat renderer, register event
 * listeners, apply theme, and check proxy connectivity.
 */
function init() {
  state.chat = new ChatRenderer(chatContainer);
  state.palette = new CommandPalette(input);

  form.addEventListener('submit', handleSubmit);
  themeToggle.addEventListener('click', cycleTheme);
  input.addEventListener('keydown', handleKeyDown);
  input.addEventListener('input', autoResizeInput);

  applyTheme(state.theme);
  checkConnection();
}

// ── Theme ─────────────────────────────────────────────────────────────────

/**
 * Read the stored theme preference from localStorage.
 *
 * @returns {'light'|'dark'|'auto'}
 */
function loadThemePreference() {
  try {
    const stored = localStorage.getItem('openclaudia-theme');
    if (stored === 'light' || stored === 'dark' || stored === 'auto') {
      return stored;
    }
  } catch { /* localStorage unavailable */ }
  return 'auto';
}

/**
 * Persist the theme preference to localStorage.
 *
 * @param {'light'|'dark'|'auto'} theme
 */
function saveThemePreference(theme) {
  try {
    localStorage.setItem('openclaudia-theme', theme);
  } catch { /* localStorage unavailable */ }
}

/**
 * Apply a theme to the document. Setting `data-theme` on `<html>`
 * overrides the CSS `@media (prefers-color-scheme)` fallback.
 *
 * @param {'light'|'dark'|'auto'} theme
 */
function applyTheme(theme) {
  const root = document.documentElement;
  if (theme === 'auto') {
    root.removeAttribute('data-theme');
  } else {
    root.setAttribute('data-theme', theme);
  }
  updateThemeIcon(theme);
}

/**
 * Update the theme toggle button icon based on current theme.
 *
 * @param {'light'|'dark'|'auto'} theme
 */
function updateThemeIcon(theme) {
  const icons = { light: '☀️', dark: '🌙', auto: '💻' };
  themeToggle.textContent = icons[theme] || icons.auto;
  themeToggle.title = `Theme: ${theme} (click to cycle)`;
}

/**
 * Cycle through theme options: auto → light → dark → auto.
 */
function cycleTheme() {
  const order = /** @type {const} */ (['auto', 'light', 'dark']);
  const idx = order.indexOf(state.theme);
  state.theme = order[(idx + 1) % order.length];
  saveThemePreference(state.theme);
  applyTheme(state.theme);
}

// ── Connection ────────────────────────────────────────────────────────────

/**
 * Check if the OpenClaudia proxy is reachable and update the status indicator.
 */
async function checkConnection() {
  try {
    const health = await healthCheck();
    state.connected = true;
    statusEl.textContent = `v${health.version}`;
    statusEl.className = 'header__status header__status--connected';
  } catch {
    state.connected = false;
    statusEl.textContent = 'disconnected';
    statusEl.className = 'header__status header__status--disconnected';
  }
}

// ── Input Handling ────────────────────────────────────────────────────────

/**
 * Auto-resize the textarea as the user types, up to a max height.
 * Resets to min-height when empty.
 */
function autoResizeInput() {
  input.style.height = 'auto';
  input.style.height = Math.min(input.scrollHeight, 200) + 'px';

  // Show/hide command palette when user types /
  const val = input.value;
  if (val.startsWith('/') && !val.includes(' ')) {
    state.palette?.show();
  } else {
    state.palette?.hide();
  }
}

/**
 * Submit on Enter (without Shift). Shift+Enter inserts a newline.
 * When the command palette is open, Arrow keys navigate and
 * Tab/Enter select the highlighted command.
 *
 * @param {KeyboardEvent} e
 */
function handleKeyDown(e) {
  const palette = state.palette;

  // Palette navigation
  if (palette?.visible) {
    if (e.key === 'ArrowDown') { e.preventDefault(); palette.moveDown(); return; }
    if (e.key === 'ArrowUp') { e.preventDefault(); palette.moveUp(); return; }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      const cmd = palette.getSelected();
      if (cmd) {
        input.value = cmd + ' ';
        palette.hide();
        input.focus();
      }
      return;
    }
    if (e.key === 'Escape') { e.preventDefault(); palette.hide(); return; }
  }

  if (e.key === 'Enter' && !e.shiftKey) {
    e.preventDefault();
    palette?.hide();
    form.dispatchEvent(new Event('submit', { cancelable: true }));
  }
}

// ── Submission ────────────────────────────────────────────────────────────

/**
 * Handle a slash command entered by the user.
 *
 * Commands starting with `/` are intercepted and handled locally.
 * Unrecognized commands show a help hint.
 *
 * @param {string} text - The raw input text (e.g. "/help")
 * @returns {boolean} True if a slash command was handled
 */
function handleSlashCommand(text) {
  const parts = text.trim().split(/\s+/);
  const cmd = parts[0].toLowerCase();
  const arg = parts.slice(1).join(' ');

  switch (cmd) {
    case '/help':
    case '/?':
      state.chat.addMessage({
        role: 'assistant',
        content: `Available commands:\n\n` +
          `/help          Show this message\n` +
          `/clear         Start a new conversation\n` +
          `/theme         Show theme (light/dark/auto)\n` +
          `/theme <name>  Switch theme\n` +
          `/model         Show current model\n` +
          `/copy          Copy last response\n` +
          `/version       Show version info\n` +
          `\nOther commands (TUI-only): /sessions, /compact, /review, /commit, /mode, /plan, /init`
      });
      return true;

    case '/clear':
    case '/new':
      state.chat.clear();
      state.history = [];
      state.chat.addMessage({
        role: 'assistant',
        content: 'Conversation cleared.'
      });
      return true;

    case '/theme':
      if (arg) {
        const valid = ['light', 'dark', 'auto'];
        if (valid.includes(arg)) {
          state.theme = arg;
          saveThemePreference(arg);
          applyTheme(arg);
          state.chat.addMessage({
            role: 'assistant',
            content: `Theme set to ${arg}.`
          });
        } else {
          state.chat.addMessage({
            role: 'error',
            content: `Unknown theme: ${arg}. Use light, dark, or auto.`
          });
        }
      } else {
        state.chat.addMessage({
          role: 'assistant',
          content: `Current theme: ${state.theme}. Use /theme light|dark|auto to change.`
        });
      }
      return true;

    case '/model':
      state.chat.addMessage({
        role: 'assistant',
        content: `Current model: ${getDefaultModel()}`
      });
      return true;

    case '/copy': {
      const messages = chatContainer.querySelectorAll('.message--assistant .message__inner');
      const last = messages[messages.length - 1];
      if (last) {
        navigator.clipboard.writeText(last.textContent || '').then(() => {
          state.chat.addMessage({ role: 'assistant', content: 'Copied to clipboard.' });
        }).catch(() => {
          state.chat.addMessage({ role: 'error', content: 'Failed to copy.' });
        });
      } else {
        state.chat.addMessage({ role: 'assistant', content: 'No response to copy.' });
      }
      return true;
    }

    case '/version':
      state.chat.addMessage({
        role: 'assistant',
        content: `OpenClaudia Web GUI`
      });
      checkConnection();
      return true;

    default:
      state.chat.addMessage({
        role: 'error',
        content: `Unknown command: ${cmd}. Type /help for available commands.\nNote: many slash commands require the TUI (use --tui-mode).`
      });
      return true;
  }
}

/**
 * Resolve the default model from the `<meta name="default-model">` tag.
 *
 * @returns {string}
 */
function getDefaultModel() {
  const meta = document.querySelector('meta[name="default-model"]');
  return meta?.content || '(unknown)';
}

/**
 * Handle form submission: dispatch slash commands locally,
 * otherwise send the user's message to the proxy.
 *
 * @param {Event} e - Form submit event
 */
async function handleSubmit(e) {
  e.preventDefault();

  const prompt = input.value.trim();
  if (!prompt || state.activeRequest) return;

  // Clear input
  input.value = '';
  input.style.height = 'auto';
  submitBtn.disabled = true;

  // Intercept slash commands
  if (prompt.startsWith('/')) {
    state.chat.addMessage({ role: 'user', content: prompt });
    handleSlashCommand(prompt);
    submitBtn.disabled = false;
    input.focus();
    return;
  }

  // Render user message
  state.chat.addMessage({ role: 'user', content: prompt });

  // Show stream indicator
  const indicator = state.chat.showStreamIndicator();

  // Track turn state
  let hasReceivedText = false;

  state.activeRequest = sendChatRequest(
    prompt,
    state.history,
    // onEvent
    (event) => {
      switch (event.type) {
        case 'text':
          if (!hasReceivedText) {
            state.chat.removeStreamIndicator(indicator);
            hasReceivedText = true;
          }
          state.chat.appendStreamingText(event.content);
          break;

        case 'thinking':
          state.chat.showThinking(event.content);
          break;

        case 'tool_start':
          state.chat.showThinking(`Running: ${event.tool_name}`);
          break;

        case 'tool_done':
          if (event.tool_content) {
            state.chat.addMessage({
              role: 'tool',
              content: event.tool_content,
              toolName: event.tool_name
            });
          }
          break;

        case 'error':
          state.chat.addMessage({
            role: 'error',
            content: event.content
          });
          break;
      }
    },
    // onDone — finalize the turn
    () => {
      state.chat.finishStreaming();
      state.chat.removeStreamIndicator(indicator);
      submitBtn.disabled = false;
      input.focus();
      state.activeRequest = null;
      checkConnection();
    },
    // onError
    (error) => {
      state.chat.removeStreamIndicator(indicator);
      state.chat.finishStreaming();
      state.chat.addMessage({
        role: 'error',
        content: `Connection error: ${error}`
      });
      submitBtn.disabled = false;
      input.focus();
      state.activeRequest = null;
      state.connected = false;
      statusEl.textContent = 'error';
      statusEl.className = 'header__status header__status--disconnected';
    }
  );
}

// ── Boot ──────────────────────────────────────────────────────────────────
document.addEventListener('DOMContentLoaded', init);

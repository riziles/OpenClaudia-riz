/**
 * OpenClaudia Web GUI — Slash command palette.
 *
 * Shows a filtered dropdown of available commands when the user
 * types `/` in the input. Supports keyboard navigation and click selection.
 *
 * @module palette
 */

/**
 * @typedef {{ command: string, description: string }} CommandDef
 */

/** @type {CommandDef[]} */
const COMMANDS = [
  { command: '/help', description: 'Show available commands' },
  { command: '/clear', description: 'Start a new conversation' },
  { command: '/theme light', description: 'Switch to light theme' },
  { command: '/theme dark', description: 'Switch to dark theme' },
  { command: '/theme auto', description: 'Use system theme' },
  { command: '/model', description: 'Show current model' },
  { command: '/copy', description: 'Copy last response to clipboard' },
  { command: '/version', description: 'Show version info' },
];

/**
 * Manages the slash command palette popup.
 */
export class CommandPalette {
  /** @type {HTMLElement|null} */
  #el = null;

  /** @type {number} */
  #selectedIndex = 0;

  /** @type {HTMLTextAreaElement} */
  #input;

  /** @type {boolean} */
  #visible = false;

  /**
   * @param {HTMLTextAreaElement} input - The chat input element
   */
  constructor(input) {
    this.#input = input;
  }

  /**
   * Create the palette DOM element (once).
   */
  #ensureElement() {
    if (this.#el) return;
    this.#el = document.createElement('div');
    this.#el.className = 'command-palette';
    this.#el.setAttribute('role', 'listbox');
    this.#el.setAttribute('aria-label', 'Slash commands');
    this.#input.parentElement.appendChild(this.#el);
  }

  /**
   * Show the palette filtered by the current input text.
   */
  show() {
    this.#ensureElement();
    const query = this.#input.value.toLowerCase();
    const filtered = COMMANDS.filter(c =>
      c.command.toLowerCase().includes(query)
    );

    if (filtered.length === 0) {
      this.hide();
      return;
    }

    this.#el.innerHTML = filtered
      .map((c, i) =>
        `<div class="command-palette__item${i === 0 ? ' command-palette__item--selected' : ''}"
              role="option" data-index="${i}">
          <span class="command-palette__cmd">${c.command}</span>
          <span class="command-palette__desc">${c.description}</span>
        </div>`
      )
      .join('');

    this.#selectedIndex = 0;
    this.#el.addEventListener('click', (e) => {
      const item = /** @type {HTMLElement} */ (e.target).closest('.command-palette__item');
      if (!item) return;
      const cmd = item.querySelector('.command-palette__cmd');
      if (cmd) {
        this.#input.value = cmd.textContent + ' ';
        this.hide();
        this.#input.focus();
      }
    });

    this.#el.style.display = 'block';
    this.#visible = true;
  }

  /**
   * Hide the palette.
   */
  hide() {
    if (this.#el) this.#el.style.display = 'none';
    this.#visible = false;
  }

  /** @returns {boolean} */
  get visible() { return this.#visible; }

  /**
   * Move selection up one item. Wraps around.
   */
  moveUp() {
    if (!this.#el || !this.#visible) return;
    const items = this.#el.querySelectorAll('.command-palette__item');
    if (items.length === 0) return;
    this.#selectedIndex = (this.#selectedIndex - 1 + items.length) % items.length;
    this.#renderSelection(items);
  }

  /**
   * Move selection down one item. Wraps around.
   */
  moveDown() {
    if (!this.#el || !this.#visible) return;
    const items = this.#el.querySelectorAll('.command-palette__item');
    if (items.length === 0) return;
    this.#selectedIndex = (this.#selectedIndex + 1) % items.length;
    this.#renderSelection(items);
  }

  /**
   * @param {NodeListOf<HTMLElement>} items
   */
  #renderSelection(items) {
    items.forEach((item, i) => {
      item.classList.toggle('command-palette__item--selected', i === this.#selectedIndex);
    });
  }

  /**
   * Get the currently selected command string.
   * @returns {string|null}
   */
  getSelected() {
    if (!this.#el || !this.#visible) return null;
    const items = this.#el.querySelectorAll('.command-palette__item');
    if (items.length === 0) return null;
    const cmd = items[this.#selectedIndex]?.querySelector('.command-palette__cmd');
    return cmd?.textContent || null;
  }
}

import "@testing-library/jest-dom/vitest";
import i18n from "../i18n";

// jsdom has no layout engine. Components still need the browser observation
// lifecycle, while individual layout tests supply their own measurements.
globalThis.ResizeObserver = class implements ResizeObserver {
  observe() {}
  unobserve() {}
  disconnect() {}
};
globalThis.matchMedia = (query) => ({
  matches: query === "(prefers-reduced-motion: reduce)",
  media: query,
  onchange: null,
  addListener() {},
  removeListener() {},
  addEventListener() {},
  removeEventListener() {},
  dispatchEvent: () => true,
});

// Interaction tests do not render motion. Supply only the 2D values consumed by
// the card readers, retaining their declared translation, scale and rotation.
globalThis.DOMMatrixReadOnly = class {
  a = 1; b = 0; c = 0; d = 1; e = 0; f = 0;
  constructor(value?: string) {
    const matrix = value?.match(/^matrix\(([^)]+)\)$/)?.[1].split(",").map(Number);
    if (matrix?.length === 6) [this.a, this.b, this.c, this.d, this.e, this.f] = matrix;
    else {
      const translation = value?.match(/translate(?:3d)?\(([-\d.]+)px,\s*([-\d.]+)px/);
      const scale = Number(value?.match(/scale\(([-\d.]+)\)/)?.[1] ?? 1);
      const angle = Number(value?.match(/rotate\(([-\d.]+)deg\)/)?.[1] ?? 0) * Math.PI / 180;
      this.a = this.d = Math.cos(angle) * scale;
      this.b = Math.sin(angle) * scale; this.c = -this.b;
      this.e = Number(translation?.[1] ?? 0); this.f = Number(translation?.[2] ?? 0);
    }
  }
  get m41() { return this.e; }
  get m42() { return this.f; }
} as unknown as typeof DOMMatrixReadOnly;

await i18n.changeLanguage("zh-CN");

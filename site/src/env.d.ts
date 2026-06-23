/// <reference path="../.astro/types.d.ts" />

// katex ships its contrib types under @types/katex/contrib/; declare the
// runtime path used in dynamic imports so tsc resolves it correctly.
declare module 'katex/dist/contrib/auto-render' {
  import renderMathInElement from '@types/katex/contrib/auto-render';
  export default renderMathInElement;
}

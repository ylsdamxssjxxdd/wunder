# PPT Maintenance Guide

This folder hosts a fully static HTML-based PPT. Each slide is a standalone JS module, and the slide order is maintained in `manifest.js`. Slides are loaded via dynamic import so you can add/remove/reorder pages easily.

## Structure

- Entry page: `docs/ppt-en/index.html` (container + HUD only)
- Boot script: `docs/ppt-en/slides/boot.js` (loads the manifest, then page modules, then navigation)
- Manifest: `docs/ppt-en/slides/manifest.js` (slide order and paths)
- Slide modules: `docs/ppt-en/slides/*.js` (one file per slide)
- Styles: `docs/ppt-en/styles.css`

## Add a slide

1. Copy any existing slide file as a template and rename it to a meaningful name.
2. Update `data-title` and the slide content.
3. Add the new file path to `docs/ppt-en/slides/manifest.js` in the right order.
4. If it is a section divider or TOC page, update the related pages accordingly.

## Remove a slide

1. Delete the slide module file.
2. Remove its path from `docs/ppt-en/slides/manifest.js`.
3. Update any TOC or section pages that referenced it.

## Edit a slide

- Each slide must return a `<section class="slide ...">` root element.
- Do not add the `active` class manually; the navigation script controls it.
- Use `class="fragment"` for step-by-step reveals.
- Each module should export a default function that returns the DOM node.

## Assets and paths

- Put images/icons under `docs/ppt-en/assets/`.
- Use relative paths inside slides, e.g. `assets/xxx.png`.
- Keep it dependency-free for offline use.

## Notes

- When using template strings, escape `${` as `&#36;{` or `\${`.
- Use `docs/ppt-en/styles.css` for global styles.
- The navigation logic lives in `docs/ppt-en/app.js` and rarely needs changes.

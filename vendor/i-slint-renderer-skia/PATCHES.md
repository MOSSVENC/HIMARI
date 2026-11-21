# Native Vulkan integration

Source: crates.io i-slint-renderer-skia 1.17.0, published by Slint.
The upstream license notices and LICENSES directory are retained.

Application changes are limited to lib.rs, itemrenderer.rs, font_cache.rs,
vulkan_surface.rs, the vulkan_diagnostics.rs module and Cargo.toml:

- Expose read-only physical-device and successful-submission events on the UI
  thread. Observers do not receive device handles or submit rendering commands.
- Report CPU wall times for image acquisition, target preparation, scene drawing,
  flush/submission and presentation. GPU-completion waiting is included in the
  submission stage; these observations are not GPU timestamps.
- Offer compile-time render-tracing of text measurement, text drawing, glyph
  drawing and font lookup. Timings stay in memory until the diagnostic finishes;
  default builds omit the text spans and counters.
- Reuse base typefaces when deriving variable-font weights. Base and variation
  faces share the existing 64-entry cache and retain their font-data blobs.
- Enable Skia's reduced shader variations option to favor first-interaction
  latency for the control center's text, paths and gradients.
- Match cached Vulkan layers to their parent target's attachment format.
- Clear cached layer images on surface/adapter replacement, suspension, DPI
  changes and component destruction. Disabling a cache hint releases the
  layer-cache entry.
- Prefer a presentation-capable integrated GPU; allow discrete GPUs and reject
  software/virtual device types.
- Pass the Vulkan queue family index, rather than its index within that family,
  to Skia's backend context, and declare the enabled swapchain extension.
- Create swapchain images with the color and transfer usages required by
  Ganesh, check surface support, and supply the exact usage bits in ImageInfo.
- Support explicit one-shot GPU readback of the acquired target before
  presentation. Capture runs outside the application's cadence measurements;
  normal frames allocate no readback buffer.
- Wait for the acquired image before Skia renders. Flush with Present access and
  wait for Skia GPU completion before handing the image to the presentation
  engine. FIFO presentation supplies display pacing. CPU-side completion
  waiting is a correctness measure in this integration; it is not an
  asynchronous multi-frame rendering pipeline.
- Retain presentation futures until completion and handle zero-sized windows
  and acquisition timeouts during window state changes.

The other files retain upstream source. The application pins the complete Slint
family to 1.17.0. Review these patched modules and the feature declaration
explicitly when updating the renderer.

- Try `COMPOSITE_ALPHA_PRE_MULTIPLIED` before the modes the surface advertises,
  and fall back to them in order. The advertised bitfield iterates to `OPAQUE`
  first, which silently makes every window opaque; on the reference driver the
  premultiplied mode is rejected outright (`Swapchain::new` validation error),
  so the fallback keeps startup working while drivers that do support it get
  real window transparency. Measured on 2026-11-20: candidate probe wrote
  `rejected PreMultiplied / accepted Opaque` for both windows.

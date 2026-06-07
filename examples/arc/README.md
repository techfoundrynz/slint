# Slint Arc Primitive Example

This example demonstrates the usage and rendering capabilities of the native `Arc` primitive in Slint. It showcases:
- An indeterminate circular loading spinner with a continuous rotation animation.
- An interactive progress ring with smooth animation controlled by buttons.
- A dashboard-style speedometer gauge that can be adjusted interactively by clicking/dragging.

All widgets are rendered using the native `Arc` element, which supports stroke widths, start/end angles, and customizable endcaps (`LineCap.round` vs `LineCap.butt`). It is designed to run efficiently on both desktop platforms and embedded microcontrollers (such as ESP32) using Slint's optimized zero-allocation software renderer.

## Running the Example

You can preview the example directly using the `slint-viewer` tool:

```sh
cargo run --bin slint-viewer -- examples/arc/arc.slint
```

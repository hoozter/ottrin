# Ottrin numeric accessibility patch

Source: egui 0.33.3 from crates.io (MIT OR Apache-2.0), registry package SHA256 `6a9b567d356674e9a5121ed3fedfb0a7c31e059fe71f6972b691bcd0bfc284e3`, verified against the original Cargo.lock and actual cached crate archive. The normalized manifest, README and full source are vendored; dependencies remain registry packages. License texts are shared with upstream egui-winit.

DragValue publishes its final committed value to widget/accessibility metadata after keyboard or drag editing. Previously it published the pre-edit local value: typing2 then4 could display24 and apply24 while native RangeValue still reported2. Read the final getter once and use it for changed/value/step-action metadata. No duplicate value state or caller override is added.

The rendered Ottrin regression types2 and4 in separate frames in Light/Dark at150%, checking actual model/theme and emitted numeric value in each input frame. Preserve this invariant when updating egui.

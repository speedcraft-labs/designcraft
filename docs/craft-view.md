# Shared canvas transforms (#72)

`View` still stores the document point at the unrotated viewport minimum and
clockwise quarter turns about the viewport centre. `craft-view` supplies the
translation/scale algebra and pointer-anchored zoom, with bounds 0.05–40,
`ppp = 1.0` and `aspect = 1.0`. `set_zoom` continues to zoom at the viewport
centre through `zoom_at`, including its existing ratio calculation.

The full shared transform uses `centre = origin + rect.size() / (2 * zoom)`
and `rotation = rotation_quarters * FRAC_PI_2`: both conventions rotate
clockwise in screen coordinates about `rect.center()`. `zoom_at` builds this
transform, calls `zoom_about`, and converts the centre back to an origin.
It then corrects the anchor residual measured through `Xf` to preserve the
old f32 rounding boundaries, rather than changing the document point under
the pointer by a few millionths of a point.

For `Xf`, replacing the old origin-based f32 subtraction with a centre-based
f32 subtraction is not numerically identical, even without rotation. The
adapter therefore rebases its shared translation/scale transform to a
zero-size rectangle at the original `rect.min`, with `centre = origin`.
The existing exact quarter-turn helper runs outside that unrotated shared
transform. This preserves the original rounding stages while delegating
forward and inverse conversion to `craft-view`.

`unrotate`, `unrotate_delta` and `view_rect` remain local layout helpers;
quarter-turn raster rounding also remains local. No view state or public
`Xf` fields change. A deterministic 2,000-case battery compares the old
implementation with the adapter, including fractional rectangles, all four
rotations and absolute zoom. Dedicated tests cover both zoom clamps,
missing views, the rounding incompatibility and the shared rotation direction.

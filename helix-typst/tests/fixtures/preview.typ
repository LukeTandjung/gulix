// Manual rendering fixture for the Helix Typst preview.
// Open this file with the helix-typst hx binary inside Tern.

= Typst preview fixture

This paragraph contains inline math: $x_i^2 + y_i^2 = r^2$.

The following equation is a display-math preview:

$ sum_(i=1)^n i = (n(n + 1)) / 2 $

```rust
fn main() {
    println!("Typst code-block preview");
}
```

#table(
  columns: 2,
  [Construct], [Status],
  [Inline math], [Rendered],
  [Display math], [Rendered],
  [Code blocks], [Rendered],
  [Tables], [Rendered],
)

#image("pixel.svg", width: 48pt)

#figure(
  image("pixel.svg", width: 64pt),
  caption: [A figure containing the test image.],
)

= Scrolling content

Scrolling line 01: plain source text with an inline preview $x_1 + y_1$.

Scrolling line 02: plain source text.

Scrolling line 03: plain source text.

Scrolling line 04: plain source text.

Scrolling line 05: plain source text with an inline preview $alpha_5 + beta_5$.

Scrolling line 06: plain source text.

Scrolling line 07: plain source text.

Scrolling line 08: plain source text.

Scrolling line 09: plain source text.

Scrolling line 10: plain source text with an inline preview $x_10^2$.

Scrolling line 11: plain source text.

Scrolling line 12: plain source text.

Scrolling line 13: plain source text.

Scrolling line 14: plain source text.

Scrolling line 15: plain source text with an inline preview $gamma_15$.

Scrolling line 16: plain source text.

Scrolling line 17: plain source text.

Scrolling line 18: plain source text.

Scrolling line 19: plain source text.

Scrolling line 20: plain source text with an inline preview $sum_20 x_i$.

Scrolling line 21: plain source text.

Scrolling line 22: plain source text.

Scrolling line 23: plain source text.

Scrolling line 24: plain source text.

Scrolling line 25: plain source text with an inline preview $delta_25$.

Scrolling line 26: plain source text.

Scrolling line 27: plain source text.

Scrolling line 28: plain source text.

Scrolling line 29: plain source text.

Scrolling line 30: plain source text with an inline preview $x_30 / y_30$.

Rocket example support

rocket.rs and rocket.bin were copied verbatim on 2026-10-22 from the user-supplied
assets/rocket_sprite/output/ directory in this repository. The decoder emits an
11x21, five-color RGB888 image from a 40-byte compressed sprite. The source README
describes a manually reconstructed pixel grid from rocket_pixel_art.jpg; the
generator is assets/rocket_sprite/recreate.py. No separate asset license was
provided. See the package PROVENANCE.md; no artwork ownership is asserted here.

SHA-256:
rocket.rs  f7cfd6bc1764aee5e93d78d1eb0ff697659ca1589c219f55527584650799fb5d
rocket.bin be20a25d730ec9c1dad7d5846d6105e83e1f6485fefb24eedfbbed415b047a61

scene.rs is the allocation-free simulation and renderer shared by firmware and
preview.rs. It decodes once, treating exact RGB888 white as transparent, converts
the remaining colors to RGB332, and caches eight nearest-neighbor rotations as
scaled horizontal color runs, together with scaled exhaust coordinates.
The copied decoder and binary are unchanged. New example code follows the
repository's MIT policy.

The top-level example owns all tuning settings. See the package README for the
native preview, host simulation tests, Pico build and USB flashing commands.

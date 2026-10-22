# NOTICE

## Satya is not a replacement for any commercial tool

Satya is an independent, open-source implementation of a subset of the
*Portable Test and Stimulus Standard*. It is **not** a substitute for a
commercial PSS product, and it is not intended to be used as one.

Commercial PSS tools — for example Cadence Perspec and Siemens InFact — are
mature, vendor-supported, production-validated environments. They offer things
Satya does not:

* complete language coverage, rather than the documented subset in
  [`docs/SEMANTICS.md`](docs/SEMANTICS.md);
* coverage closure and the surrounding methodology, not just stimulus
  generation;
* integration with established verification flows and existing testbenches;
* vendor support, qualification, regression stability and long-term maintenance;
* the performance and capacity of a product built for production-scale models.

If you need production stimulus generation, **use a commercial tool**. Satya
exists to explore the problem space and to serve as a small, readable reference
implementation.

## No affiliation

Satya is not affiliated with, endorsed by, sponsored by, or derived from
Cadence Design Systems, Siemens EDA, or any other EDA vendor. It was written
independently from the public standard.

Product and company names are the trademarks or registered trademarks of their
respective owners. They are used in this repository only to describe the
category of tooling or to refer to a public document, and their use does not
imply any affiliation or endorsement.

## Relationship to the standard

*Portable Test and Stimulus Standard 3.0* is published by Accellera Systems
Initiative. The standard's own examples were used to test this implementation
(see [`docs/LRM-EXAMPLES.md`](docs/LRM-EXAMPLES.md)); those examples remain the
property of their authors.

## Third-party content

The models under `examples/opentitan/` are written against the register
interfaces published in the [OpenTitan](https://github.com/lowRISC/opentitan)
project (`hw/ip/uart/data/uart.hjson`, `hw/ip/aes/data/aes.hjson`), which is
licensed under Apache-2.0. The models themselves are original to this
repository.

Satya has no runtime dependencies. Its build depends only on the Rust standard
library.

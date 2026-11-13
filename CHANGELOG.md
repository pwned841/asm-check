# Changelog

## 0.1.0

First release.

- Parser for NASM syntax: labels, local labels, memory operands, constant
  expressions, prefixes, `times`, directives.
- Register, flag and control-flow effects for 110 mnemonics and every
  conditional form.
- Function and basic-block recovery, per-function summaries, a forward
  dataflow solver.
- 26 lints in four groups: correctness, suspicious, style, pedantic.
- Command-line interface with human, short and JSON output.
- Language server with diagnostics and quick fixes.
- Zed extension.

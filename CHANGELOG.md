# Changelog

## 0.2.0

- 32 new lints, 58 in total. Highlights:
  - `uninitialized-register`, `partial-register-read` and a path-sensitive
    `clobbered-register`, from one analysis of which registers hold a
    value. It knows system call arguments and what each local function
    reads from its caller.
  - `undefined-flags`: a conditional jump after a `mov`.
  - System calls: `unknown-syscall`, `exit-code-truncated`,
    `pointer-from-contents`.
  - Memory and data: `invalid-address`, `truncated-address`,
    `data-size-mismatch`, `write-to-readonly`, `immediate-out-of-range`,
    `shift-out-of-range`.
  - Sections: `code-in-data-section`, `data-in-bss`, `unknown-section`,
    `misplaced-length`, `jump-to-data`, `unused-extern`.
  - Calls and stack: `mismatched-pop`, `odd-stack-adjustment`,
    `variadic-call-without-al`, `main-return-value`.
  - `unknown-instruction` for misspelt mnemonics; register suggestions in
    `undefined-symbol`.
  - Style: `constant-comparison`, `jump-over-jump`, `jump-to-jump`,
    `lea-as-mov`, `redundant-test`, `redundant-push-pop`,
    `redundant-move`, `slow-loop`, `absolute-address`.
- Language server: hover, go to definition, find references, rename,
  document outline, inlay hints naming each system call, completion.
- `examples/showcase.s` triggers every lint.
- Fixed: escapes in backquoted strings; a lone half-typed word no longer
  breaks local-label resolution; `needless-rex` no longer suggests what
  NASM already does.

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

# Changelog

## 0.5.0

- Checked against NASM 2.16.01 on 17,004 generated instructions
  (`scripts/nasm-diff.py`): 0 false positives and 0 missed errors. The
  first run found 3 false positives and 4,688 missed errors, all fixed.
- New table of the valid operand forms of `bsf`, `popcnt`, `bt`, `xadd`,
  `cmpxchg`, `imul`, `cmov`, `setcc`, `bswap`, `movzx`, `movsx`, `movsxd`,
  and of the sizes NASM wants on `push`, `pop`, `call` and `jmp`.
- `ambiguous-operand-size` also covers `push [mem]`, `pop [mem]`,
  `movzx eax, [mem]` and `bt [mem], imm`, which NASM rejects.
- `invalid-address` covers `[rsp + rsp]`.
- No longer reported, because NASM accepts them: `lea rax, label`,
  `movsx rax, eax`, and an n-bit immediate down to -2^n.
- The 21 course exercises behave identically before and after `--fix`,
  checked by running them in an x86-64 container.

## 0.4.1

- MASM and ARM assembly are recognised and left alone. A Python
  virtualenv held one of each with an `.asm` extension, and both got over a
  hundred false errors. Checks, hover, outline and completion stay silent on
  them, as they already did on GNU assembler syntax.

## 0.4.0

- Editor popups are plain text: the headline, then the fix on one line
  (`fix: mov [x], 0  →  mov byte [x], 0`, `fix: remove mov rax, rax`).
  Zed does not render Markdown in diagnostics. Fix previews no longer copy
  the comment at the end of the line.
- A new `performance` group, on by default, whose lints follow what
  registers hold, what comparisons compare and which flags are read:
  `redundant-load` (inside loops), `div-by-power-of-two`,
  `mul-by-power-of-two`, `fold-constants`, `use-lea`,
  `redundant-memory-load`, `mergeable-stores`, `jump-to-ret`.
- `constant-condition`: a conditional jump whose outcome is decided.
- `needless-rex` also covers `movzx r64, ...`.
- `70` lints in total.
- Fixed a duplicate: `mov rax, rax` is no longer also a `dead-store`.

## 0.3.0

- Diagnostics are one short sentence, with a label under the code and on
  the related lines, in the manner of rustc.
- Fixes are shown as a diff, in the terminal and in the editor, and can
  change several lines. Most lints now have one; several infer the intent
  from the rest of the program (declared size, register just loaded,
  registers pushed, `r10` for `rcx`).
- `--fix` applies the safe fixes in place; `--unsafe-fixes` adds the others.
- Inlay hints: argument values after `syscall`
  (`write(fd=1, buf=message, count=6)`), the condition after a conditional
  jump (`if rax < 5`), the character after a byte-sized number.
- New lints, 61 in total: `buffer-overflow`, `missing-nul-terminator`,
  `infinite-loop`.
- Data definitions continued on unlabelled lines are measured as a whole.

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

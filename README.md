# asm-check

A semantic linter for x86-64 assembly written in NASM (Intel) syntax.

An assembler tells you when a line cannot be encoded. It does not tell you
that `div` is about to divide by a stale `rdx`, that `syscall` just destroyed
your loop counter, or that the `je` you wrote tests the wrong instruction.
`asm-check` does: it is to NASM what `cargo clippy` is to rustc.

```console
$ asm-check examples/demo.s
warning[clobbered-register]: `rcx` was destroyed by `syscall`
  --> examples/demo.s:14:9
   |
13 |     syscall
   |     ------- `syscall` overwrites `rcx` and `r11`
14 |     dec rcx
   |         ^^^ read here
   |
   = help: keep the value in a register `syscall` preserves, such as `r12`

warning[div-without-rdx]: `rdx` is not set up for `div`
  --> examples/demo.s:21:5
   |
13 |     syscall
   |     ------- `rdx` was the 3rd argument of this call
...
21 |     div rbx
   |     ^^^^^^^ divides `rdx:rax`
   |
help: clear `rdx` first
   |
   +     xor edx, edx
21 |     div rbx

error[ambiguous-operand-size]: the size of `[result]` is not specified
  --> examples/demo.s:28:9
   |
28 |     mov [result], 0
   |         ^^^^^^^^ 1, 2, 4 or 8 bytes?
   |
help: it is declared as `qword`
   |
28 -     mov [result], 0
   +     mov qword [result], 0

...
asm-check: 1 error, 4 warnings, 1 hint in 1 file
```

It runs from the command line and as a language server, so the same
diagnostics show up in the editor as you type.

## Install

```console
$ cargo install --path .
```

This puts `asm-check` in `~/.cargo/bin`. Rust 1.88 or later is required.

## Usage

```console
$ asm-check file.s                  # one file
$ asm-check src/                    # every .s, .S, .asm and .nasm below src/
$ asm-check --pedantic file.s       # stricter suggestions too
$ asm-check --level warning file.s  # hide the style hints
$ asm-check -A unused-label file.s  # silence one lint
$ asm-check -D file.s               # warnings make the exit code non-zero
$ asm-check --format json file.s    # machine-readable
$ asm-check --fix file.s            # apply the safe fixes in place
$ asm-check --fix --unsafe-fixes file.s   # also the ones that guess the intent
$ asm-check lints                   # list every lint
```

The exit code is `0` when nothing above a hint was found, `1` on errors
(or warnings with `-D`), `2` when a file could not be read.

`examples/showcase.s` triggers every lint, one commented paragraph each:
open it in your editor to see them all.

## Lints

70 lints in five groups. [docs/lints.md](docs/lints.md) describes each one
with an example.

| Group | Severity | Lints |
|-------|----------|-------|
| correctness | error | `operand-size-mismatch` `ambiguous-operand-size` `invalid-operands` `invalid-address` `immediate-out-of-range` `jump-to-data` `unknown-instruction` `undefined-symbol` `duplicate-label` `unbalanced-stack` `ret-in-start` |
| suspicious | warning | `misaligned-call` `mismatched-pop` `odd-stack-adjustment` `variadic-call-without-al` `main-return-value` `fallthrough` `unreachable-code` `int80-in-64bit` `entry-not-global` `div-without-rdx` `idiv-zero-extended` `uninitialized-register` `clobbered-register` `partial-register-read` `unknown-syscall` `exit-code-truncated` `pointer-from-contents` `buffer-overflow` `missing-nul-terminator` `infinite-loop` `truncated-address` `data-size-mismatch` `write-to-readonly` `shift-out-of-range` `code-in-data-section` `data-in-bss` `unknown-section` `misplaced-length` `callee-saved-clobbered` `dead-store` `undefined-flags` `constant-comparison` `flags-clobbered` `unused-comparison` `constant-condition` |
| style | hint | `unused-label` `unused-extern` `redundant-jump` `jump-over-jump` `jump-to-jump` `mov-zero` `cmp-zero` `self-move` `noop-arithmetic` `lea-as-mov` `redundant-test` `redundant-push-pop` `redundant-move` |
| performance | hint | `redundant-load` `div-by-power-of-two` `mul-by-power-of-two` `fold-constants` `use-lea` `redundant-memory-load` `mergeable-stores` `jump-to-ret` |
| pedantic | hint, opt-in | `needless-rex` `slow-loop` `absolute-address` |

### Silencing a lint

```nasm
    mov rax, 0              ; asm-check: allow(mov-zero)

    ; asm-check: allow(dead-store)
    mov rbx, 1

; asm-check: allow-file(unused-label)
```

A comment on its own line applies to the next statement. `allow(all)`
silences everything on that line.

### Understanding what the code does

The `performance` lints reason about the program rather than match
patterns. The analysis follows what every register holds (a constant, the
address of a label), what each comparison compares and which flags a later
instruction still reads, through branches, loops, `syscall` (which keeps
everything but `rax`, `rcx` and `r11`) and calls to the file's own
functions. From that it finds:

- a load repeated inside a loop that changes nothing (`redundant-load`);
- a jump whose outcome is already decided (`constant-condition`);
- `div` by a power of two, with `rdx` known to be zero, that a shift and a
  mask replace (`div-by-power-of-two`);
- two adjustments of a register that fold into one, `mov` then `add` that
  one `lea` does, a second load of memory nothing could have changed;
- four `mov byte` stores of `H`, `4`, `C`, `K` that one
  `mov dword [buffer], 'H4CK'` does.

None is suggested when a later instruction reads a flag the replacement
would set differently.

### Fixes

Most lints come with a fix, printed as a diff and offered in the editor as
a code action. Several are inferred from the rest of the program:

- `mov [counter], 0` gets the size `counter` was declared with;
- `jz` after a `mov` gets a `test` of the register that was just loaded;
- a `ret` with registers still pushed gets the matching `pop`s, in order;
- the 4th system call argument put in `rcx` is moved to `r10`;
- `and rax, 0xFFFFFFFF` becomes `mov eax, eax`;
- a `cmp` separated from its jump is moved next to it, when nothing in
  between changes what it compares.

A fix is *safe* when it cannot change what the program does. `--fix`
applies only those; fixes that guess the intent need `--unsafe-fixes`.

## Editor integration

`asm-check lsp` is a Language Server Protocol server over standard input
and output.

| Feature | What it does |
|---------|--------------|
| Diagnostics | Every lint, updated on each change: a one-line message, the related locations, and the fix as a diff. |
| Quick fixes | The fix of each lint, and a comment that silences it on that line. |
| Hover | Instructions: what they do, the registers and flags they touch, and for `syscall` the call and its arguments. Registers: their role and sub-registers. Labels: definition, section, size, number of uses. Numbers: decimal, hex, binary, character. |
| Inlay hints | After `syscall`, the call with its argument values: `write(fd=1, buf=message, count=6)`. After a conditional jump, the condition it tests: `if rax < 5`. After a byte-sized number, its character: `'0'`. |
| Go to definition, find references, rename | For labels, with `.local` labels resolved in their scope. |
| Outline | Functions, labels, data and constants. |
| Completion | Instructions with a description, registers, and the labels in scope. |

Settings are read from the initialization options:

```json
{ "pedantic": false, "allow": ["unused-label"] }
```

### Zed

The extension lives in [editors/zed](editors/zed). It adds the server to
the Assembly language, which the
[`assembly`](https://github.com/DevBlocky/zed-asm) extension provides.

1. Install the `assembly` extension from the Zed extension gallery.
2. Run `cargo install --path .` so that `asm-check` is on your `PATH`.
3. In Zed, run `zed: install dev extension` from the command palette and
   pick the `editors/zed` directory.
4. Enable the server in `settings.json`:

```json
{
  "languages": {
    "Assembly": { "language_servers": ["asm-check", "..."] }
  },
  "lsp": {
    "asm-check": {
      "initialization_options": { "pedantic": false, "allow": [] }
    }
  }
}
```

### Neovim

```lua
vim.api.nvim_create_autocmd("FileType", {
  pattern = { "asm", "nasm" },
  callback = function()
    vim.lsp.start({ name = "asm-check", cmd = { "asm-check", "lsp" } })
  end,
})
```

### Helix

```toml
[language-server.asm-check]
command = "asm-check"
args = ["lsp"]

[[language]]
name = "nasm"
language-servers = ["asm-check"]
```

### Anything else

`--format short` prints one `path:line:col: severity[lint]: message` line
per diagnostic, which most editors can parse. For a VS Code task:

```json
{
  "label": "asm-check",
  "type": "shell",
  "command": "asm-check --format short --level warning ${file}",
  "problemMatcher": {
    "owner": "asm-check",
    "fileLocation": ["autoDetect", "${workspaceFolder}"],
    "pattern": {
      "regexp": "^(.*):(\\d+):(\\d+): (error|warning)\\[([\\w-]+)\\]: (.*)$",
      "file": 1, "line": 2, "column": 3, "severity": 4, "code": 5, "message": 6
    }
  }
}
```

## How it works

```
source ─▶ parser ─▶ lines ─▶ program ─▶ lints ─▶ diagnostics
                              │
                              ├─ symbols, sections
                              ├─ instruction effects (registers, flags, control flow)
                              ├─ basic blocks and their successors
                              └─ functions and their summaries
```

1. **Parsing** ([src/parser.rs](src/parser.rs)) is line-oriented and never
   fails: what it does not recognise becomes an instruction with an unknown
   mnemonic.
2. **Instruction effects** ([src/insn.rs](src/insn.rs)) describe, for 110
   mnemonics and every conditional form (`jcc`, `setcc`, `cmovcc`), which registers and flags are read and written and where
   control goes. Unknown instructions (SSE, AVX, macros) are marked as such
   and every analysis treats them as "could do anything".
3. **Program structure** ([src/program.rs](src/program.rs)). Assembly has
   no syntax for functions, so they are inferred: a label is a function
   entry when it is `global`, is the target of a `call`, or is `_start` or
   `main`. Code is split into basic blocks, and each function gets a
   summary of the registers it clobbers and saves, propagated through the
   call graph.
4. **Lints** ([src/lints](src/lints)) come in three kinds. Some look at one
   instruction. Some walk a basic block. The ones that must know what holds
   on every path (the stack depth at `ret`, the state of `rdx` at `div`,
   which registers hold a value, whether the flags were set) use a forward
   dataflow solver
   ([src/dataflow.rs](src/dataflow.rs)).

The guiding rule is that a wrong diagnostic costs more than a missed one:
when the tool cannot tell, it stays quiet.

## Scope and limits

- **NASM syntax only.** GNU assembler (AT&T operands, `.globl`
  directives), MASM (`PROC`, `.code`) and ARM files are detected and
  skipped.
- **x86-64 Linux.** The calling-convention lints assume the System V ABI
  and the Linux `syscall` convention. Files that select `bits 16` or
  `bits 32` are skipped.
- **One file at a time.** A symbol defined in another file must be declared
  `extern`, as the assembler requires anyway.
- **Macros are opaque.** In a file that uses `%macro`, `%define` or
  `%include`, the lints that need the complete symbol table
  (`undefined-symbol`, `unused-label`, `entry-not-global`) are turned off.
- **No encoding.** The tool does not assemble: an instruction it does not
  model is not checked, and an invalid one outside the cases listed under
  `invalid-operands` is left for the assembler to report.

## Validation

The error lints claim that the assembler rejects something, so they are
checked against the assembler itself, NASM 2.16.01 (the version Ubuntu 24.04
ships):

- `python3 scripts/nasm-diff.py` combines every mnemonic with registers of
  each size, memory operands with and without a size, immediates and labels,
  has NASM assemble the 17,004 results and compares them with asm-check line
  by line: **0 false positives and 0 missed errors**. It needs Docker and the
  image in `scripts/Dockerfile`, or a local NASM 2.16.
- The 21 exercises of the course, assembled and run in an x86-64 container
  with 1,263 combinations of arguments and input, behave identically before
  and after `--fix` and after `--fix --unsafe-fixes`.

## Development

```console
$ cargo test                 # unit tests, fixtures, an LSP session
$ cargo clippy --all-targets
$ cargo fmt
```

Each lint has a fixture, `tests/fixtures/<lint>.s`. Lines ending in `;~`
must be reported by that lint, and no other line may be. `tests/clean/`
holds correct programs that must produce no diagnostic at all. To add a
lint: declare it next to its pass in `src/lints/`, register it in
`src/lints/mod.rs`, add its fixture and its section in `docs/lints.md`.
The test suite fails if any of these is missing.

## License

[MIT](LICENSE)

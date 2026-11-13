# asm-check

A semantic linter for x86-64 assembly written in NASM (Intel) syntax.

An assembler tells you when a line cannot be encoded. It does not tell you
that `div` is about to divide by a stale `rdx`, that `syscall` just destroyed
your loop counter, or that the `je` you wrote tests the wrong instruction.
`asm-check` does: it is to NASM what `cargo clippy` is to rustc.

```console
$ asm-check examples/demo.s
warning[clobbered-register]: `rcx` is read here, but the `syscall` on line 13 destroyed its value
  --> examples/demo.s:14:9
   |
14 |     dec rcx
   |         ^^^
   |
   = note: the kernel uses `rcx` and `r11` to return from `syscall`: both are destroyed
   = help: keep the value in another register, or `push`/`pop` it around the `syscall`

warning[div-without-rdx]: `div` divides `rdx:rax`, but `rdx` still holds the third argument of the system call on line 13
  --> examples/demo.s:21:5
   |
21 |     div rbx
   |     ^^^^^^^
   |
   = note: a leftover `rdx` gives a wrong quotient, or a divide error (SIGFPE) when the quotient does not fit
   = help: clear it first with `xor edx, edx`

warning[flags-clobbered]: `je` tests the flags of `add` (line 25), not those of the `cmp` on line 24
  --> examples/demo.s:26:5
   |
26 |     je .ten
   |     ^^^^^^^
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
$ asm-check lints                   # list every lint
```

The exit code is `0` when nothing above a hint was found, `1` on errors
(or warnings with `-D`), `2` when a file could not be read.

## Lints

26 lints in four groups. [docs/lints.md](docs/lints.md) describes each one
with an example.

| Group | Severity | Lints |
|-------|----------|-------|
| correctness | error | `operand-size-mismatch` `ambiguous-operand-size` `invalid-operands` `undefined-symbol` `duplicate-label` `unbalanced-stack` `ret-in-start` |
| suspicious | warning | `div-without-rdx` `idiv-zero-extended` `clobbered-register` `callee-saved-clobbered` `misaligned-call` `dead-store` `flags-clobbered` `unused-comparison` `fallthrough` `unreachable-code` `int80-in-64bit` `entry-not-global` |
| style | hint | `unused-label` `redundant-jump` `mov-zero` `cmp-zero` `self-move` `noop-arithmetic` |
| pedantic | hint, opt-in | `needless-rex` |

### Silencing a lint

```nasm
    mov rax, 0              ; asm-check: allow(mov-zero)

    ; asm-check: allow(dead-store)
    mov rbx, 1

; asm-check: allow-file(unused-label)
```

A comment on its own line applies to the next statement. `allow(all)`
silences everything on that line.

## Editor integration

`asm-check lsp` is a Language Server Protocol server over standard input
and output. It publishes diagnostics on every change and offers two kinds
of quick fix: the rewrite a lint suggests (`mov rax, 0` to `xor eax, eax`),
and a comment that silences the lint on that line.

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
2. **Instruction effects** ([src/insn.rs](src/insn.rs)) describe, for about
   120 mnemonics, which registers and flags are read and written and where
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
   which registers a call destroyed) use a forward dataflow solver
   ([src/dataflow.rs](src/dataflow.rs)).

The guiding rule is that a wrong diagnostic costs more than a missed one:
when the tool cannot tell, it stays quiet.

## Scope and limits

- **NASM syntax only.** Files in GNU assembler syntax (AT&T operands,
  `.globl` directives) are detected and skipped.
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

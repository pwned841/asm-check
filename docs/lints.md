# Lints

Every lint belongs to a group, which sets its severity:

| Group | Severity | Meaning |
|-------|----------|---------|
| `correctness` | error | The assembler rejects it, or it is wrong on every run. |
| `suspicious` | warning | Almost certainly a bug. |
| `style` | hint | Works, but has a shorter or clearer form. |
| `pedantic` | hint | Stricter suggestions, only with `--pedantic`. |

Silence a lint on one line with `; asm-check: allow(name)`, for a whole
file with `; asm-check: allow-file(name)`, or globally with `-A name`.

## Correctness

### operand-size-mismatch

The two operands do not have the same size, the source of `movzx`/`movsx`
is not smaller than its destination, or a shift count is a register other
than `cl`.

```nasm
mov rax, ebx        ; 64-bit destination, 32-bit source
shl rax, rcx        ; the count must be cl
```

Use the matching sub-register, or widen explicitly with `movzx`/`movsx`.

### ambiguous-operand-size

Nothing tells the assembler how many bytes a memory operand covers.

```nasm
mov [rdi], 0        ; 1, 2, 4 or 8 bytes?
inc [counter]
```

State it: `mov byte [rdi], 0`, `inc qword [counter]`.

### invalid-operands

An operand combination the instruction does not have:

```nasm
mov [dst], [src]        ; two memory operands
mov counter, rax        ; a label is an address, not a location: [counter]
cmp 5, rax              ; immediate first
lea rax, rbx            ; lea needs a memory operand
push eax                ; no 32-bit push in 64-bit mode
```

### invalid-address

A memory operand that no addressing mode can express. An address is
`base + index*scale + displacement`, with 64-bit (or 32-bit) registers
and a scale of 1, 2, 4 or 8.

```nasm
mov rax, [bx]               ; 16-bit register
mov rax, [rax + rbx*3]      ; scale
mov rax, [rax + rbx + rcx]  ; three registers
mov rax, [rax + rsp*2]      ; rsp cannot be an index
```

### immediate-out-of-range

An immediate that does not fit its operand. The assembler warns and keeps
the low bits.

```nasm
mov al, 300             ; al holds 0-255: this stores 44
and rax, 0xFFFFFFFF     ; sign-extended to 0xFFFFFFFFFFFFFFFF: masks nothing
```

The second case is the dangerous one: every instruction except
`mov r64, imm64` takes at most a 32-bit immediate and sign-extends it.

### jump-to-data

A `jmp`, a conditional jump or a `call` targets a data label. The
processor would execute the bytes of the data.

### unknown-instruction

A word in instruction position that is not an instruction, but is one typo
away from one: `mvo`, `sycall`, `pus`. A fix replaces it. Words that match
nothing are left alone, since the tool does not know every instruction.

### undefined-symbol

A name is used that is neither a label of the file nor declared `extern`,
or a `global` declaration names a label that does not exist. When a label
with a similar name exists, it is suggested.

Skipped when the file uses macros or `%include`, which can define symbols
the tool cannot see.

### duplicate-label

The same label is defined twice. Local labels (`.loop`) only clash within
the same parent label.

### unbalanced-stack

A `ret` is reached with a stack that is not back where it was on entry.
`ret` jumps to whatever is on top of the stack.

```nasm
leaky:
    push rbx
    mov rbx, rdi
    ret             ; jumps to the saved rbx
```

The check follows every path, including `sub rsp`/`add rsp`, `leave` and
`mov rsp, rbp`. It gives up, silently, when `rsp` is changed by an amount
it cannot determine.

### ret-in-start

`_start` is entered by the kernel, not called: the top of the stack is
`argc`, not a return address. Leave with the exit system call.

## Suspicious

### div-without-rdx

`div` and `idiv` divide the 128-bit value `rdx:rax`. Reaching one with an
`rdx` that was not prepared for it gives a wrong result or a divide error.

```nasm
mov rax, [total]
mov rbx, 10
div rbx             ; rdx is whatever it was
```

`rdx` counts as unprepared when it was never written in the function, was
left by a call, still holds the remainder of a previous division (a common
bug in loops), or was loaded as a system call argument. Clear it with
`xor edx, edx` before `div`, sign-extend with `cqo` before `idiv`.

### idiv-zero-extended

`idiv` is signed, so `rdx` must hold the sign of `rax`. Zeroing it makes
every negative dividend a huge positive one. Use `cqo` (`cdq` in 32-bit).

### clobbered-register

A register is read after a `syscall` or a call destroyed its value.

```nasm
    mov rcx, 10
.loop:
    mov eax, 1
    syscall
    dec rcx         ; rcx was overwritten by syscall
    jnz .loop
```

`syscall` always destroys `rcx` and `r11`. An external function may
overwrite every caller-saved register. For a function defined in the same
file, the tool looks at what it actually does, and only reports registers
that end up destroyed as a side effect, not the ones it sets on purpose,
which may be results under a custom calling convention.

### uninitialized-register

A register is read in `_start` before anything wrote it. No register
except `rsp` has a specified value when a program starts. On Linux they
are usually zero, which is exactly why the bug goes unnoticed.

```nasm
_start:
    add rbx, 1          ; rbx was never set
    mov eax, 1
    mov edi, 1
    syscall             ; write(fd, buf, count): rsi and rdx were never set
```

The check knows the arguments of the common system calls, and what each
function of the file reads from its caller, so a missing argument is
reported at the `syscall` or the `call` that needs it. Functions other
than the entry point are assumed to receive their registers as parameters.

### partial-register-read

A whole register is read after only its low bits were written.

```nasm
mov al, [rsi]
cmp rax, 10         ; bits 8-63 of rax are whatever was there
```

Load with `movzx eax, byte [rsi]`, or clear the register first. Writing a
32-bit register is fine: it zeroes the upper half.

### undefined-flags

A conditional jump, `setcc` or `cmovcc` is reached when nothing has set
the flags: at the start of the program, or after a call.

```nasm
mov rax, [count]
jz .empty           ; mov does not set the flags
```

Data moves (`mov`, `lea`, `movzx`, `push`, `pop`) leave the flags alone.
Compare explicitly with `test rax, rax` or `cmp`.

### unknown-syscall

`rax` holds a number that is not a Linux x86-64 system call. The usual
cause is the 32-bit numbering (1 `exit`, 4 `write`), which only applies
to `int 0x80`.

### exit-code-truncated

`exit` is called with a status outside 0-255. Only the low 8 bits reach
the parent process: 256 becomes 0, which reads as success.

### pointer-from-contents

A system call argument that must be a pointer is loaded with the contents
of a byte buffer instead of its address.

```nasm
mov rsi, [message]  ; the first 8 bytes of the text
mov rsi, message    ; its address: what write expects
```

### truncated-address

A 32-bit register is used as an address, as in `[eax]`. The upper half of
the pointer is dropped: it works for addresses below 4 GiB and crashes
with a real stack or heap pointer. Not reported for `lea`, where 32-bit
registers are a common arithmetic trick.

### data-size-mismatch

An access is wider than the data it names.

```nasm
flag db 0
    mov rax, [flag]     ; reads 8 bytes from a 1-byte variable
    mov [flag], rax     ; writes 8 bytes: overwrites what follows
```

Only reported for a direct `[label]` whose definition has a known size.

### write-to-readonly

A store to a label defined in `.rodata` or in `.text`. Both are mapped
read-only: the store faults at run time.

### shift-out-of-range

A shift or rotate by a constant at least as large as its operand. The
processor masks the count to 5 bits (6 for 64-bit operands), so
`shl rax, 64` shifts by 0.

### code-in-data-section

Instructions appear after `section .data` or `section .bss`. They are
assembled, but into a section that is not executable: usually a forgotten
`section .text`.

### data-in-bss

`.bss` only reserves space. An initialised definition there (`db`, `dq`)
has its values ignored: use `resb`-style reservations, or `.data`.

### unknown-section

A section name one typo away from a standard one: `text` without the dot,
`.txt`, `.date`, `.bbs`, `.rodata:`. The assembler accepts any name and
creates a new section with default attributes, which is not executable.

### misplaced-length

`len equ $ - msg` measures from `msg` to the current position. Placed
after another definition, it counts that one too.

```nasm
first  db "hello"
second db "world"
first_len equ $ - first     ; 10, not 5
```

### mismatched-pop

Registers are popped in the order they were pushed instead of the reverse,
which swaps their values.

```nasm
push rbx
push r12
pop rbx         ; receives the saved r12
pop r12         ; receives the saved rbx
```

`push rax` / `pop rdi`, used as a move, is not reported.

### odd-stack-adjustment

`sub rsp, 4` or `add rsp, 12`: `rsp` is moved by an amount that is not a
multiple of 8, the size of every stack slot in 64-bit mode.

### variadic-call-without-al

`printf`, `scanf` and the other variadic functions read `al` as the number
of vector registers that carry arguments. It is reported when `al` still
holds the result of a previous call, or was never set in the function.
Put `xor eax, eax` before the call.

### main-return-value

`main` reaches `ret` with `eax` holding the result of its last call, or
never set. That value becomes the exit status: a program that prints 12
characters with `printf` and returns exits with status 12.

### constant-comparison

A comparison whose outcome never varies: `cmp rax, rax` is always equal,
`test rax, 0` always gives zero.

### callee-saved-clobbered

An exported function (`global`, or `main`) changes `rbx`, `rbp` or
`r12`-`r15` and returns without restoring it. Callers compiled from C rely
on these registers surviving a call.

### misaligned-call

An `extern` function is called while `rsp` is not a multiple of 16. The
System V ABI requires it, and library functions that use SSE crash
otherwise (the classic segfault inside `printf`).

A function is entered with `rsp` 8 bytes off, because of the return
address: push an odd number of registers, or `sub rsp, 8`.

### dead-store

A register is written, then written again before anything read the first
value.

```nasm
mov rax, 1          ; never read
mov rax, 2
```

Also reported when the value is destroyed by a `syscall`: putting the
fourth system call argument in `rcx` instead of `r10` is the usual cause.

### flags-clobbered

A conditional jump follows a `cmp`, but an instruction in between
overwrote the flags it tests.

```nasm
cmp rax, 10
add rbx, 1          ; sets the flags
je .ten             ; tests the add, not the cmp
```

`mov`, `lea`, `push`, `pop` and `not` leave the flags alone; `inc` and
`dec` keep the carry flag only.

### unused-comparison

A `cmp` or `test` whose flags are overwritten before anything reads them.

### fallthrough

Execution runs past the last instruction of a function: into the next
function, or off the end of the section into whatever bytes follow.

A final `syscall` is accepted when its number cannot be determined, since
it may be `exit`.

### unreachable-code

Instructions that follow a `jmp`, a `ret` or an exit system call, with no
label through which they could be reached.

### int80-in-64bit

`int 0x80` is the 32-bit system call gate. From 64-bit code it truncates
every pointer to 32 bits and uses a different numbering. Use `syscall`.

### entry-not-global

`_start` is defined but not exported, so the linker cannot find the entry
point and falls back to the start of the section.

## Style

### unused-label

A label or a data item that nothing refers to.

### unused-extern

An `extern` declaration for a symbol the file never uses.

### redundant-jump

A `jmp` to the instruction that immediately follows it.

### jump-over-jump

```nasm
    jne .skip
    jmp .target
.skip:
```

The conditional jump only skips the `jmp`: `je .target` does the same.

### jump-to-jump

A jump lands on a label whose instruction is another unconditional jump.
Jump to the final target directly.

### lea-as-mov

`lea rax, [rbx]` computes the address of nothing: it is `mov rax, rbx`.

### redundant-test

```nasm
dec rcx
test rcx, rcx       ; dec already set ZF and SF for rcx
jz .done
```

Arithmetic sets the zero and sign flags from its result. Only reported
when what follows reads those flags alone: the carry and overflow flags
do differ between the arithmetic and a `test`.

### redundant-push-pop

`push rax` immediately followed by `pop rax` does nothing.

### redundant-move

`mov rax, rbx` followed by `mov rbx, rax`, or a store followed by a load
of the same location: the second copies back what the first just copied.

### mov-zero

`mov reg, 0` can be `xor reg32, reg32`: fewer bytes, and the form
processors recognise as zeroing. Only suggested where nothing reads the
flags afterwards, because `xor` changes them and `mov` does not.

### cmp-zero

`cmp reg, 0` can be `test reg, reg`: same flags, one byte less.

### self-move

`mov reg, reg` with the same register on both sides does nothing. The
32-bit form, `mov eax, eax`, is not reported: it clears the upper half of
`rax`.

### noop-arithmetic

`add reg, 0`, `shl reg, 0`, `and reg, -1`, `imul reg, 1`: the operand is
left unchanged and only the flags are affected.

## Pedantic

### needless-rex

`xor rax, rax` can be `xor eax, eax`: writing a 32-bit register clears the
upper half, so the result is the same and the REX prefix byte is saved.
Not reported for `r8`-`r15`, which need the prefix either way.

`mov rax, 1` is not reported: NASM already encodes it as `mov eax, 1`.

### slow-loop

The `loop` instruction is microcoded and slower than `dec rcx` followed by
`jnz` on current processors.

### absolute-address

`[label]` without `rel` is assembled as an absolute 32-bit address.
Position-independent executables, the default on most Linux
distributions, need `[rel label]`, or `default rel` once at the top of the
file.

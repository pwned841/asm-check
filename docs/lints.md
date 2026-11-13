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
and rax, 0xFFFFFFFF     ; immediates are 32-bit and sign-extended
```

The last one assembles with a warning and does something else than intended:
only `mov r64, imm64` takes a 64-bit immediate.

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

### redundant-jump

A `jmp` to the instruction that immediately follows it.

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

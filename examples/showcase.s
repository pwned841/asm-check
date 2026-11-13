; asm-check showcase: every lint, one paragraph each.
;
; Open this file in an editor with the language server running, or run
;     asm-check --pedantic examples/showcase.s
; Each paragraph names the lint it triggers. The file is not meant to run.

global _start, exported, missing_symbol         ; undefined-symbol: `missing_symbol` is never defined
extern printf

section .text

; ---------------------------------------------------------------- operands

operands:
    mov rax, ebx                ; operand-size-mismatch: 64-bit and 32-bit
    shl rax, rbx                ; operand-size-mismatch: the count must be cl
    mov [buffer], 0             ; ambiguous-operand-size: 1, 2, 4 or 8 bytes?
    mov [buffer], [value]       ; invalid-operands: two memory operands
    mov value, rax              ; invalid-operands: a label is not a location
    push eax                    ; invalid-operands: no 32-bit push in 64-bit mode
    and rax, 0xFFFFFFFF         ; invalid-operands: sign-extended 32-bit immediate
    mov rsi, mesage             ; undefined-symbol: did you mean `message`?
    ret

; ------------------------------------------------------------------- stack

leaky:
    push rbx
    mov rbx, rdi
    ret                         ; unbalanced-stack: rbx is still on the stack

exported:
    mov r12, rdi                ; callee-saved-clobbered: r12 is not restored
    lea rdi, [rel message]
    xor eax, eax
    call printf                 ; misaligned-call: rsp is 8 bytes off
    ret

; ---------------------------------------------------------------- division

division:
    mov rax, 100
    mov rcx, 7
    div rcx                     ; div-without-rdx: rdx was never set
    xor edx, edx
    idiv rcx                    ; idiv-zero-extended: use cqo for a signed division
    ret

; --------------------------------------------------------------- registers

registers:
    mov rcx, 3
.again:
    mov eax, 1
    mov edi, 1
    lea rsi, [rel message]
    mov edx, 6
    syscall
    dec rcx                     ; clobbered-register: syscall destroyed rcx
    jnz .again

    mov rbx, 1                  ; dead-store: overwritten before being read
    mov rbx, 2
    mov rax, rbx
    ret

; ------------------------------------------------------------ system calls

syscalls:
    mov eax, 1
    mov edi, 1
    mov rsi, [message]          ; pointer-from-contents: the text, not its address
    mov edx, 6
    syscall
    mov eax, 400
    syscall                     ; unknown-syscall: no such number
    mov edi, 256
    mov eax, 60
    syscall                     ; exit-code-truncated: the shell sees 0

; ------------------------------------------------------------------- flags

flags:
    cmp rax, 10
    add rbx, 1
    je .ten                     ; flags-clobbered: tests the add, not the cmp
    cmp rax, 5                  ; unused-comparison: overwritten by the next cmp
    cmp rax, 6
    jne .ten
    nop
.ten:
    ret

; ------------------------------------------------------------ control flow

control:
    jmp .next                   ; redundant-jump: goes to the next instruction
.next:
    mov eax, 4
    int 0x80                    ; int80-in-64bit: the 32-bit system call gate
    jmp .end
    mov rax, 1                  ; unreachable-code: nothing jumps here
.end:
    mov rax, 2                  ; fallthrough: runs into `style`

; ------------------------------------------------------------------- style

style:
    mov rax, 0                  ; mov-zero: xor eax, eax
    cmp rbx, 0                  ; cmp-zero: test rbx, rbx
    mov rcx, rcx                ; self-move: does nothing
    add rdx, 0                  ; noop-arithmetic: leaves rdx unchanged
    xor rsi, rsi                ; needless-rex (pedantic): xor esi, esi
    ret

orphan:                         ; unused-label: nothing refers to it
    ret

duplicate:
    ret
duplicate:                      ; duplicate-label
    ret

; ------------------------------------------------------------- entry point

_start:
    add rbx, 1                  ; uninitialized-register: rbx was never set
    mov al, [rsp]
    cmp rax, 2                  ; partial-register-read: only al was written
    call operands
    mov rax, [value]
    jz .done                    ; undefined-flags: neither the call nor mov sets them
    call leaky
    call division
    call registers
    call flags
    call control
    call style
    call duplicate
    call syscalls
.done:
    ret                         ; ret-in-start: there is no return address

section .data
message db "hello", 10
value   dq 0

section .bss
buffer  resb 64

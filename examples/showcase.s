; asm-check showcase: every lint, one paragraph each.
;
; Open this file in an editor with the language server running, or run
;     asm-check --pedantic examples/showcase.s
; Each paragraph names the lint it triggers. The file is not meant to run.

global _start, exported, missing_symbol         ; undefined-symbol: `missing_symbol` is never defined
extern printf
extern puts                     ; unused-extern: never called

section .text

; ---------------------------------------------------------------- operands

operands:
    mov rax, ebx                ; operand-size-mismatch: 64-bit and 32-bit
    shl rax, rbx                ; operand-size-mismatch: the count must be cl
    mov [buffer], 0             ; ambiguous-operand-size: 1, 2, 4 or 8 bytes?
    mov [buffer], [value]       ; invalid-operands: two memory operands
    mov value, rax              ; invalid-operands: a label is not a location
    push eax                    ; invalid-operands: no 32-bit push in 64-bit mode
    mov rsi, mesage             ; undefined-symbol: did you mean `message`?
    ret

; ------------------------------------------------------------------ memory

memory:
    mov rax, [bx]               ; invalid-address: 16-bit register in an address
    mov rax, [eax]              ; truncated-address: 32-bit pointer
    mov rax, [flag]             ; data-size-mismatch: 8 bytes from a 1-byte variable
    mov byte [greeting], 'H'    ; write-to-readonly: `greeting` is in .rodata
    mov al, 300                 ; immediate-out-of-range: al holds 0-255
    and rax, 0xFFFFFFFF         ; immediate-out-of-range: sign-extended, masks nothing
    shl rax, 64                 ; shift-out-of-range: shifts by 0
    call message                ; jump-to-data: `message` is not code
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

swapped:
    push rbx
    push r12
    pop rbx                     ; mismatched-pop: receives the saved r12
    pop r12
    ret

main:
    lea rdi, [rel message]
    call printf                 ; variadic-call-without-al: al was never set
    ret                         ; main-return-value: returns what printf returned

; ---------------------------------------------------------------- division

division:
    sub rsp, 4                  ; odd-stack-adjustment: not a multiple of 8
    add rsp, 4
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
    xor eax, eax
    mov rsi, buffer
    mov edx, 128
    syscall                     ; buffer-overflow: 128 bytes into a 64-byte buffer
    mov eax, 2
    mov rdi, greeting
    syscall                     ; missing-nul-terminator: `greeting` has no final 0
.wait:
    cmp rbx, 5
    jne .wait                   ; infinite-loop: nothing changes rbx
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

comparisons:
    cmp rax, rax                ; constant-comparison: always equal
    dec rcx
    test rcx, rcx               ; redundant-test: dec already set the flags
    jz .out
    jne .skip                   ; jump-over-jump: use `je .hop`
    jmp .hop                    ; jump-to-jump: .hop only jumps on to .out
.skip:
    nop
.hop:
    jmp .out
.out:
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
    lea rax, [rbx]              ; lea-as-mov: mov rax, rbx
    push rax                    ; redundant-push-pop: undone by the pop
    pop rax
    mov rax, rbx
    mov rbx, rax                ; redundant-move: copies back the same value
    mvo rax, 1                  ; unknown-instruction: did you mean `mov`?
    mov rax, [value]            ; absolute-address (pedantic): use [rel value]
.spin:
    loop .spin                  ; slow-loop (pedantic): dec rcx / jnz
    xor rsi, rsi                ; needless-rex (pedantic): xor esi, esi
    ret

orphan:                         ; unused-label: nothing refers to it
    ret

duplicate:
    ret
duplicate:                      ; duplicate-label
    ret

; ------------------------------------------------------------- performance

performance:
    mov edi, 1
    mov rsi, message
    mov ebx, 3
.repeat:
    mov eax, 1
    mov edi, 1                  ; redundant-load: rdi never changes inside the loop
    mov edx, 6
    syscall
    dec ebx
    jnz .repeat

    xor edx, edx
    mov rbx, 8
    mov rax, rcx
    div rbx                     ; div-by-power-of-two: shr rax, 3 and and rdx, 7
    imul rax, 16                ; mul-by-power-of-two: shl rax, 4
    add rax, 1                  ; fold-constants: add rax, 3
    add rax, 2
    mov rbx, rax                ; use-lea: lea rbx, [rax + rcx]
    add rbx, rcx
    mov rax, [value]
    mov rax, [value]            ; redundant-memory-load: already in rax
    mov byte [buffer], 'H'      ; mergeable-stores: one 16-bit store
    mov byte [buffer + 1], 'i'
    mov eax, 5
    cmp eax, 5
    jne .never                  ; constant-condition: eax is always 5
    jmp .quit                   ; jump-to-ret: return from here
.never:
    nop
.quit:
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
    call memory
    call swapped
    call main
    call comparisons
    call performance
.done:
    ret                         ; ret-in-start: there is no return address

section .data
message db "hello", 10
value   dq 0
flag    db 0
message_len equ $ - message     ; misplaced-length: also counts `value` and `flag`

section .rodata
greeting db "hi", 10

section .date                   ; unknown-section: did you mean .data?
typo    dq 0

section .bss
buffer  resb 64
counter dq 0                    ; data-in-bss: the value is ignored
    inc qword [counter]         ; code-in-data-section: not executable here

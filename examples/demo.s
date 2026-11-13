; A program with one bug per paragraph. Run `asm-check examples/demo.s`.
global _start

section .text
_start:
    ; Prints the message three times... or not: `syscall` destroys rcx.
    mov rcx, 3
.again:
    mov rax, 1
    mov rdi, 1
    mov rsi, message
    mov rdx, message_len
    syscall
    dec rcx
    jnz .again

    ; Average of two numbers, with rdx left over from the write above.
    mov rax, [first]
    add rax, [second]
    mov rbx, 2
    div rbx

    ; The comparison is lost before the jump tests it.
    cmp rax, 10
    add rax, 1
    je .ten

    mov [result], 0
.ten:
    mov rdi, 0
    mov rax, 60
    syscall
    ret

section .data
message db "hello", 10
message_len equ $ - message
first dq 4
second dq 8
result dq 0

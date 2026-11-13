global _start
global missing              ;~
extern printf

section .text
_start:
    mov rsi, mesage         ;~
    mov rdi, [bufer + 8]    ;~
    call printf
    call puts               ;~
    jmp .done
.done:
    jmp .nope               ;~
    mov rax, LEN
    mov rsi, message
    mov rdi, buffer
    mov rax, 60
    syscall

section .data
message db "hi"
LEN equ $ - message

section .bss
buffer resb 8

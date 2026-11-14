global _start
section .text
_start:
    mov edi, 1
    mov rsi, message
    mov edx, 6
    mov r12d, 3
.again:
    mov eax, 1
    mov edi, 1              ;~
    mov rsi, message        ;~
    syscall
    dec r12
    jnz .again
    mov eax, 1
    mov edi, 1
    mov rsi, message
    syscall
    mov r13d, 2
.count:
    mov edx, 6              ;~
    xor ebx, ebx
    cmp r13d, 1
    xor ebx, ebx            ;~
    dec r13
    jnz .count
    mov eax, 60
    xor edi, edi
    syscall

section .data
message db "hello", 10

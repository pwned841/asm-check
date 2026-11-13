global _start
section .text
_start:
    mov eax, 1
    mov edi, 1
    mov rsi, message
    mov edx, 20
    syscall                 ;~
    mov eax, 1
    lea rsi, [rel message]
    mov edx, 6
    syscall
    xor eax, eax
    xor edi, edi
    mov rsi, buffer
    mov edx, 128
    syscall                 ;~
    xor eax, eax
    mov rsi, buffer
    mov edx, 64
    syscall
    mov eax, 1
    mov rsi, long
    mov edx, 12
    syscall
    mov eax, 60
    syscall

section .data
message db "hello", 10
long db "first "
     db "second"

section .bss
buffer resb 64

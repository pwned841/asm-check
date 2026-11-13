global _start
section .text
_start:
    mov eax, 400
    syscall                 ;~
    mov rax, -1
    syscall                 ;~
    mov eax, 9999
    syscall                 ;~
    mov eax, 1
    syscall
    mov eax, 25
    syscall
    mov rax, rdi
    syscall

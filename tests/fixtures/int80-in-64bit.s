global _start
section .text
_start:
    mov eax, 4
    int 0x80                ;~
    int 80h                 ;~
    int 3
    mov rax, 60
    syscall

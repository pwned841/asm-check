global _start
section .text
_start:
    call helper
    cmp rax, 0
    je .quit
    ret                     ;~
.quit:
    mov rax, 60
    syscall
helper:
    ret

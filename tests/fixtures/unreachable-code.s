global _start
section .text
_start:
    jmp .next
    mov rax, 1              ;~
    mov rbx, 2
.next:
    mov rax, 60
    syscall
    ret                     ;~
helper:
    ret
    nop                     ;~
    jmp helper
    nop

global _start, first, quit, tail, maybe
section .text
_start:
    call first
    mov rax, 1
    mov rdi, 1
    syscall                 ;~
first:
    mov rax, 2
    ret
quit:
    mov rax, 60
    syscall
tail:
    call first
    mov rax, 1              ;~

section .data
value dq 0

section .text
maybe:
    mov rax, rdi
    syscall

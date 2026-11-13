global _start
extern rand
section .text
_start:
    mov rax, 100
    mov rcx, 7
    div rcx                 ;~
    xor edx, edx
    div rcx
    div rcx                 ;~
    mov rax, 5
    cqo
    idiv rcx
    mov rdx, 0
    div rcx
    mov al, 9
    div cl
    call rand
    div rcx                 ;~
.loop:
    xor edx, edx
    div rcx
    test rax, rax
    jnz .loop
    xor edx, edx
.bad:
    div rcx                 ;~
    dec rbx
    jnz .bad
    mul rcx
    div rcx
    mov rax, 60
    syscall
